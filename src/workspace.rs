use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use tower_lsp::lsp_types::{FileEvent, Url, WorkspaceFolder};

use crate::config::LumalsConfig;

pub const LUMA_GLOB: &str = "**/*.luma";
pub const WATCH_REGISTRATION_ID: &str = "lumals-watch-luma-files";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceSnapshot {
    pub folders: Vec<WorkspaceFolder>,
    pub watched_file_invalidations: u64,
    pub last_invalidated_files: Vec<Url>,
}

#[derive(Debug, Default)]
pub struct WorkspaceState {
    folders: Vec<WorkspaceFolder>,
    watched_file_invalidations: u64,
    last_invalidated_files: Vec<Url>,
}

impl WorkspaceState {
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            folders: self.folders.clone(),
            watched_file_invalidations: self.watched_file_invalidations,
            last_invalidated_files: self.last_invalidated_files.clone(),
        }
    }

    pub fn set_folders(&mut self, folders: Vec<WorkspaceFolder>) {
        self.folders = dedupe_folders(folders);
    }

    pub fn apply_folder_change(
        &mut self,
        added: Vec<WorkspaceFolder>,
        removed: Vec<WorkspaceFolder>,
    ) {
        let removed: BTreeSet<_> = removed.into_iter().map(|folder| folder.uri).collect();
        self.folders.retain(|folder| !removed.contains(&folder.uri));
        self.folders.extend(added);
        self.folders = dedupe_folders(std::mem::take(&mut self.folders));
    }

    pub fn note_watched_file_changes(
        &mut self,
        changes: &[FileEvent],
        config: &LumalsConfig,
    ) -> Vec<Url> {
        let mut invalidated = BTreeSet::new();
        for change in changes {
            if is_workspace_luma_uri(&change.uri, &self.folders, config) {
                invalidated.insert(change.uri.clone());
            }
        }

        let invalidated: Vec<_> = invalidated.into_iter().collect();
        if !invalidated.is_empty() {
            self.watched_file_invalidations = self.watched_file_invalidations.saturating_add(1);
            self.last_invalidated_files = invalidated.clone();
        }
        invalidated
    }
}

pub fn folders_from_initialize_params(
    workspace_folders: Option<Vec<WorkspaceFolder>>,
    root_uri: Option<Url>,
    root_path: Option<String>,
) -> Vec<WorkspaceFolder> {
    if let Some(folders) = workspace_folders {
        return dedupe_folders(folders);
    }

    root_uri
        .or_else(|| {
            root_path.and_then(|path| {
                let path = PathBuf::from(path);
                Url::from_directory_path(path).ok()
            })
        })
        .map(|uri| {
            vec![WorkspaceFolder {
                name: workspace_folder_name(&uri),
                uri,
            }]
        })
        .unwrap_or_default()
}

pub fn is_workspace_luma_uri(
    uri: &Url,
    workspace_folders: &[WorkspaceFolder],
    config: &LumalsConfig,
) -> bool {
    if !config.index_workspace || uri.scheme() != "file" || !uri.path().ends_with(".luma") {
        return false;
    }

    let Some(path) = file_url_to_path(uri) else {
        return false;
    };

    let Some(relative_path) = path_relative_to_any_root(&path, workspace_folders, config) else {
        return false;
    };

    !matches_any_glob(
        &normalize_path_for_glob(&relative_path),
        &config.exclude_globs,
    )
}

pub fn effective_roots(
    workspace_folders: &[WorkspaceFolder],
    config: &LumalsConfig,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for folder in workspace_folders {
        if let Some(path) = file_url_to_path(&folder.uri) {
            roots.push(normalize_path(path));
        }
    }

    for root in &config.allowed_roots {
        if let Some(path) = parse_root(root) {
            roots.push(normalize_path(path));
        }
    }

    roots.sort();
    roots.dedup();
    roots
}

pub fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub fn file_url_to_path(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path().ok().map(normalize_path)
}

fn parse_root(root: &str) -> Option<PathBuf> {
    Url::parse(root)
        .ok()
        .and_then(|uri| (uri.scheme() == "file").then_some(uri))
        .and_then(|uri| file_url_to_path(&uri))
        .or_else(|| Some(normalize_path(PathBuf::from(root))))
}

