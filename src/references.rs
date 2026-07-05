use std::{collections::BTreeSet, fs, path::Path};

use tower_lsp::lsp_types::{Location, Url, WorkspaceFolder};

use crate::{
    ast::{
        AstFile, Directive, DocumentItem, LetBinding, MappingEntry, Node, Scalar, ScalarKind,
        Sequence, TagNode,
    },
    config::LumalsConfig,
    document::DocumentSnapshot,
    imports::resolve_guarded_import,
    parser,
    position::LineIndex,
    semantic::SemanticDocument,
    symbols::{Definition, DefinitionKind},
    syntax::{FileId, ParsedFile, SourceSpan, SourceText},
    workspace::{effective_roots, file_url_to_path, is_workspace_luma_uri},
};

#[derive(Debug, Clone, Copy)]
pub struct ReferencesRequest<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub file_id: FileId,
    pub offset: usize,
    pub include_declaration: bool,
    pub workspace_folders: &'a [WorkspaceFolder],
    pub config: &'a LumalsConfig,
    pub open_documents: &'a [DocumentSnapshot],
}

pub fn find_references(request: ReferencesRequest<'_>) -> Vec<Location> {
    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let source = parsed.source.clone();
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return Vec::new(),
    };
    let semantic = SemanticDocument::from_ast(file);
    let workspace = WorkspaceCorpus::load(request);
    let Some(target) = reference_target(
        file,
        request.offset,
        &source,
        &semantic.definitions,
        request,
        &workspace,
    ) else {
        return Vec::new();
    };

    let mut locations = workspace
        .files
        .iter()
        .flat_map(|file| collect_file_references(file, &target, request))
        .collect::<Vec<_>>();
    if !request.include_declaration {
        locations.retain(|location| !is_declaration_location(location, &target, &workspace));
    }
    unique_locations(locations)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceTarget {
    Let { uri: Url, name: String },
    Alias { uri: Url, name: String },
    ImportTarget { uri: Url },
    TypeLike { name: String },
    KeyPath { uri: Url, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentifierPath {
    segments: Vec<String>,
    selected_segment: usize,
}

#[derive(Debug, Clone)]
struct StaticTarget {
    uri: Url,
    path_prefix: Vec<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceFileEntry {
    uri: Url,
    text: String,
    definitions: Vec<Definition>,
    ast: AstFile,
    file_id: FileId,
}

#[derive(Debug, Clone, Default)]
struct WorkspaceCorpus {
    files: Vec<WorkspaceFileEntry>,
}

impl WorkspaceCorpus {
    fn load(request: ReferencesRequest<'_>) -> Self {
        let mut files = Vec::new();
        let mut seen = BTreeSet::new();

        for document in request.open_documents {
            if seen.insert(document.uri.clone()) {
                files.push(file_entry_from_text(
                    document.uri.clone(),
                    document.text.clone(),
                    document.file_id,
                ));
            }
        }

        if seen.insert(request.uri.clone()) {
            files.push(file_entry_from_text(
                request.uri.clone(),
                request.text.to_owned(),
                request.file_id,
            ));
        }

        if request.config.index_workspace {
            let open_paths = seen
                .iter()
                .filter_map(file_url_to_path)
                .collect::<BTreeSet<_>>();
            let mut next_file_id = request.file_id.0.saturating_add(1 + files.len() as u32);
            for root in effective_roots(request.workspace_folders, request.config) {
                let mut indexed = 0u32;
                walk_workspace(root.as_path(), &mut |path| {
                    if indexed >= request.config.max_indexed_files_per_workspace
                        || open_paths.contains(path)
                    {
                        return;
                    }
                    let Ok(uri) = Url::from_file_path(path) else {
                        return;
                    };
                    if !is_workspace_luma_uri(&uri, request.workspace_folders, request.config) {
                        return;
                    }
                    let Ok(metadata) = fs::metadata(path) else {
                        return;
                    };
                    if metadata.len() > u64::from(request.config.max_indexed_file_bytes) {
                        return;
                    }
                    let Ok(text) = fs::read_to_string(path) else {
                        return;
                    };
                    files.push(file_entry_from_text(uri, text, FileId(next_file_id)));
                    next_file_id = next_file_id.saturating_add(1);
                    indexed = indexed.saturating_add(1);
                });
            }
        }

        Self { files }
    }
}

fn reference_target(
    file: &AstFile,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: ReferencesRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Option<ReferenceTarget> {
    for definition in definitions {
        if definition.kind == DefinitionKind::Key && contains(definition.selection_span, offset) {
            return Some(ReferenceTarget::KeyPath {
                uri: request.uri.clone(),
                path: definition.path.to_string(),
            });
        }
    }

    for document in &file.documents {
        for item in &document.items {
            let result = match item {
                DocumentItem::Directive(directive) => {
                    directive_target_for_offset(directive, offset, source, request)
                }
                DocumentItem::Let(binding) => {
                    let_target_for_offset(binding, offset, source, definitions, request, workspace)
                }
                DocumentItem::Node(node) => {
                    node_target_for_offset(node, offset, source, definitions, request, workspace)
                }
                DocumentItem::Comment(_) => None,
            };
            if result.is_some() {
                return result;
            }
        }
    }

    None
}

fn directive_target_for_offset(
    directive: &Directive,
    offset: usize,
    source: &SourceText,
    request: ReferencesRequest<'_>,
) -> Option<ReferenceTarget> {
    if matches!(directive.name.as_str(), "@import" | "@include" | "@use")
        && directive_target_span(directive, source).is_some_and(|span| contains(span, offset))
    {
        let target = directive_target(directive)?;
        let uri = resolve_guarded_import(
            request.uri,
            &target,
            request.workspace_folders,
            request.config,
        )
        .ok()?;
        return Some(ReferenceTarget::ImportTarget { uri });
    }

    if matches!(directive.name.as_str(), "@schema" | "@profile")
        && directive_value_span(directive, source).is_some_and(|span| contains(span, offset))
    {
        return directive
            .value
            .as_deref()
            .map(|value| ReferenceTarget::TypeLike {
                name: normalize_name(value).to_owned(),
            });
    }

    None
}

fn let_target_for_offset(
    binding: &LetBinding,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: ReferencesRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Option<ReferenceTarget> {
    if let Some(name_span) = let_name_span(binding)
        && contains(name_span, offset)
    {
        return Some(ReferenceTarget::Let {
            uri: request.uri.clone(),
            name: binding.name.clone(),
        });
    }

    let value_span = binding.value_span?;
    if !contains(value_span, offset) {
        return None;
    }
    let trimmed = trimmed_value_span(source, value_span)?;
    let path = identifier_path_at_span(trimmed, offset, source)?;
    resolve_identifier_target(path, definitions, request, workspace)
}

fn node_target_for_offset(
    node: &Node,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: ReferencesRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Option<ReferenceTarget> {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                if contains(entry.key_span, offset)
                    && let Some(definition) = definitions.iter().find(|definition| {
                        definition.kind == DefinitionKind::Key
                            && definition.selection_span == entry.key_span
                    })
                {
                    return Some(ReferenceTarget::KeyPath {
                        uri: request.uri.clone(),
                        path: definition.path.to_string(),
                    });
                }
                if let Some(metadata_span) = entry_metadata_span(entry, source)
                    && contains(metadata_span, offset)
                {
                    return source
                        .slice(metadata_span)
                        .map(|name| ReferenceTarget::TypeLike {
                            name: name.to_owned(),
                        });
                }
                if let Some(value) = &entry.value
                    && let Some(target) = node_target_for_offset(
                        value,
                        offset,
                        source,
                        definitions,
                        request,
                        workspace,
                    )
                {
                    return Some(target);
                }
            }
            None
        }
        Node::Sequence(Sequence { items, .. }) => items
            .iter()
            .filter_map(|item| item.value.as_deref())
            .find_map(|value| {
                node_target_for_offset(value, offset, source, definitions, request, workspace)
            }),
        Node::Scalar(Scalar {
            kind: ScalarKind::LuaExpression,
            span,
            ..
        }) => {
            let path = identifier_path_at_span(*span, offset, source)?;
            resolve_identifier_target(path, definitions, request, workspace)
        }
        Node::Tag(tag) => {
            if let Some(name_span) = tag_name_span(tag, source)
                && contains(name_span, offset)
            {
                return Some(ReferenceTarget::TypeLike {
                    name: tag.name.clone(),
                });
            }
            tag.value.as_deref().and_then(|value| {
                node_target_for_offset(value, offset, source, definitions, request, workspace)
            })
        }
        _ => None,
    }
}

fn resolve_identifier_target(
    path: IdentifierPath,
    definitions: &[Definition],
    request: ReferencesRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Option<ReferenceTarget> {
    let base = path.segments.first()?;
    if path.selected_segment == 0 {
        if definitions
            .iter()
            .any(|definition| definition.kind == DefinitionKind::Alias && definition.name == *base)
        {
            return Some(ReferenceTarget::Alias {
                uri: request.uri.clone(),
                name: base.clone(),
            });
        }
        if definitions
            .iter()
            .any(|definition| definition.kind == DefinitionKind::Let && definition.name == *base)
        {
            return Some(ReferenceTarget::Let {
                uri: request.uri.clone(),
                name: base.clone(),
            });
        }
        if path.segments.len() == 1
            && let Some(definition) = definitions.iter().find(|definition| {
                definition.kind == DefinitionKind::Key && definition.name == *base
            })
        {
            return Some(ReferenceTarget::KeyPath {
                uri: request.uri.clone(),
                path: definition.path.to_string(),
            });
        }
    }

    let target = static_target_for_identifier(base, definitions, request, workspace, 0)?;
    let mut resolved = target.path_prefix;
    resolved.extend(
        path.segments
            .into_iter()
            .skip(1)
            .take(path.selected_segment),
    );
    Some(ReferenceTarget::KeyPath {
        uri: target.uri,
        path: dotted_key_path(&resolved),
    })
}

fn collect_file_references(
    file: &WorkspaceFileEntry,
    target: &ReferenceTarget,
    request: ReferencesRequest<'_>,
) -> Vec<Location> {
    let source = SourceText::new(file.file_id, file.uri.as_str(), file.text.clone());
    let mut locations = Vec::new();

    for document in &file.ast.documents {
        for item in &document.items {
            collect_item_references(item, file, &source, target, request, &mut locations);
        }
    }

    if let ReferenceTarget::TypeLike { name } = target {
        collect_tag_name_occurrences(file, name, &mut locations);
    }

    locations
}

fn collect_item_references(
    item: &DocumentItem,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &ReferenceTarget,
    request: ReferencesRequest<'_>,
    locations: &mut Vec<Location>,
) {
    match item {
        DocumentItem::Directive(directive) => {
            collect_directive_references(directive, file, source, target, request, locations)
        }
        DocumentItem::Let(binding) => {
            collect_let_references(binding, file, source, target, request, locations)
        }
        DocumentItem::Node(node) => {
            collect_node_references(node, file, source, target, request, locations)
        }
        DocumentItem::Comment(_) => {}
    }
}

fn collect_directive_references(
    directive: &Directive,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &ReferenceTarget,
    request: ReferencesRequest<'_>,
    locations: &mut Vec<Location>,
) {
    match target {
        ReferenceTarget::Alias { uri, name } if &file.uri == uri => {
            if let Some(span) = directive_alias_span(directive, source)
                && source.slice(span) == Some(name)
            {
                push_location(locations, &file.uri, span, &file.text);
            }
        }
        ReferenceTarget::ImportTarget { uri } => {
            if matches!(directive.name.as_str(), "@import" | "@include" | "@use")
                && let Some(target_path) = directive_target(directive)
                && let Ok(resolved) = resolve_guarded_import(
                    &file.uri,
                    &target_path,
                    request.workspace_folders,
                    request.config,
                )
                && &resolved == uri
                && let Some(span) = directive_target_span(directive, source)
            {
                push_location(locations, &file.uri, span, &file.text);
            }
        }
        ReferenceTarget::TypeLike { name } => {
            if matches!(directive.name.as_str(), "@schema" | "@profile")
                && directive
                    .value
                    .as_deref()
                    .is_some_and(|value| normalize_name(value) == name)
                && let Some(span) = directive_value_span(directive, source)
            {
                push_location(locations, &file.uri, span, &file.text);
            }
        }
        _ => {}
    }
}

fn collect_let_references(
    binding: &LetBinding,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &ReferenceTarget,
    request: ReferencesRequest<'_>,
    locations: &mut Vec<Location>,
) {
    match target {
        ReferenceTarget::Let { uri, name } if &file.uri == uri && &binding.name == name => {
            if let Some(span) = let_name_span(binding) {
                push_location(locations, &file.uri, span, &file.text);
            }
        }
        _ => {}
    }

    if let Some(span) = binding.value_span {
        collect_expression_reference(span, file, source, target, request, locations);
    }
}

fn collect_node_references(
    node: &Node,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &ReferenceTarget,
    request: ReferencesRequest<'_>,
    locations: &mut Vec<Location>,
) {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                collect_entry_references(entry, file, source, target, request, locations);
            }
        }
        Node::Sequence(Sequence { items, .. }) => {
            for item in items {
                if let Some(value) = &item.value {
                    collect_node_references(value, file, source, target, request, locations);
                }
            }
        }
        Node::Scalar(Scalar {
            kind: ScalarKind::LuaExpression,
            span,
            ..
        }) => {
            collect_expression_reference(*span, file, source, target, request, locations);
        }
        Node::Tag(tag) => {
            if let ReferenceTarget::TypeLike { name } = target
                && &tag.name == name
                && let Some(span) = tag_name_span(tag, source)
            {
                push_location(locations, &file.uri, span, &file.text);
            }
            if let Some(value) = &tag.value {
                collect_node_references(value, file, source, target, request, locations);
            }
        }
        _ => {}
    }
}

fn collect_entry_references(
    entry: &MappingEntry,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &ReferenceTarget,
    request: ReferencesRequest<'_>,
    locations: &mut Vec<Location>,
) {
    if let ReferenceTarget::KeyPath { uri, path } = target
        && &file.uri == uri
        && file.definitions.iter().any(|definition| {
            definition.kind == DefinitionKind::Key
                && definition.selection_span == entry.key_span
                && definition.path.to_string() == *path
        })
    {
        push_location(locations, &file.uri, entry.key_span, &file.text);
    }

    if let Some(metadata_span) = entry_metadata_span(entry, source)
        && let ReferenceTarget::TypeLike { name } = target
        && source.slice(metadata_span) == Some(name)
    {
        push_location(locations, &file.uri, metadata_span, &file.text);
    }

    if let Some(value) = &entry.value {
        collect_node_references(value, file, source, target, request, locations);
    }
}

fn collect_expression_reference(
    span: SourceSpan,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &ReferenceTarget,
    request: ReferencesRequest<'_>,
    locations: &mut Vec<Location>,
) {
    let Some(text) = source.slice(span) else {
        return;
    };
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if !is_ident(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < bytes.len() && is_path(bytes[index]) {
            index += 1;
        }
        let token = &text[start..index];
        let segments = token
            .split('.')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            continue;
        }
        for segment_index in 0..segments.len() {
            if let Some(segment_span) = segment_span_in_token(span, token, start, segment_index) {
                match target {
                    ReferenceTarget::Let { uri, name }
                        if &file.uri == uri && segments[0] == name =>
                    {
                        if segment_index == 0 {
                            push_location(locations, &file.uri, segment_span, &file.text);
                        }
                    }
                    ReferenceTarget::Alias { uri, name }
                        if &file.uri == uri && segments[0] == name =>
                    {
                        if segment_index == 0 {
                            push_location(locations, &file.uri, segment_span, &file.text);
                        }
                    }
                    ReferenceTarget::KeyPath { uri, path } => {
                        if segment_index > 0
                            && let Some(resolved) = resolve_expression_key_path(
                                file,
                                segments.as_slice(),
                                segment_index,
                                request,
                            )
                            && &resolved.0 == uri
                            && resolved.1 == *path
                        {
                            push_location(locations, &file.uri, segment_span, &file.text);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn resolve_expression_key_path(
    file: &WorkspaceFileEntry,
    segments: &[&str],
    segment_index: usize,
    request: ReferencesRequest<'_>,
) -> Option<(Url, String)> {
    let base = *segments.first()?;
    let target = static_target_for_identifier_in_file(base, file, request, 0)?;
    let mut path = target.path_prefix;
    path.extend(
        segments
            .iter()
            .skip(1)
            .take(segment_index)
            .map(|segment| (*segment).to_owned()),
    );
    Some((target.uri, dotted_key_path(&path)))
}

fn static_target_for_identifier_in_file(
    ident: &str,
    file: &WorkspaceFileEntry,
    request: ReferencesRequest<'_>,
    depth: u32,
) -> Option<StaticTarget> {
    if depth >= request.config.max_resolve_depth {
        return None;
    }
    if let Some(alias) = file
        .definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Alias && definition.name == ident)
    {
        let uri = resolve_guarded_import(
            &file.uri,
            alias.detail.as_deref()?,
            request.workspace_folders,
            request.config,
        )
        .ok()?;
        return Some(StaticTarget {
            uri,
            path_prefix: Vec::new(),
        });
    }

    let binding = file
        .definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Let && definition.name == ident)?;
    let value_span = parse_let_value_span(binding.detail.as_deref()?)?;
    let source = SourceText::new(file.file_id, file.uri.as_str(), file.text.clone());
    let value_text = trimmed_value_span(&source, value_span)
        .and_then(|span| file.text.get(span.start..span.end))?;
    let path = parse_static_expression_path(value_text)?;
    let base = path.segments.first()?;
    let mut target = static_target_for_identifier_in_file(base, file, request, depth + 1)?;
    target.path_prefix.extend(path.segments.into_iter().skip(1));
    Some(target)
}

fn static_target_for_identifier(
    ident: &str,
    definitions: &[Definition],
    request: ReferencesRequest<'_>,
    _workspace: &WorkspaceCorpus,
    depth: u32,
) -> Option<StaticTarget> {
    if depth >= request.config.max_resolve_depth {
        return None;
    }

    if let Some(alias) = definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Alias && definition.name == ident)
    {
        let target = alias.detail.as_deref()?;
        let uri = resolve_guarded_import(
            request.uri,
            target,
            request.workspace_folders,
            request.config,
        )
        .ok()?;
        return Some(StaticTarget {
            uri,
            path_prefix: Vec::new(),
        });
    }

    let binding = definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Let && definition.name == ident)?;
    let value_span = parse_let_value_span(binding.detail.as_deref()?)?;
    let source = SourceText::new(request.file_id, request.uri.as_str(), request.text);
    let value_text = trimmed_value_span(&source, value_span)
        .and_then(|span| request.text.get(span.start..span.end))?;
    let path = parse_static_expression_path(value_text)?;
    let base = path.segments.first()?;
    let mut target =
        static_target_for_identifier(base, definitions, request, _workspace, depth + 1)?;
    target.path_prefix.extend(path.segments.into_iter().skip(1));
    Some(target)
}

fn parse_let_value_span(detail: &str) -> Option<SourceSpan> {
    let (start, end) = detail.split_once("..")?;
    Some(SourceSpan::new(
        FileId::default(),
        start.parse().ok()?,
        end.parse().ok()?,
    ))
}

fn parse_static_expression_path(text: &str) -> Option<IdentifierPath> {
    let inner = text.trim().strip_prefix("${")?.strip_suffix('}')?.trim();
    let span = SourceSpan::new(FileId::default(), 0, inner.len());
    let source = SourceText::new(FileId::default(), "inline", inner.to_owned());
    identifier_path_at_span(span, 0, &source)
}

fn file_entry_from_text(uri: Url, text: String, file_id: FileId) -> WorkspaceFileEntry {
    let parsed = parser::parse_fallback(file_id, uri.as_str(), &text);
    let file = match parsed.file {
        ParsedFile::Fallback(file) => file,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => unreachable!(),
    };
    let semantic = SemanticDocument::from_ast(&file.ast);
    WorkspaceFileEntry {
        uri,
        text,
        definitions: semantic.definitions,
        ast: file.ast,
        file_id,
    }
}

fn push_location(locations: &mut Vec<Location>, uri: &Url, span: SourceSpan, text: &str) {
    if let Ok(range) = LineIndex::new(text).span_to_range(text, span) {
        locations.push(Location {
            uri: uri.clone(),
            range,
        });
    }
}

fn is_declaration_location(
    location: &Location,
    target: &ReferenceTarget,
    workspace: &WorkspaceCorpus,
) -> bool {
    workspace.files.iter().any(|file| {
        if file.uri != location.uri {
            return false;
        }
        match target {
            ReferenceTarget::Let { uri, name } => {
                &file.uri == uri
                    && file.definitions.iter().any(|definition| {
                        definition.kind == DefinitionKind::Let
                            && &definition.name == name
                            && location_matches_definition(location, file, definition)
                    })
            }
            ReferenceTarget::Alias { uri, name } => {
                &file.uri == uri
                    && file.definitions.iter().any(|definition| {
                        definition.kind == DefinitionKind::Alias
                            && &definition.name == name
                            && location_matches_definition(location, file, definition)
                    })
            }
            ReferenceTarget::KeyPath { uri, path } => {
                &file.uri == uri
                    && file.definitions.iter().any(|definition| {
                        definition.kind == DefinitionKind::Key
                            && &definition.path.to_string() == path
                            && location_matches_definition(location, file, definition)
                    })
            }
            ReferenceTarget::TypeLike { name } => file.definitions.iter().any(|definition| {
                definition.kind == DefinitionKind::Directive
                    && matches!(definition.name.as_str(), "@schema" | "@profile")
                    && definition
                        .detail
                        .as_deref()
                        .is_some_and(|value| normalize_name(value) == name)
                    && location_matches_definition(location, file, definition)
            }),
            ReferenceTarget::ImportTarget { .. } => false,
        }
    })
}

fn location_matches_definition(
    location: &Location,
    file: &WorkspaceFileEntry,
    definition: &Definition,
) -> bool {
    let span = match definition.kind {
        DefinitionKind::Let => SourceSpan::new(
            definition.span.file_id,
            definition.span.start + 4,
            definition.span.start + 4 + definition.name.len(),
        ),
        DefinitionKind::Alias => {
            let source = SourceText::new(file.file_id, file.uri.as_str(), file.text.clone());
            match file
                .ast
                .documents
                .iter()
                .flat_map(|document| document.items.iter())
                .find_map(|item| match item {
                    DocumentItem::Directive(directive) if directive.span == definition.span => {
                        directive_alias_span(directive, &source)
                    }
                    _ => None,
                }) {
                Some(span) => span,
                None => definition.selection_span,
            }
        }
        DefinitionKind::Directive => {
            let source = SourceText::new(file.file_id, file.uri.as_str(), file.text.clone());
            match file
                .ast
                .documents
                .iter()
                .flat_map(|document| document.items.iter())
                .find_map(|item| match item {
                    DocumentItem::Directive(directive) if directive.span == definition.span => {
                        directive_value_span(directive, &source)
                    }
                    _ => None,
                }) {
                Some(span) => span,
                None => definition.selection_span,
            }
        }
        _ => definition.selection_span,
    };

    LineIndex::new(&file.text)
        .span_to_range(&file.text, span)
        .map(|range| range == location.range)
        .unwrap_or(false)
}

fn collect_tag_name_occurrences(
    file: &WorkspaceFileEntry,
    name: &str,
    locations: &mut Vec<Location>,
) {
    let needle = format!("!{name}");
    let bytes = file.text.as_bytes();
    let mut start = 0usize;
    while let Some(found) = file.text[start..].find(&needle) {
        let absolute = start + found;
        let before_ok = absolute == 0 || !is_ident(bytes[absolute.saturating_sub(1)]);
        let after_index = absolute + needle.len();
        let after_ok = after_index >= bytes.len() || !is_ident(bytes[after_index]);
        if before_ok && after_ok {
            push_location(
                locations,
                &file.uri,
                SourceSpan::new(file.file_id, absolute + 1, absolute + 1 + name.len()),
                &file.text,
            );
        }
        start = absolute + needle.len();
    }
}

fn unique_locations(mut locations: Vec<Location>) -> Vec<Location> {
    locations.sort_by(|left, right| {
        left.uri
            .cmp(&right.uri)
            .then_with(|| left.range.start.line.cmp(&right.range.start.line))
            .then_with(|| left.range.start.character.cmp(&right.range.start.character))
            .then_with(|| left.range.end.line.cmp(&right.range.end.line))
            .then_with(|| left.range.end.character.cmp(&right.range.end.character))
    });
    locations.dedup_by(|left, right| left.uri == right.uri && left.range == right.range);
    locations
}

fn walk_workspace(root: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_workspace(&path, visit);
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "luma") {
            visit(&path);
        }
    }
}

fn directive_target_span(directive: &Directive, source: &SourceText) -> Option<SourceSpan> {
    let line = source.slice(directive.span)?;
    let target = directive
        .value
        .as_ref()?
        .split_once(" as ")
        .map(|(t, _)| t)
        .unwrap_or(directive.value.as_ref()?);
    let local_start = line.find(target)?;
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + local_start,
        directive.span.start + local_start + target.len(),
    ))
}

