use std::path::{Path, PathBuf};

use thiserror::Error;
use tower_lsp::lsp_types::Url;

use crate::config::LumalsConfig;
use crate::workspace::{effective_roots, file_url_to_path, normalize_path};

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
}

pub fn resolve_guarded_import(
    base_uri: &Url,
    target: &str,
    workspace_folders: &[tower_lsp::lsp_types::WorkspaceFolder],
    config: &LumalsConfig,
) -> Result<Url, ImportPolicyError> {
    let target = strip_matching_quotes(target).unwrap_or(target);

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
            return Ok(uri);
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
    let roots = effective_roots(workspace_folders, config);
    if roots.is_empty() || roots.iter().any(|root| is_under_root(&path, root)) {
        Ok(uri)
    } else {
        Err(ImportPolicyError::OutsideAllowedRoots)
    }
}

fn is_under_root(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
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
