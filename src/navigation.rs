use std::{fs, path::Path};

use tower_lsp::lsp_types::{Location, Position, Url, WorkspaceFolder};

use crate::{
    ast::{
        AstFile, Directive, DocumentItem, LetBinding, MappingEntry, Node, Scalar, ScalarKind,
        Sequence, TagNode,
    },
    config::LumalsConfig,
    imports::resolve_guarded_import,
    parser,
    position::LineIndex,
    semantic::SemanticDocument,
    symbols::{Definition, DefinitionKind},
    syntax::{FileId, ParsedFile, SourceSpan, SourceText},
    workspace::{effective_roots, file_url_to_path, is_workspace_luma_uri},
};

#[derive(Debug, Clone, Copy)]
pub struct NavigationRequest<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub file_id: FileId,
    pub offset: usize,
    pub workspace_folders: &'a [WorkspaceFolder],
    pub config: &'a LumalsConfig,
}

pub fn goto_definition(request: NavigationRequest<'_>) -> Vec<Location> {
    goto(request)
}

pub fn goto_declaration(request: NavigationRequest<'_>) -> Vec<Location> {
    goto(request)
}

pub fn goto_type_definition(request: NavigationRequest<'_>) -> Vec<Location> {
    goto_type(request)
}

pub fn goto_implementation(request: NavigationRequest<'_>) -> Vec<Location> {
    goto_implementation_inner(request)
}

fn goto(request: NavigationRequest<'_>) -> Vec<Location> {
    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let source = parsed.source.clone();
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return Vec::new(),
    };
    let semantic = SemanticDocument::from_ast(file);
    let workspace = WorkspaceCorpus::load(request);

    find_in_file(
        file,
        request.offset,
        &source,
        &semantic.definitions,
        request,
        &workspace,
    )
}

fn goto_type(request: NavigationRequest<'_>) -> Vec<Location> {
    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let source = parsed.source.clone();
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return Vec::new(),
    };
    let workspace = WorkspaceCorpus::load(request);

    find_type_in_file(file, request.offset, &source, request, &workspace)
}

fn goto_implementation_inner(request: NavigationRequest<'_>) -> Vec<Location> {
    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let source = parsed.source.clone();
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return Vec::new(),
    };
    let semantic = SemanticDocument::from_ast(file);
    let workspace = WorkspaceCorpus::load(request);

    find_implementation_in_file(
        file,
        request.offset,
        &source,
        &semantic.definitions,
        request,
        &workspace,
    )
}

fn find_in_file(
    file: &AstFile,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    for document in &file.documents {
        for item in &document.items {
            let result = match item {
                DocumentItem::Directive(directive) => {
                    directive_navigation(directive, offset, source, definitions, request, workspace)
                }
                DocumentItem::Let(binding) => {
                    let_navigation(binding, offset, source, definitions, request, workspace)
                }
                DocumentItem::Node(node) => {
                    node_navigation(node, offset, source, definitions, request, workspace)
                }
                DocumentItem::Comment(_) => Vec::new(),
            };

            if !result.is_empty() {
                return result;
            }
        }
    }

    Vec::new()
}

fn find_type_in_file(
    file: &AstFile,
    offset: usize,
    source: &SourceText,
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    for document in &file.documents {
        for item in &document.items {
            let result = match item {
                DocumentItem::Directive(directive) => {
                    directive_type_navigation(directive, offset, source, workspace)
                }
                DocumentItem::Let(_) | DocumentItem::Comment(_) => Vec::new(),
                DocumentItem::Node(node) => {
                    node_type_navigation(node, offset, source, request, workspace)
                }
            };

            if !result.is_empty() {
                return unique_locations(result);
            }
        }
    }

    Vec::new()
}

fn find_implementation_in_file(
    file: &AstFile,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    for document in &file.documents {
        for item in &document.items {
            let result = match item {
                DocumentItem::Directive(directive) => directive_implementation_navigation(
                    directive, offset, source, request, workspace,
                ),
                DocumentItem::Let(binding) => let_implementation_navigation(
                    binding,
                    offset,
                    source,
                    definitions,
                    request,
                    workspace,
                ),
                DocumentItem::Node(node) => node_implementation_navigation(
                    node,
                    offset,
                    source,
                    definitions,
                    request,
                    workspace,
                ),
                DocumentItem::Comment(_) => Vec::new(),
            };

            if !result.is_empty() {
                return result;
            }
        }
    }

    Vec::new()
}