fn directive_value_span(directive: &Directive, source: &SourceText) -> Option<SourceSpan> {
    let line = source.slice(directive.span)?;
    let value = directive.value.as_ref()?.trim();
    let local_start = line.find(value)?;
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + local_start,
        directive.span.start + local_start + value.len(),
    ))
}

fn directive_alias_span(directive: &Directive, source: &SourceText) -> Option<SourceSpan> {
    if !matches!(directive.name.as_str(), "@import" | "@use") {
        return None;
    }
    let line = source.slice(directive.span)?;
    let alias = directive.value.as_ref()?.rsplit_once(" as ")?.1.trim();
    let local_start = line.rfind(alias)?;
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + local_start,
        directive.span.start + local_start + alias.len(),
    ))
}

fn directive_target(directive: &Directive) -> Option<String> {
    let value = directive.value.as_ref()?.trim();
    Some(
        strip_quotes(
            value
                .split_once(" as ")
                .map(|(target, _)| target)
                .unwrap_or(value),
        )
        .to_owned(),
    )
}

fn let_name_span(binding: &LetBinding) -> Option<SourceSpan> {
    let start = binding.span.start + 4;
    let end = start + binding.name.len();
    (binding.span.end >= end).then_some(SourceSpan::new(binding.span.file_id, start, end))
}

