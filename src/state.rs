use parking_lot::RwLock;
use tower_lsp::lsp_types::{ClientCapabilities, TraceValue, Url};

use crate::config::LumalsConfig;
use crate::document::{Document, DocumentSnapshot, DocumentStore, DocumentStoreError};
use crate::syntax::FileId;
use crate::workspace::{WorkspaceSnapshot, WorkspaceState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LifecyclePhase {
    #[default]
    Created,
    Initialized,
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub client_capabilities: ClientCapabilities,
    pub config: LumalsConfig,
    pub trace: TraceValue,
    pub phase: LifecyclePhase,
    pub workspace: WorkspaceSnapshot,
}

#[derive(Debug, Default)]
pub struct SessionState {
    inner: RwLock<SessionSnapshot>,
    documents: RwLock<DocumentStore>,
    workspace: RwLock<WorkspaceState>,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            client_capabilities: ClientCapabilities::default(),
            config: LumalsConfig::default(),
            trace: TraceValue::Off,
            phase: LifecyclePhase::Created,
            workspace: WorkspaceSnapshot::default(),
        }
    }
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let mut snapshot = self.inner.read().clone();
        snapshot.workspace = self.workspace.read().snapshot();
        snapshot
    }

    pub fn set_client_capabilities(&self, capabilities: ClientCapabilities) {
        self.inner.write().client_capabilities = capabilities;
    }

    pub fn set_trace(&self, trace: TraceValue) {
        self.inner.write().trace = trace;
    }

    pub fn set_config(&self, config: LumalsConfig) {
        self.inner.write().config = config;
    }

    pub fn set_workspace_folders(&self, folders: Vec<Url>) {
        let folders = folders
            .into_iter()
            .map(|uri| tower_lsp::lsp_types::WorkspaceFolder {
                name: uri
                    .path_segments()
                    .and_then(Iterator::last)
                    .filter(|segment| !segment.is_empty())
                    .unwrap_or("workspace")
                    .to_string(),
                uri,
            })
            .collect();
        self.set_workspace_folder_entries(folders);
    }

    pub fn set_workspace_folder_entries(
        &self,
        folders: Vec<tower_lsp::lsp_types::WorkspaceFolder>,
    ) {
        self.workspace.write().set_folders(folders);
    }

    pub fn apply_workspace_folder_change(
        &self,
        added: Vec<tower_lsp::lsp_types::WorkspaceFolder>,
        removed: Vec<tower_lsp::lsp_types::WorkspaceFolder>,
    ) {
        self.workspace.write().apply_folder_change(added, removed);
    }

    pub fn note_watched_file_changes(
        &self,
        changes: &[tower_lsp::lsp_types::FileEvent],
    ) -> Vec<Url> {
        let snapshot = self.inner.read().clone();
        self.workspace
            .write()
            .note_watched_file_changes(changes, &snapshot.config)
    }

    pub fn mark_initialized(&self) {
        self.inner.write().phase = LifecyclePhase::Initialized;
    }

    pub fn mark_shutdown(&self) {
        self.inner.write().phase = LifecyclePhase::Shutdown;
    }

    pub fn open_document(&self, uri: Url, version: i32, text: impl AsRef<str>) -> FileId {
        self.documents.write().open(uri, version, text)
    }

    pub fn update_document(
        &self,
        uri: &Url,
        version: i32,
        text: impl AsRef<str>,
    ) -> Result<FileId, DocumentStoreError> {
        self.documents.write().update(uri, version, text)
    }

    pub fn close_document(&self, uri: &Url) -> Option<Document> {
        self.documents.write().close(uri)
    }

    #[must_use]
    pub fn has_document(&self, uri: &Url) -> bool {
        self.documents.read().get(uri).is_some()
    }

    #[must_use]
    pub fn document_count(&self) -> usize {
        self.documents.read().len()
    }

    pub fn with_document<R>(&self, uri: &Url, f: impl FnOnce(&Document) -> R) -> Option<R> {
        let documents = self.documents.read();
        documents.get(uri).map(f)
    }

    pub fn with_document_mut<R>(&self, uri: &Url, f: impl FnOnce(&mut Document) -> R) -> Option<R> {
        let mut documents = self.documents.write();
        documents.get_mut(uri).map(f)
    }

    pub fn open_document_snapshots(&self) -> Vec<DocumentSnapshot> {
        self.documents.read().snapshots()
    }
}
