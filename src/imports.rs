use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use tower_lsp::lsp_types::Url;

use crate::workspace::{effective_roots, file_url_to_path, normalize_path};
use crate::{
    ast::{AstFile, Directive, DocumentItem},
    config::LumalsConfig,
    diagnostics::{Diagnostic, DiagnosticSeverity},
    parser,
    syntax::{FileId, ParsedFile, SourceSpan, SourceText},
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImportPolicyError {
    #[error("import/include path must not contain parent traversal")]
    ParentTraversal,
    #[error("import/include scheme '{0}' is not allowed")]
    DisallowedScheme(String),
    #[error("import/include absolute path is not allowed")]
    AbsolutePathNotAllowed,
    #[error("import/include target must stay within configured roots")]
    OutsideAllowedRoots,
    #[error("import/include base URI must be a file URI")]
    NonFileBaseUri,
    #[error("import/include target is not a valid URI or relative path")]
    InvalidTarget,
    #[error("import/include target file does not exist")]
    MissingTarget,
    #[error("import/include target exceeds the configured size limit")]
    FileTooLarge,
    #[error("import/include cycle detected")]
    Cycle,
    #[error("import/include resolution exceeded the configured depth limit")]
    DepthLimit,
    #[error("import/include resolution exceeded the configured edge limit")]
    EdgeLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEdge {
    pub source: Url,
    pub target: Url,
    pub directive: String,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportGraph {
    pub roots: Vec<Url>,
    pub files: Vec<Url>,
    pub edges: Vec<ImportEdge>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
struct PendingFile {
    uri: Url,
    depth: u32,
    stack: Vec<PathBuf>,
}

pub fn resolve_guarded_import(
    base_uri: &Url,
    target: &str,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
) -> Result<Url, ImportPolicyError> {
    let target = strip_matching_quotes(target.trim()).unwrap_or(target.trim());

    if contains_parent_traversal(target) {
        return Err(ImportPolicyError::ParentTraversal);
    }

    if let Ok(uri) = Url::parse(target) {
        let scheme = uri.scheme().to_string();
        if !config
            .allowed_schemes
            .iter()
            .any(|allowed| allowed == &scheme)
        {
            return Err(ImportPolicyError::DisallowedScheme(scheme));
        }

        if uri.scheme() != "file" {
            return Err(ImportPolicyError::DisallowedScheme(scheme));
        }

        return ensure_file_uri_within_roots(uri, workspace_folders, config);
    }

    let target_path = PathBuf::from(target);
    if target_path.is_absolute() {
        if !config.allow_absolute_file_uris {
            return Err(ImportPolicyError::AbsolutePathNotAllowed);
        }
        let normalized = normalize_path(target_path);
        let uri = Url::from_file_path(&normalized).map_err(|_| ImportPolicyError::InvalidTarget)?;
        return ensure_path_within_roots(normalized, uri, workspace_folders, config);
    }

    let Some(base_path) = file_url_to_path(base_uri) else {
        return Err(ImportPolicyError::NonFileBaseUri);
    };
    let base_dir = base_path
        .parent()
        .ok_or(ImportPolicyError::NonFileBaseUri)?;
    let candidate = normalize_path(base_dir.join(target));
    let candidate_uri =
        Url::from_file_path(&candidate).map_err(|_| ImportPolicyError::InvalidTarget)?;
    ensure_path_within_roots(candidate, candidate_uri, workspace_folders, config)
}

pub fn resolve_import_graph(
    root_uri: &Url,
    root_text: &str,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
) -> ImportGraph {
    let mut graph = ImportGraph {
        roots: vec![root_uri.clone()],
        ..ImportGraph::default()
    };
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    let mut next_file_id = 1u32;

    let root_stack = canonical_existing_path(root_uri)
        .into_iter()
        .collect::<Vec<_>>();
    visited.extend(root_stack.iter().cloned());
    graph.files.push(root_uri.clone());

    ingest_document_edges(
        root_uri,
        root_text,
        FileId(0),
        0,
        root_stack,
        workspace_folders,
        config,
        &mut graph,
        &mut queue,
    );

    while let Some(pending) = queue.pop_front() {
        if pending.depth > config.max_resolve_depth {
            graph.diagnostics.push(policy_diagnostic(
                "L023",
                ImportPolicyError::DepthLimit,
                None,
            ));
            continue;
        }

        let Some(path) = canonical_existing_path(&pending.uri) else {
            continue;
        };
        if !visited.insert(path.clone()) {
            continue;
        }
        graph.files.push(pending.uri.clone());

        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > u64::from(config.max_indexed_file_bytes) {
            graph.diagnostics.push(policy_diagnostic(
                "L020",
                ImportPolicyError::FileTooLarge,
                None,
            ));
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            graph.diagnostics.push(policy_diagnostic(
                "L019",
                ImportPolicyError::MissingTarget,
                None,
            ));
            continue;
        };

        let mut stack = pending.stack.clone();
        stack.push(path);
        ingest_document_edges(
            &pending.uri,
            &text,
            FileId(next_file_id),
            pending.depth,
            stack,
            workspace_folders,
            config,
            &mut graph,
            &mut queue,
        );
        next_file_id = next_file_id.saturating_add(1);
    }

    graph
        .files
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    graph.files.dedup();
    graph
}

pub fn collect_resolution_diagnostics(
    uri: &Url,
    text: &str,
    file_id: FileId,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
) -> Vec<Diagnostic> {
    let parsed = parser::parse_fallback(file_id, uri.as_str(), text);
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return Vec::new(),
    };
    let mut diagnostics = Vec::new();
    for directive in import_directives(file) {
        let Some(target) = directive_target(directive) else {
            continue;
        };
        let span = directive_target_span(directive, &parsed.source).or(Some(directive.span));
        match resolve_guarded_import(uri, &target, workspace_folders, config)
            .and_then(|target_uri| validate_resolved_file(&target_uri, workspace_folders, config))
        {
            Ok(()) => {}
            Err(error) if is_lexical_policy_error(&error) => {}
            Err(error) => diagnostics.push(policy_diagnostic(
                code_for_policy_error(&error),
                error,
                span,
            )),
        }
    }
    diagnostics
}

fn ingest_document_edges(
    uri: &Url,
    text: &str,
    file_id: FileId,
    depth: u32,
    stack: Vec<PathBuf>,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
    graph: &mut ImportGraph,
    queue: &mut VecDeque<PendingFile>,
) {
    let parsed = parser::parse_fallback(file_id, uri.as_str(), text);
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return,
    };

    for directive in import_directives(file) {
        if graph.edges.len() >= config.max_resolved_edges_per_file as usize {
            graph.diagnostics.push(policy_diagnostic(
                "L024",
                ImportPolicyError::EdgeLimit,
                Some(directive.span),
            ));
            return;
        }
        let Some(target) = directive_target(directive) else {
            continue;
        };
        let span = directive_target_span(directive, &parsed.source).or(Some(directive.span));
        let target_uri = match resolve_guarded_import(uri, &target, workspace_folders, config) {
            Ok(uri) => uri,
            Err(error) => {
                graph.diagnostics.push(policy_diagnostic(
                    code_for_policy_error(&error),
                    error,
                    span,
                ));
                continue;
            }
        };
        if let Err(error) = validate_resolved_file(&target_uri, workspace_folders, config) {
            graph.diagnostics.push(policy_diagnostic(
                code_for_policy_error(&error),
                error,
                span,
            ));
            continue;
        }

        graph.edges.push(ImportEdge {
            source: uri.clone(),
            target: target_uri.clone(),
            directive: directive.name.clone(),
            span,
        });

        if let Some(target_path) = canonical_existing_path(&target_uri) {
            if stack.contains(&target_path) {
                graph
                    .diagnostics
                    .push(policy_diagnostic("L021", ImportPolicyError::Cycle, span));
                continue;
            }
            if depth.saturating_add(1) > config.max_resolve_depth {
                graph.diagnostics.push(policy_diagnostic(
                    "L023",
                    ImportPolicyError::DepthLimit,
                    span,
                ));
                continue;
            }
            queue.push_back(PendingFile {
                uri: target_uri,
                depth: depth.saturating_add(1),
                stack: stack.clone(),
            });
        }
    }
}

fn ensure_file_uri_within_roots(
    uri: Url,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
) -> Result<Url, ImportPolicyError> {
    let path = file_url_to_path(&uri).ok_or(ImportPolicyError::InvalidTarget)?;
    ensure_path_within_roots(path, uri, workspace_folders, config)
}

fn ensure_path_within_roots(
    path: PathBuf,
    uri: Url,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
) -> Result<Url, ImportPolicyError> {
    let mut roots = effective_roots(workspace_folders, config);
    let canonical_roots = roots
        .iter()
        .filter_map(|root| fs::canonicalize(root).ok().map(normalize_path))
        .collect::<Vec<_>>();
    roots.extend(canonical_roots);
    roots.extend(
        workspace_folders
            .iter()
            .filter_map(|folder| file_url_to_path(&folder.uri)),
    );
    roots.extend(
        config
            .allowed_roots
            .iter()
            .map(PathBuf::from)
            .map(normalize_path),
    );
    let canonical_path = fs::canonicalize(&path)
        .ok()
        .map(normalize_path)
        .unwrap_or(path);
    if roots.is_empty()
        || roots
            .iter()
            .any(|root| is_under_root(&canonical_path, root))
    {
        Ok(uri)
    } else {
        Err(ImportPolicyError::OutsideAllowedRoots)
    }
}

fn validate_resolved_file(
    uri: &Url,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
) -> Result<(), ImportPolicyError> {
    if uri.scheme() != "file" {
        return Err(ImportPolicyError::DisallowedScheme(
            uri.scheme().to_string(),
        ));
    }
    let path = file_url_to_path(uri).ok_or(ImportPolicyError::InvalidTarget)?;
    let canonical = fs::canonicalize(&path).map_err(|_| ImportPolicyError::MissingTarget)?;
    ensure_path_within_roots(canonical.clone(), uri.clone(), workspace_folders, config)?;
    let metadata = fs::metadata(&canonical).map_err(|_| ImportPolicyError::MissingTarget)?;
    if metadata.len() > u64::from(config.max_indexed_file_bytes) {
        return Err(ImportPolicyError::FileTooLarge);
    }
    Ok(())
}

fn canonical_existing_path(uri: &Url) -> Option<PathBuf> {
    file_url_to_path(uri)
        .and_then(|path| fs::canonicalize(path).ok())
        .map(normalize_path)
}

fn is_under_root(path: &Path, root: &Path) -> bool {
    path.starts_with(root) || path_key(path).starts_with(&path_key(root))
}

fn path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    for prefix in ["//?/", "/?/", "//./", "/./"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped.to_string();
            break;
        }
    }
    value.trim_end_matches('/').to_ascii_lowercase()
}