fn directive_navigation(
    directive: &Directive,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    if let Some(path_span) = directive_target_span(directive, source)
        && contains(path_span, offset)
        && matches!(directive.name.as_str(), "@import" | "@include" | "@use")
    {
        if let Some(target) = directive_target(directive)
            && let Ok(uri) = resolve_guarded_import(
                request.uri,
                &target,
                request.workspace_folders,
                request.config,
            )
            && let Some(location) = workspace.file_entry(&uri).map(file_entry_root_location)
        {
            return vec![location];
        }
    }

    if let Some(alias_span) = directive_alias_span(directive, source)
        && contains(alias_span, offset)
        && let Some(alias) = source.slice(alias_span)
    {
        return definition_locations(
            request.uri,
            definitions,
            DefinitionKind::Alias,
            alias,
            Some(request.text),
        );
    }

    Vec::new()
}

fn directive_type_navigation(
    directive: &Directive,
    offset: usize,
    source: &SourceText,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    if matches!(directive.name.as_str(), "@schema" | "@profile")
        && let Some(value_span) = directive_value_span(directive, source)
        && contains(value_span, offset)
        && let Some(value) = directive.value.as_deref()
    {
        return workspace.directive_value_locations(&directive.name, normalize_name(value));
    }

    Vec::new()
}

fn directive_implementation_navigation(
    directive: &Directive,
    offset: usize,
    source: &SourceText,
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    if matches!(directive.name.as_str(), "@import" | "@include" | "@use")
        && ((directive_target_span(directive, source).is_some_and(|span| contains(span, offset)))
            || (directive_alias_span(directive, source).is_some_and(|span| contains(span, offset))))
        && let Some(target) = directive_target(directive)
        && let Ok(uri) = resolve_guarded_import(
            request.uri,
            &target,
            request.workspace_folders,
            request.config,
        )
        && let Some(location) = workspace.file_entry(&uri).map(file_entry_root_location)
    {
        return vec![location];
    }

    Vec::new()
}

fn let_navigation(
    binding: &LetBinding,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    if let Some(name_span) = let_name_span(binding)
        && contains(name_span, offset)
    {
        return definition_locations(
            request.uri,
            definitions,
            DefinitionKind::Let,
            &binding.name,
            Some(request.text),
        );
    }

    if let Some(value_span) = binding.value_span
        && contains(value_span, offset)
        && let Some(trimmed_span) = trimmed_value_span(source, value_span)
        && let Some((_, path)) = identifier_path_at_span(trimmed_span, offset, source)
    {
        return resolve_reference_path(path, definitions, request, workspace);
    }

    Vec::new()
}

fn let_implementation_navigation(
    binding: &LetBinding,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    if let Some(value_span) = binding.value_span
        && contains(value_span, offset)
        && let Some(trimmed_span) = trimmed_value_span(source, value_span)
        && let Some((_, path)) = identifier_path_at_span(trimmed_span, offset, source)
    {
        return resolve_implementation_path(path, definitions, request, workspace);
    }

    Vec::new()
}

fn node_navigation(
    node: &Node,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                if let Some(metadata_span) = entry_metadata_span(entry, source)
                    && contains(metadata_span, offset)
                    && let Some(name) = source.slice(metadata_span)
                {
                    let locations = definition_locations(
                        request.uri,
                        definitions,
                        DefinitionKind::Key,
                        name,
                        Some(request.text),
                    );
                    if !locations.is_empty() {
                        return locations;
                    }
                    return workspace.key_locations(name);
                }

                if let Some(value) = &entry.value {
                    let locations =
                        node_navigation(value, offset, source, definitions, request, workspace);
                    if !locations.is_empty() {
                        return locations;
                    }
                }
            }
            Vec::new()
        }
        Node::Sequence(Sequence { items, .. }) => items
            .iter()
            .filter_map(|item| item.value.as_deref())
            .find_map(|value| {
                let locations =
                    node_navigation(value, offset, source, definitions, request, workspace);
                (!locations.is_empty()).then_some(locations)
            })
            .unwrap_or_default(),
        Node::Scalar(Scalar {
            kind: ScalarKind::LuaExpression,
            span,
            ..
        }) => {
            if let Some((_, path)) = identifier_path_at_span(*span, offset, source) {
                resolve_reference_path(path, definitions, request, workspace)
            } else {
                Vec::new()
            }
        }
        Node::Tag(TagNode { value, .. }) => value
            .as_deref()
            .map(|value| node_navigation(value, offset, source, definitions, request, workspace))
            .unwrap_or_default(),
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => Vec::new(),
    }
}