fn trimmed_value_span(source: &SourceText, span: SourceSpan) -> Option<SourceSpan> {
    let text = source.slice(span)?;
    let trimmed_start = text.len().saturating_sub(text.trim_start().len());
    let trimmed_end = text.trim_end().len();
    Some(SourceSpan::new(
        span.file_id,
        span.start + trimmed_start,
        span.start + trimmed_end,
    ))
}

fn tag_name_span(tag: &TagNode, source: &SourceText) -> Option<SourceSpan> {
    let text = source.slice(tag.span)?;
    let bang = text.find('!')?;
    let name_start = bang + 1;
    let name_len = text[name_start..]
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        .count();
    (name_len > 0).then_some(SourceSpan::new(
        tag.span.file_id,
        tag.span.start + name_start,
        tag.span.start + name_start + name_len,
    ))
}

fn entry_metadata_span(entry: &MappingEntry, source: &SourceText) -> Option<SourceSpan> {
    let metadata = entry.metadata.as_ref()?;
    let line = source.slice(entry.span)?;
    let needle = format!("@{metadata}");
    let local_start = line.find(&needle)? + 1;
    Some(SourceSpan::new(
        entry.span.file_id,
        entry.span.start + local_start,
        entry.span.start + local_start + metadata.len(),
    ))
}

