use tower_lsp::lsp_types::{
    DidChangeWatchedFilesParams, DidChangeWatchedFilesRegistrationOptions,
    DidChangeWorkspaceFoldersParams, FileSystemWatcher, GlobPattern, OneOf, Registration,
    RelativePattern, WatchKind, WorkspaceFolder,
};

use crate::workspace::{LYMA_GLOB, WATCH_REGISTRATION_ID};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_did_change_workspace_folders(
        &self,
        params: DidChangeWorkspaceFoldersParams,
    ) {
        let event = params.event;
        self.state
            .apply_workspace_folder_change(event.added, event.removed);
        self.trace("workspace folders changed", None).await;
    }

    pub(super) async fn handle_did_change_watched_files(
        &self,
        params: DidChangeWatchedFilesParams,
    ) {
        let invalidated = self.state.note_watched_file_changes(&params.changes);

        for uri in invalidated {
            self.publish_document_diagnostics(&uri).await;
        }
    }

    pub(super) async fn register_lyma_file_watchers(&self) {
        let Some(registration) = self.file_watch_registration() else {
            return;
        };

        if let Err(error) = self.client.register_capability(vec![registration]).await {
            self.trace(
                "workspace watcher registration failed",
                Some(format!("error={error}")),
            )
            .await;
        } else {
            self.trace(
                "workspace watcher registration succeeded",
                Some(format!("glob={LYMA_GLOB}")),
            )
            .await;
        }
    }

    fn file_watch_registration(&self) -> Option<Registration> {
        let snapshot = self.state.snapshot();
        let workspace = snapshot.client_capabilities.workspace.as_ref()?;
        let watched = workspace.did_change_watched_files.as_ref()?;
        if !watched.dynamic_registration.unwrap_or(false) {
            return None;
        }

        let watchers = if watched.relative_pattern_support.unwrap_or(false)
            && !snapshot.workspace.folders.is_empty()
        {
            snapshot
                .workspace
                .folders
                .iter()
                .cloned()
                .map(relative_file_system_watcher)
                .collect()
        } else {
            vec![FileSystemWatcher {
                glob_pattern: GlobPattern::String(LYMA_GLOB.to_string()),
                kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
            }]
        };

        serde_json::to_value(DidChangeWatchedFilesRegistrationOptions { watchers })
            .ok()
            .map(|register_options| Registration {
                id: WATCH_REGISTRATION_ID.to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: Some(register_options),
            })
    }
}

fn relative_file_system_watcher(folder: WorkspaceFolder) -> FileSystemWatcher {
    FileSystemWatcher {
        glob_pattern: GlobPattern::Relative(RelativePattern {
            base_uri: OneOf::Left(folder),
            pattern: LYMA_GLOB.to_string(),
        }),
        kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
    }
}