fn node_type_navigation(
    node: &Node,
    offset: usize,
    source: &SourceText,
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                if let Some(metadata_span) = entry_metadata_span(entry, source)
                    && contains(metadata_span, offset)
                    && let Some(name) = source.slice(metadata_span)
                {
                    return type_like_locations(name, request, workspace);
                }

                if let Some(value) = &entry.value {
                    let locations = node_type_navigation(value, offset, source, request, workspace);
                    if !locations.is_empty() {
                        return locations;
                    }
                }
            }
            Vec::new()
        }
        Node::Sequence(Sequence { items, .. }) => items
            .iter()
            .filter_map(|item| item.value.as_deref())
            .find_map(|value| {
                let locations = node_type_navigation(value, offset, source, request, workspace);
                (!locations.is_empty()).then_some(locations)
            })
            .unwrap_or_default(),
        Node::Tag(tag) => {
            if let Some(name_span) = tag_name_span(tag, source)
                && contains(name_span, offset)
            {
                return type_like_locations(&tag.name, request, workspace);
            }

            tag.value
                .as_deref()
                .map(|value| node_type_navigation(value, offset, source, request, workspace))
                .unwrap_or_default()
        }
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => Vec::new(),
    }
}

fn node_implementation_navigation(
    node: &Node,
    offset: usize,
    source: &SourceText,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                if let Some(value) = &entry.value {
                    let locations = node_implementation_navigation(
                        value,
                        offset,
                        source,
                        definitions,
                        request,
                        workspace,
                    );
                    if !locations.is_empty() {
                        return locations;
                    }
                }
            }
            Vec::new()
        }
        Node::Sequence(Sequence { items, .. }) => items
            .iter()
            .filter_map(|item| item.value.as_deref())
            .find_map(|value| {
                let locations = node_implementation_navigation(
                    value,
                    offset,
                    source,
                    definitions,
                    request,
                    workspace,
                );
                (!locations.is_empty()).then_some(locations)
            })
            .unwrap_or_default(),
        Node::Scalar(Scalar {
            kind: ScalarKind::LuaExpression,
            span,
            ..
        }) => {
            if let Some((_, path)) = identifier_path_at_span(*span, offset, source) {
                resolve_implementation_path(path, definitions, request, workspace)
            } else {
                Vec::new()
            }
        }
        Node::Tag(TagNode { value, .. }) => value
            .as_deref()
            .map(|value| {
                node_implementation_navigation(
                    value,
                    offset,
                    source,
                    definitions,
                    request,
                    workspace,
                )
            })
            .unwrap_or_default(),
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => Vec::new(),
    }
}

fn resolve_reference_path(
    path: IdentifierPath,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    if path.segments.is_empty() {
        return Vec::new();
    }

    if path.segments.len() == 1 {
        let ident = &path.segments[0];
        let local_alias = definition_locations(
            request.uri,
            definitions,
            DefinitionKind::Alias,
            ident,
            Some(request.text),
        );
        if !local_alias.is_empty() {
            return local_alias;
        }
        let local_let = definition_locations(
            request.uri,
            definitions,
            DefinitionKind::Let,
            ident,
            Some(request.text),
        );
        if !local_let.is_empty() {
            return local_let;
        }
        let local_key = definition_locations(
            request.uri,
            definitions,
            DefinitionKind::Key,
            ident,
            Some(request.text),
        );
        if !local_key.is_empty() {
            return local_key;
        }
        return workspace.named_locations(ident);
    }

    if let Some(target) =
        static_target_for_identifier(&path.segments[0], definitions, request, workspace, 0)
    {
        return workspace.resolve_path_target(
            &target.uri,
            &path.segments[1..],
            Some(target.path_prefix),
        );
    }

    Vec::new()
}

fn resolve_implementation_path(
    path: IdentifierPath,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    if path.segments.is_empty() {
        return Vec::new();
    }

    let base = &path.segments[0];
    let Some(target) = static_target_for_identifier(base, definitions, request, workspace, 0)
    else {
        return Vec::new();
    };

    workspace.resolve_path_target(&target.uri, &path.segments[1..], Some(target.path_prefix))
}