fn path_relative_to_any_root(
    path: &Path,
    workspace_folders: &[WorkspaceFolder],
    config: &LumalsConfig,
) -> Option<PathBuf> {
    for root in effective_roots(workspace_folders, config) {
        if let Ok(relative) = path.strip_prefix(&root) {
            return Some(relative.to_path_buf());
        }
    }
    None
}

fn dedupe_folders(folders: Vec<WorkspaceFolder>) -> Vec<WorkspaceFolder> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for folder in folders {
        if seen.insert(folder.uri.clone()) {
            deduped.push(folder);
        }
    }
    deduped
}

fn workspace_folder_name(uri: &Url) -> String {
    uri.path_segments()
        .and_then(Iterator::last)
        .filter(|segment| !segment.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

fn normalize_path_for_glob(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn matches_any_glob(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| glob_matches(path, pattern))
}

fn glob_matches(path: &str, pattern: &str) -> bool {
    let path: Vec<_> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let pattern: Vec<_> = pattern
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    glob_segments_match(&path, &pattern)
}

fn glob_segments_match(path: &[&str], pattern: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            glob_segments_match(path, rest)
                || (!path.is_empty() && glob_segments_match(&path[1..], pattern))
        }
        Some((segment, rest)) => {
            !path.is_empty()
                && glob_segment_matches(path[0], segment)
                && glob_segments_match(&path[1..], rest)
        }
    }
}

fn glob_segment_matches(value: &str, pattern: &str) -> bool {
    let value: Vec<_> = value.chars().collect();
    let pattern: Vec<_> = pattern.chars().collect();
    glob_chars_match(&value, &pattern)
}

fn glob_chars_match(value: &[char], pattern: &[char]) -> bool {
    match pattern.split_first() {
        None => value.is_empty(),
        Some((&'*', rest)) => {
            glob_chars_match(value, rest)
                || (!value.is_empty() && glob_chars_match(&value[1..], pattern))
        }
        Some((&'?', rest)) => !value.is_empty() && glob_chars_match(&value[1..], rest),
        Some((expected, rest)) => {
            value.first() == Some(expected) && glob_chars_match(&value[1..], rest)
        }
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::WorkspaceFolder;

    use super::{LUMA_GLOB, WorkspaceState, folders_from_initialize_params, glob_matches};
    use crate::config::LumalsConfig;

    #[test]
    fn initializes_workspace_from_root_uri_when_folders_are_missing() {
        let folders = folders_from_initialize_params(
            None,
            Some(tower_lsp::lsp_types::Url::parse("file:///workspace").unwrap()),
            None,
        );

        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "workspace");
    }

    #[test]
    fn glob_matching_supports_recursive_patterns() {
        assert!(glob_matches("nested/file.luma", LUMA_GLOB));
        assert!(glob_matches("a/generated/file.luma", "**/generated/**"));
        assert!(!glob_matches("a/source/file.luma", "**/generated/**"));
    }

    #[test]
    fn watched_file_invalidations_are_root_and_glob_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let workspace_uri = tower_lsp::lsp_types::Url::from_directory_path(temp.path()).unwrap();
        let source_uri =
            tower_lsp::lsp_types::Url::from_file_path(temp.path().join("source/test.luma"))
                .unwrap();
        let generated_uri =
            tower_lsp::lsp_types::Url::from_file_path(temp.path().join("generated/test.luma"))
                .unwrap();

        let mut workspace = WorkspaceState::default();
        workspace.set_folders(vec![WorkspaceFolder {
            uri: workspace_uri,
            name: "workspace".to_string(),
        }]);

        let config = LumalsConfig {
            exclude_globs: vec!["**/generated/**".to_string()],
            ..LumalsConfig::default()
        };

        let invalidated = workspace.note_watched_file_changes(
            &[
                tower_lsp::lsp_types::FileEvent::new(
                    source_uri,
                    tower_lsp::lsp_types::FileChangeType::CHANGED,
                ),
                tower_lsp::lsp_types::FileEvent::new(
                    generated_uri,
                    tower_lsp::lsp_types::FileChangeType::CHANGED,
                ),
            ],
            &config,
        );

        assert_eq!(invalidated.len(), 1);
        assert_eq!(workspace.snapshot().watched_file_invalidations, 1);
    }
}