fn strip_matching_quotes(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && *bytes.last()? == b'\'')
            || (bytes[0] == b'"' && *bytes.last()? == b'"'))
    {
        Some(&value[1..value.len() - 1])
    } else {
        None
    }
}

fn contains_parent_traversal(value: &str) -> bool {
    value.split(['/', '\\']).any(|segment| segment == "..")
}

fn import_directives(file: &AstFile) -> Vec<&Directive> {
    file.documents
        .iter()
        .flat_map(|document| &document.items)
        .filter_map(|item| match item {
            DocumentItem::Directive(directive)
                if matches!(directive.name.as_str(), "@import" | "@include" | "@schema") =>
            {
                Some(directive)
            }
            _ => None,
        })
        .collect()
}

fn directive_target(directive: &Directive) -> Option<String> {
    let value = directive.value.as_deref()?.trim();
    let target = value
        .split_once(" as ")
        .map(|(target, _)| target)
        .unwrap_or(value)
        .trim();
    Some(strip_matching_quotes(target).unwrap_or(target).to_owned())
}

fn directive_target_span(directive: &Directive, source: &SourceText) -> Option<SourceSpan> {
    let line = source.slice(directive.span)?;
    let value = directive.value.as_deref()?.trim();
    let raw_target = value
        .split_once(" as ")
        .map(|(target, _)| target.trim())
        .unwrap_or(value);
    let target = strip_matching_quotes(raw_target).unwrap_or(raw_target);
    let local = line.find(target)?;
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + local,
        directive.span.start + local + target.len(),
    ))
}