fn static_target_for_identifier(
    ident: &str,
    definitions: &[Definition],
    request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
    depth: u32,
) -> Option<StaticTarget> {
    if depth >= request.config.max_resolve_depth {
        return None;
    }

    if let Some(alias) = definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Alias && definition.name == ident)
    {
        if let Some(target) = alias.detail.as_deref()
            && let Ok(uri) = resolve_guarded_import(
                request.uri,
                target,
                request.workspace_folders,
                request.config,
            )
        {
            return Some(StaticTarget {
                uri,
                path_prefix: Vec::new(),
            });
        }
    }

    let binding = definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Let && definition.name == ident)?;
    let value_span = parse_let_value_span(binding.detail.as_deref()?)?;
    let source = SourceText::new(request.file_id, request.uri.as_str(), request.text);
    let value_text = trimmed_value_span(
        &source,
        SourceSpan::new(request.file_id, value_span.start, value_span.end),
    )
    .and_then(|span| request.text.get(span.start..span.end))?;
    let path = parse_static_expression_path(value_text)?;
    let base = path.segments.first()?;
    let mut target =
        static_target_for_identifier(base, definitions, request, workspace, depth + 1)?;
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
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix("${")?.strip_suffix('}')?.trim();
    let span = SourceSpan::new(FileId::default(), 0, inner.len());
    let source = SourceText::new(FileId::default(), "inline", inner.to_owned());
    identifier_path_at_span(span, 0, &source).map(|(_, path)| path)
}

fn definition_locations(
    uri: &Url,
    definitions: &[Definition],
    kind: DefinitionKind,
    name: &str,
    text: Option<&str>,
) -> Vec<Location> {
    definitions
        .iter()
        .filter(|definition| definition.kind == kind && definition.name == name)
        .filter_map(|definition| location_from_definition(uri.clone(), definition, text))
        .collect()
}