fn identifier_path_at_span(
    span: SourceSpan,
    offset: usize,
    source: &SourceText,
) -> Option<IdentifierPath> {
    let text = source.slice(span)?;
    let relative = offset.checked_sub(span.start)?;
    let bytes = text.as_bytes();
    if relative >= bytes.len() || !is_ident(bytes[relative]) {
        return None;
    }
    let mut start = relative;
    while start > 0 && is_path(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = relative;
    while end < bytes.len() && is_path(bytes[end]) {
        end += 1;
    }
    let raw = text.get(start..end)?;
    let segments = raw
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    let selected_segment = raw[..relative - start]
        .bytes()
        .filter(|byte| *byte == b'.')
        .count();
    Some(IdentifierPath {
        segments,
        selected_segment,
    })
}

fn segment_span_in_token(
    span: SourceSpan,
    token: &str,
    token_start: usize,
    segment_index: usize,
) -> Option<SourceSpan> {
    let mut cursor = 0usize;
    for (index, segment) in token
        .split('.')
        .filter(|segment| !segment.is_empty())
        .enumerate()
    {
        let start = token[cursor..].find(segment)? + cursor;
        if index == segment_index {
            return Some(SourceSpan::new(
                span.file_id,
                span.start + token_start + start,
                span.start + token_start + start + segment.len(),
            ));
        }
        cursor = start + segment.len() + 1;
    }
    None
}

fn dotted_key_path(path: &[String]) -> String {
    let mut out = String::from("document[1]");
    for segment in path {
        out.push('.');
        out.push_str(segment);
    }
    out
}

fn contains(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

fn normalize_name(value: &str) -> &str {
    strip_quotes(value.trim())
}

fn strip_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn is_path(byte: u8) -> bool {
    is_ident(byte) || byte == b'.'
}
