use std::collections::BTreeSet;

use parking_lot::Mutex;
use tower_lsp::lsp_types::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedDocumentGuard {
    uri: Url,
    version: i32,
}

impl VersionedDocumentGuard {
    #[must_use]
    pub fn new(uri: Url, version: i32) -> Self {
        Self { uri, version }
    }

    #[must_use]
    pub fn uri(&self) -> &Url {
        &self.uri
    }

    #[must_use]
    pub fn version(&self) -> i32 {
        self.version
    }

    #[must_use]
    pub fn is_current(&self, latest_version: Option<i32>) -> bool {
        latest_version == Some(self.version)
    }
}

#[derive(Debug, Default)]
pub struct CancellationRegistry {
    cancelled: Mutex<BTreeSet<u64>>,
}

impl CancellationRegistry {
    pub fn cancel(&self, id: u64) {
        self.cancelled.lock().insert(id);
    }

    #[must_use]
    pub fn is_cancelled(&self, id: u64) -> bool {
        self.cancelled.lock().contains(&id)
    }

    pub fn finish(&self, id: u64) {
        self.cancelled.lock().remove(&id);
    }
}

#[must_use]
pub fn debounce_millis_for_document_change() -> u64 {
    50
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::Url;

    use super::{
        CancellationRegistry, VersionedDocumentGuard, debounce_millis_for_document_change,
    };

    #[test]
    fn versioned_guard_rejects_stale_results() {
        let guard = VersionedDocumentGuard::new(Url::parse("file:///test.luma").unwrap(), 2);

        assert!(guard.is_current(Some(2)));
        assert!(!guard.is_current(Some(3)));
        assert!(!guard.is_current(None));
    }

    #[test]
    fn cancellation_registry_tracks_request_ids() {
        let registry = CancellationRegistry::default();

        registry.cancel(7);
        assert!(registry.is_cancelled(7));
        registry.finish(7);
        assert!(!registry.is_cancelled(7));
    }

    #[test]
    fn debounce_baseline_is_nonzero() {
        assert!(debounce_millis_for_document_change() > 0);
    }
}