fn policy_diagnostic(code: &str, error: ImportPolicyError, span: Option<SourceSpan>) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(code, DiagnosticSeverity::Error, error.to_string())
        .with_source("lumals.imports");
    diagnostic.primary_span = span;
    diagnostic
}

fn code_for_policy_error(error: &ImportPolicyError) -> &'static str {
    match error {
        ImportPolicyError::DisallowedScheme(_) => "L010",
        ImportPolicyError::ParentTraversal => "L011",
        ImportPolicyError::AbsolutePathNotAllowed => "L012",
        ImportPolicyError::OutsideAllowedRoots => "L013",
        ImportPolicyError::MissingTarget => "L019",
        ImportPolicyError::FileTooLarge => "L020",
        ImportPolicyError::Cycle => "L021",
        ImportPolicyError::NonFileBaseUri | ImportPolicyError::InvalidTarget => "L022",
        ImportPolicyError::DepthLimit => "L023",
        ImportPolicyError::EdgeLimit => "L024",
    }
}

fn is_lexical_policy_error(error: &ImportPolicyError) -> bool {
    matches!(
        error,
        ImportPolicyError::DisallowedScheme(_)
            | ImportPolicyError::ParentTraversal
            | ImportPolicyError::AbsolutePathNotAllowed
            | ImportPolicyError::OutsideAllowedRoots
    )
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::{Url, WorkspaceFolder};

    use super::{ImportPolicyError, resolve_guarded_import};
    use crate::config::LumalsConfig;

    #[test]
    fn blocks_parent_traversal_in_relative_imports() {
        let err = resolve_guarded_import(
            &Url::parse("file:///workspace/pkg/main.luma").unwrap(),
            "../escape.luma",
            &[WorkspaceFolder {
                uri: Url::parse("file:///workspace").unwrap(),
                name: "workspace".to_string(),
            }],
            &LumalsConfig::default(),
        )
        .unwrap_err();

        assert_eq!(err, ImportPolicyError::ParentTraversal);
    }

    #[test]
    fn blocks_non_file_schemes_by_default() {
        let err = resolve_guarded_import(
            &Url::parse("file:///workspace/pkg/main.luma").unwrap(),
            "https://example.com/remote.luma",
            &[WorkspaceFolder {
                uri: Url::parse("file:///workspace").unwrap(),
                name: "workspace".to_string(),
            }],
            &LumalsConfig::default(),
        )
        .unwrap_err();

        assert_eq!(
            err,
            ImportPolicyError::DisallowedScheme("https".to_string())
        );
    }
}
