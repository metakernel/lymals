use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use thiserror::Error;
use tower_lsp::lsp_types::{Range, TextEdit, Url, WorkspaceEdit, WorkspaceFolder};

use crate::{
    ast::{
        AstFile, Directive, DocumentItem, LetBinding, MappingEntry, Node, Scalar, ScalarKind,
        Sequence,
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
pub struct RenameRequest<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub file_id: FileId,
    pub offset: usize,
    pub workspace_folders: &'a [WorkspaceFolder],
    pub config: &'a LumalsConfig,
    pub open_documents: &'a [DocumentSnapshot],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRename {
    pub range: Range,
    pub placeholder: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RenameError {
    #[error("rename is not supported at this position")]
    UnsupportedRange,
    #[error("rename target must use static identifier characters only")]
    InvalidName,
    #[error("rename would conflict with an existing symbol")]
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RenameTarget {
    Let {
        uri: Url,
        name: String,
    },
    Alias {
        uri: Url,
        name: String,
    },
    KeyPath {
        uri: Url,
        path: String,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedTarget {
    target: RenameTarget,
    span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentifierOccurrence {
    segments: Vec<String>,
    selected_segment: usize,
    segment_span: SourceSpan,
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

pub fn prepare_rename(request: RenameRequest<'_>) -> Option<PreparedRename> {
    let prepared = prepared_target(request)?;
    let range = LineIndex::new(request.text)
        .span_to_range(request.text, prepared.span)
        .ok()?;
    let placeholder = match prepared.target {
        RenameTarget::Let { name, .. }
        | RenameTarget::Alias { name, .. }
        | RenameTarget::KeyPath { name, .. } => name,
    };
    Some(PreparedRename { range, placeholder })
}

pub fn rename(request: RenameRequest<'_>, new_name: &str) -> Result<WorkspaceEdit, RenameError> {
    if !is_valid_new_name(new_name) {
        return Err(RenameError::InvalidName);
    }

    let prepared = prepared_target(request).ok_or(RenameError::UnsupportedRange)?;
    let workspace = WorkspaceCorpus::load(request);
    detect_conflict(&prepared.target, new_name, &workspace)?;

    let mut changes = BTreeMap::<Url, Vec<TextEdit>>::new();
    for file in &workspace.files {
        for span in collect_file_occurrences(file, &prepared.target, request) {
            let range = LineIndex::new(&file.text)
                .span_to_range(&file.text, span)
                .map_err(|_| RenameError::UnsupportedRange)?;
            changes.entry(file.uri.clone()).or_default().push(TextEdit {
                range,
                new_text: new_name.to_owned(),
            });
        }
    }

    for edits in changes.values_mut() {
        edits.sort_by(|left, right| {
            left.range
                .start
                .line
                .cmp(&right.range.start.line)
                .then_with(|| left.range.start.character.cmp(&right.range.start.character))
                .then_with(|| left.range.end.line.cmp(&right.range.end.line))
                .then_with(|| left.range.end.character.cmp(&right.range.end.character))
        });
        edits.dedup_by(|left, right| left.range == right.range);
    }

    Ok(WorkspaceEdit {
        changes: Some(changes.into_iter().collect()),
        document_changes: None,
        change_annotations: None,
    })
}

impl WorkspaceCorpus {
    fn load(request: RenameRequest<'_>) -> Self {
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

    fn file(&self, uri: &Url) -> Option<&WorkspaceFileEntry> {
        self.files.iter().find(|file| &file.uri == uri)
    }
}

fn prepared_target(request: RenameRequest<'_>) -> Option<PreparedTarget> {
    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let source = parsed.source.clone();
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return None,
    };
    let semantic = SemanticDocument::from_ast(file);
    let workspace = WorkspaceCorpus::load(request);

    for definition in &semantic.definitions {
        if definition.kind == DefinitionKind::Key
            && contains(definition.selection_span, request.offset)
        {
            return Some(PreparedTarget {
                target: RenameTarget::KeyPath {
                    uri: request.uri.clone(),
                    path: definition.path.to_string(),
                    name: definition.name.clone(),
                },
                span: definition.selection_span,
            });
        }
    }

    for document in &file.documents {
        for item in &document.items {
            let result = match item {
                DocumentItem::Directive(directive) => {
                    directive_target_for_offset(directive, request.offset, &source, request)
                }
                DocumentItem::Let(binding) => let_target_for_offset(
                    binding,
                    request.offset,
                    &source,
                    &semantic.definitions,
                    request,
                    &workspace,
                ),
                DocumentItem::Node(node) => node_target_for_offset(
                    node,
                    request.offset,
                    &source,
                    &semantic.definitions,
                    request,
                    &workspace,
                ),
                DocumentItem::Comment(_) => None,
            };
            if result.is_some() {
                return result;
            }
        }
    }

    None
}

fn detect_conflict(
    target: &RenameTarget,
    new_name: &str,
    workspace: &WorkspaceCorpus,
) -> Result<(), RenameError> {
    match target {
        RenameTarget::Let { uri, name } => {
            let file = workspace.file(uri).ok_or(RenameError::UnsupportedRange)?;
            if file.definitions.iter().any(|definition| {
                matches!(definition.kind, DefinitionKind::Let | DefinitionKind::Alias)
                    && definition.name == new_name
                    && definition.name != *name
            }) {
                return Err(RenameError::Conflict);
            }
        }
        RenameTarget::Alias { uri, name } => {
            let file = workspace.file(uri).ok_or(RenameError::UnsupportedRange)?;
            if file.definitions.iter().any(|definition| {
                matches!(definition.kind, DefinitionKind::Let | DefinitionKind::Alias)
                    && definition.name == new_name
                    && definition.name != *name
            }) {
                return Err(RenameError::Conflict);
            }
        }
        RenameTarget::KeyPath { uri, path, name } => {
            let file = workspace.file(uri).ok_or(RenameError::UnsupportedRange)?;
            let parent = parent_path(path);
            if file.definitions.iter().any(|definition| {
                definition.kind == DefinitionKind::Key
                    && definition.name == new_name
                    && definition.name != *name
                    && parent_path(&definition.path.to_string()) == parent
            }) {
                return Err(RenameError::Conflict);
            }
        }
    }
    Ok(())
}

fn collect_file_occurrences(
    file: &WorkspaceFileEntry,
    target: &RenameTarget,
    request: RenameRequest<'_>,
) -> Vec<SourceSpan> {
    let source = SourceText::new(file.file_id, file.uri.as_str(), file.text.clone());
    let mut spans = Vec::new();
    for document in &file.ast.documents {
        for item in &document.items {
            collect_item_occurrences(item, file, &source, target, request, &mut spans);
        }
    }
    unique_spans(spans)
}

fn collect_item_occurrences(
    item: &DocumentItem,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &RenameTarget,
    request: RenameRequest<'_>,
    spans: &mut Vec<SourceSpan>,
) {
    match item {
        DocumentItem::Directive(directive) => {
            collect_directive_occurrences(directive, file, source, target, request, spans)
        }
        DocumentItem::Let(binding) => {
            collect_let_occurrences(binding, file, source, target, request, spans)
        }
        DocumentItem::Node(node) => {
            collect_node_occurrences(node, file, source, target, request, spans)
        }
        DocumentItem::Comment(_) => {}
    }
}

fn collect_directive_occurrences(
    directive: &Directive,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &RenameTarget,
    _request: RenameRequest<'_>,
    spans: &mut Vec<SourceSpan>,
) {
    if let RenameTarget::Alias { uri, name } = target
        && &file.uri == uri
        && let Some(span) = directive_alias_span(directive, source)
        && source.slice(span) == Some(name)
    {
        spans.push(span);
    }
}

fn collect_let_occurrences(
    binding: &LetBinding,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &RenameTarget,
    request: RenameRequest<'_>,
    spans: &mut Vec<SourceSpan>,
) {
    if let RenameTarget::Let { uri, name } = target
        && &file.uri == uri
        && &binding.name == name
        && let Some(span) = let_name_span(binding)
    {
        spans.push(span);
    }

    if let Some(span) = binding.value_span {
        collect_expression_occurrences(span, file, source, target, request, spans);
    }
}

fn collect_node_occurrences(
    node: &Node,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &RenameTarget,
    request: RenameRequest<'_>,
    spans: &mut Vec<SourceSpan>,
) {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                collect_entry_occurrences(entry, file, source, target, request, spans);
            }
        }
        Node::Sequence(Sequence { items, .. }) => {
            for item in items {
                if let Some(value) = &item.value {
                    collect_node_occurrences(value, file, source, target, request, spans);
                }
            }
        }
        Node::Scalar(Scalar {
            kind: ScalarKind::LuaExpression,
            span,
            ..
        }) => collect_expression_occurrences(*span, file, source, target, request, spans),
        Node::Tag(tag) => {
            if let Some(value) = &tag.value {
                collect_node_occurrences(value, file, source, target, request, spans);
            }
        }
        _ => {}
    }
}

fn collect_entry_occurrences(
    entry: &MappingEntry,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &RenameTarget,
    request: RenameRequest<'_>,
    spans: &mut Vec<SourceSpan>,
) {
    if let RenameTarget::KeyPath { uri, path, .. } = target
        && &file.uri == uri
        && file.definitions.iter().any(|definition| {
            definition.kind == DefinitionKind::Key
                && definition.selection_span == entry.key_span
                && definition.path.to_string() == *path
        })
    {
        spans.push(entry.key_span);
    }

    if let Some(value) = &entry.value {
        collect_node_occurrences(value, file, source, target, request, spans);
    }
}

fn collect_expression_occurrences(
    span: SourceSpan,
    file: &WorkspaceFileEntry,
    source: &SourceText,
    target: &RenameTarget,
    request: RenameRequest<'_>,
    spans: &mut Vec<SourceSpan>,
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
            let Some(segment_span) = segment_span_in_token(span, token, start, segment_index)
            else {
                continue;
            };
            match target {
                RenameTarget::Let { uri, name }
                    if &file.uri == uri && segments[0] == name && segment_index == 0 =>
                {
                    spans.push(segment_span)
                }
                RenameTarget::Alias { uri, name }
                    if &file.uri == uri && segments[0] == name && segment_index == 0 =>
                {
                    spans.push(segment_span)
                }
                RenameTarget::KeyPath { uri, path, .. } if segment_index > 0 => {
                    if let Some((resolved_uri, resolved_path)) = resolve_expression_key_path(
                        file,
                        segments.as_slice(),
                        segment_index,
                        request,
                    ) && &resolved_uri == uri
                        && resolved_path == *path
                    {
                        spans.push(segment_span);
                    }
                }
                _ => {}
            }
        }
    }
}

fn directive_target_for_offset(
    directive: &Directive,
    offset: usize,
    source: &SourceText,
    request: RenameRequest<'_>,
) -> Option<PreparedTarget> {
    if let Some(span) = directive_alias_span(directive, source)
        && contains(span, offset)
    {
        let name = source.slice(span)?.to_owned();
        return Some(PreparedTarget {
            target: RenameTarget::Alias {
                uri: request.uri.clone(),
                name,
            },
            span,
        });
    }
    None
}

fn let_target_for_offset(
    binding: &LetBinding,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: RenameRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Option<PreparedTarget> {
    if let Some(name_span) = let_name_span(binding)
        && contains(name_span, offset)
    {
        return Some(PreparedTarget {
            target: RenameTarget::Let {
                uri: request.uri.clone(),
                name: binding.name.clone(),
            },
            span: name_span,
        });
    }

    let value_span = binding.value_span?;
    if !contains(value_span, offset) {
        return None;
    }
    let trimmed = trimmed_value_span(source, value_span)?;
    let occurrence = identifier_occurrence_at_span(trimmed, offset, source)?;
    rename_site_for_identifier(occurrence, definitions, request, workspace)
}

fn node_target_for_offset(
    node: &Node,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: RenameRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Option<PreparedTarget> {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                if contains(entry.key_span, offset)
                    && let Some(definition) = definitions.iter().find(|definition| {
                        definition.kind == DefinitionKind::Key
                            && definition.selection_span == entry.key_span
                    })
                {
                    return Some(PreparedTarget {
                        target: RenameTarget::KeyPath {
                            uri: request.uri.clone(),
                            path: definition.path.to_string(),
                            name: definition.name.clone(),
                        },
                        span: entry.key_span,
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
            let occurrence = identifier_occurrence_at_span(*span, offset, source)?;
            rename_site_for_identifier(occurrence, definitions, request, workspace)
        }
        Node::Tag(tag) => tag.value.as_deref().and_then(|value| {
            node_target_for_offset(value, offset, source, definitions, request, workspace)
        }),
        _ => None,
    }
}

fn rename_site_for_identifier(
    occurrence: IdentifierOccurrence,
    definitions: &[Definition],
    request: RenameRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Option<PreparedTarget> {
    let base = occurrence.segments.first()?;
    if occurrence.selected_segment == 0 {
        if definitions
            .iter()
            .any(|definition| definition.kind == DefinitionKind::Alias && definition.name == *base)
        {
            return Some(PreparedTarget {
                target: RenameTarget::Alias {
                    uri: request.uri.clone(),
                    name: base.clone(),
                },
                span: occurrence.segment_span,
            });
        }
        if definitions
            .iter()
            .any(|definition| definition.kind == DefinitionKind::Let && definition.name == *base)
        {
            return Some(PreparedTarget {
                target: RenameTarget::Let {
                    uri: request.uri.clone(),
                    name: base.clone(),
                },
                span: occurrence.segment_span,
            });
        }
        if occurrence.segments.len() == 1
            && let Some(definition) = definitions.iter().find(|definition| {
                definition.kind == DefinitionKind::Key && definition.name == *base
            })
        {
            return Some(PreparedTarget {
                target: RenameTarget::KeyPath {
                    uri: request.uri.clone(),
                    path: definition.path.to_string(),
                    name: definition.name.clone(),
                },
                span: occurrence.segment_span,
            });
        }
    }

    let target = static_target_for_identifier(base, definitions, request, workspace, 0)?;
    let mut resolved = target.path_prefix;
    resolved.extend(
        occurrence
            .segments
            .iter()
            .skip(1)
            .take(occurrence.selected_segment)
            .cloned(),
    );
    let name = occurrence
        .segments
        .get(occurrence.selected_segment)?
        .clone();
    Some(PreparedTarget {
        target: RenameTarget::KeyPath {
            uri: target.uri,
            path: dotted_key_path(&resolved),
            name,
        },
        span: occurrence.segment_span,
    })
}

fn resolve_expression_key_path(
    file: &WorkspaceFileEntry,
    segments: &[&str],
    segment_index: usize,
    request: RenameRequest<'_>,
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
    request: RenameRequest<'_>,
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
    let occurrence = parse_static_expression_occurrence(value_text)?;
    let base = occurrence.segments.first()?;
    let mut target = static_target_for_identifier_in_file(base, file, request, depth + 1)?;
    target
        .path_prefix
        .extend(occurrence.segments.into_iter().skip(1));
    Some(target)
}

fn static_target_for_identifier(
    ident: &str,
    definitions: &[Definition],
    request: RenameRequest<'_>,
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
        let uri = resolve_guarded_import(
            request.uri,
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
    let binding = definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Let && definition.name == ident)?;
    let value_span = parse_let_value_span(binding.detail.as_deref()?)?;
    let source = SourceText::new(request.file_id, request.uri.as_str(), request.text);
    let value_text = trimmed_value_span(&source, value_span)
        .and_then(|span| request.text.get(span.start..span.end))?;
    let occurrence = parse_static_expression_occurrence(value_text)?;
    let base = occurrence.segments.first()?;
    let mut target =
        static_target_for_identifier(base, definitions, request, _workspace, depth + 1)?;
    target
        .path_prefix
        .extend(occurrence.segments.into_iter().skip(1));
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

fn parse_static_expression_occurrence(text: &str) -> Option<IdentifierOccurrence> {
    let inner = text.trim().strip_prefix("${")?.strip_suffix('}')?.trim();
    let span = SourceSpan::new(FileId::default(), 0, inner.len());
    let source = SourceText::new(FileId::default(), "inline", inner.to_owned());
    identifier_occurrence_at_span(span, 0, &source)
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

fn identifier_occurrence_at_span(
    span: SourceSpan,
    offset: usize,
    source: &SourceText,
) -> Option<IdentifierOccurrence> {
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
    let segment_span = segment_span_in_token(span, raw, start, selected_segment)?;
    Some(IdentifierOccurrence {
        segments,
        selected_segment,
        segment_span,
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

fn parent_path(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(parent, _)| parent)
        .unwrap_or(path)
        .to_owned()
}

fn unique_spans(mut spans: Vec<SourceSpan>) -> Vec<SourceSpan> {
    spans.sort_by(|left, right| {
        left.file_id
            .cmp(&right.file_id)
            .then_with(|| left.start.cmp(&right.start))
            .then_with(|| left.end.cmp(&right.end))
    });
    spans.dedup();
    spans
}

fn contains(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

fn is_valid_new_name(name: &str) -> bool {
    !name.is_empty() && !name.as_bytes().contains(&b'.') && name.bytes().all(is_ident)
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn is_path(byte: u8) -> bool {
    is_ident(byte) || byte == b'.'
}