#[derive(Debug, Clone)]
struct StaticTarget {
    uri: Url,
    path_prefix: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentifierPath {
    segments: Vec<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceFileEntry {
    uri: Url,
    text: String,
    definitions: Vec<Definition>,
    root_span: SourceSpan,
}

#[derive(Debug, Clone, Default)]
struct WorkspaceCorpus {
    files: Vec<WorkspaceFileEntry>,
}

impl WorkspaceCorpus {
    fn load(request: NavigationRequest<'_>) -> Self {
        let mut files = Vec::new();
        files.push(file_entry_from_text(
            request.uri.clone(),
            request.text.to_owned(),
            request.file_id,
        ));

        if request.config.index_workspace {
            let current_path = file_url_to_path(request.uri);
            let mut next_file_id = request.file_id.0.saturating_add(1);
            for root in effective_roots(request.workspace_folders, request.config) {
                walk_workspace(root.as_path(), &mut |path| {
                    if current_path.as_deref() == Some(path) {
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
                });
            }
        }

        Self { files }
    }

    fn file_entry(&self, uri: &Url) -> Option<&WorkspaceFileEntry> {
        self.files.iter().find(|file| &file.uri == uri)
    }

    fn named_locations(&self, name: &str) -> Vec<Location> {
        self.files
            .iter()
            .flat_map(|file| {
                file.definitions
                    .iter()
                    .filter(move |definition| definition.name == name)
                    .filter_map(|definition| {
                        location_from_definition(file.uri.clone(), definition, Some(&file.text))
                    })
            })
            .collect()
    }

    fn key_locations(&self, name: &str) -> Vec<Location> {
        self.files
            .iter()
            .flat_map(|file| {
                file.definitions
                    .iter()
                    .filter(move |definition| {
                        definition.kind == DefinitionKind::Key && definition.name == name
                    })
                    .filter_map(|definition| {
                        location_from_definition(file.uri.clone(), definition, Some(&file.text))
                    })
            })
            .collect()
    }

    fn tag_locations(&self, name: &str) -> Vec<Location> {
        self.files
            .iter()
            .flat_map(|file| {
                file.definitions
                    .iter()
                    .filter(move |definition| {
                        definition.kind == DefinitionKind::Tag && definition.name == name
                    })
                    .filter_map(|definition| {
                        location_from_definition(file.uri.clone(), definition, Some(&file.text))
                    })
            })
            .collect()
    }

    fn directive_value_locations(&self, directive_name: &str, value: &str) -> Vec<Location> {
        self.files
            .iter()
            .flat_map(|file| {
                file.definitions
                    .iter()
                    .filter(move |definition| {
                        definition.kind == DefinitionKind::Directive
                            && definition.name == directive_name
                            && definition
                                .detail
                                .as_deref()
                                .is_some_and(|detail| normalize_name(detail) == value)
                    })
                    .filter_map(|definition| {
                        location_from_definition(file.uri.clone(), definition, Some(&file.text))
                    })
            })
            .collect()
    }

    fn resolve_path_target(
        &self,
        uri: &Url,
        suffix: &[String],
        mut prefix: Option<Vec<String>>,
    ) -> Vec<Location> {
        let Some(file) = self.file_entry(uri) else {
            return Vec::new();
        };

        let mut path = prefix.take().unwrap_or_default();
        path.extend_from_slice(suffix);
        if path.is_empty() {
            return vec![file_entry_root_location(file)];
        }

        file.definitions
            .iter()
            .filter(|definition| {
                definition.kind == DefinitionKind::Key
                    && definition.path.to_string() == dotted_key_path(&path)
            })
            .filter_map(|definition| {
                location_from_definition(file.uri.clone(), definition, Some(&file.text))
            })
            .collect()
    }
}

fn dotted_key_path(path: &[String]) -> String {
    let mut out = String::from("document[1]");
    for segment in path {
        out.push('.');
        out.push_str(segment);
    }
    out
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
        root_span: file.document_spans.first().copied().unwrap_or(file.span),
    }
}

fn file_entry_root_location(file: &WorkspaceFileEntry) -> Location {
    location_from_text(file.uri.clone(), file.root_span, Some(&file.text), None)
        .expect("workspace file root range should exist")
}

fn location_from_definition(
    uri: Url,
    definition: &Definition,
    text: Option<&str>,
) -> Option<Location> {
    let span = refined_selection_span(definition, text).unwrap_or(definition.selection_span);
    location_from_text(uri, span, text, None)
}

fn refined_selection_span(definition: &Definition, text: Option<&str>) -> Option<SourceSpan> {
    let text = text?;
    match definition.kind {
        DefinitionKind::Let => {
            let start = definition.span.start + 4;
            Some(SourceSpan::new(
                definition.span.file_id,
                start,
                start + definition.name.len(),
            ))
        }
        DefinitionKind::Alias => {
            let line = text.get(definition.span.start..definition.span.end)?;
            let local_start = line.rfind(&definition.name)?;
            Some(SourceSpan::new(
                definition.span.file_id,
                definition.span.start + local_start,
                definition.span.start + local_start + definition.name.len(),
            ))
        }
        DefinitionKind::Directive => {
            let detail = definition.detail.as_deref()?.trim();
            let line = text.get(definition.span.start..definition.span.end)?;
            let local_start = line.find(detail)?;
            Some(SourceSpan::new(
                definition.span.file_id,
                definition.span.start + local_start,
                definition.span.start + local_start + detail.len(),
            ))
        }
        _ => Some(definition.selection_span),
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

fn location_from_text(
    uri: Url,
    span: SourceSpan,
    text: Option<&str>,
    _position: Option<Position>,
) -> Option<Location> {
    let text = text?;
    let range = LineIndex::new(text).span_to_range(text, span).ok()?;
    Some(Location { uri, range })
}

fn identifier_path_at_span(
    span: SourceSpan,
    offset: usize,
    source: &SourceText,
) -> Option<(SourceSpan, IdentifierPath)> {
    let text = source.slice(span)?;
    let relative = offset.checked_sub(span.start)?;
    let bytes = text.as_bytes();
    if relative >= bytes.len() {
        return None;
    }

    let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-');
    let is_path = |byte: u8| is_ident(byte) || byte == b'.';
    if !is_ident(bytes[relative]) {
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
    (!segments.is_empty()).then_some((
        SourceSpan::new(span.file_id, span.start + start, span.start + end),
        IdentifierPath { segments },
    ))
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
    let target = value
        .split_once(" as ")
        .map(|(target, _)| target)
        .unwrap_or(value);
    Some(strip_quotes(target).to_owned())
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

fn contains(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

fn type_like_locations(
    name: &str,
    _request: NavigationRequest<'_>,
    workspace: &WorkspaceCorpus,
) -> Vec<Location> {
    let schema_locations = workspace.directive_value_locations("@schema", name);
    if !schema_locations.is_empty() {
        return schema_locations;
    }

    let profile_locations = workspace.directive_value_locations("@profile", name);
    if !profile_locations.is_empty() {
        return profile_locations;
    }

    workspace.tag_locations(name)
}

fn unique_locations(mut locations: Vec<Location>) -> Vec<Location> {
    locations.dedup_by(|left, right| left.uri == right.uri && left.range == right.range);
    locations
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
