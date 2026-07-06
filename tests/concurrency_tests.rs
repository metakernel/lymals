mod support;

use serde_json::json;
use tempfile::tempdir;
use tower_lsp::lsp_types::Url;

use lymals::{
    state::SessionState,
    tasks::{CancellationRegistry, VersionedDocumentGuard},
};
use support::LspHarness;

#[test]
fn versioned_work_guards_detect_stale_document_versions() {
    let state = SessionState::new();
    let uri = Url::parse("file:///workspace/stale.lyma").unwrap();
    state.open_document(uri.clone(), 1, "root: true\n");

    let guard = VersionedDocumentGuard::new(uri.clone(), 1);
    assert!(guard.is_current(state.document_version(&uri)));

    state.update_document(&uri, 2, "root: false\n").unwrap();
    assert!(!guard.is_current(state.document_version(&uri)));
}

#[test]
fn cancellation_registry_honors_request_lifecycle() {
    let registry = CancellationRegistry::default();

    registry.cancel(42);
    assert!(registry.is_cancelled(42));
    registry.finish(42);
    assert!(!registry.is_cancelled(42));
}

#[tokio::test(flavor = "current_thread")]
async fn rapid_changes_publish_only_latest_observed_version() {
    let temp = tempdir().unwrap();
    let root_uri = Url::from_directory_path(temp.path()).unwrap();
    let doc_uri = Url::from_file_path(temp.path().join("concurrency.lyma")).unwrap();
    let mut harness = LspHarness::start().await;

    harness.initialize(&root_uri).await;
    let _ = harness.initialized().await;
    let _ = harness.did_open(&doc_uri, "root:\n\tbad: true\n").await;
    harness
        .notify("$/cancelRequest", json!({ "id": 123 }))
        .await;

    let latest = harness
        .did_change(&doc_uri, 2, "root:\n  good: true\n")
        .await;

    assert_eq!(latest["params"]["version"], 2);
    assert_eq!(latest["params"]["diagnostics"], json!([]));

    harness.shutdown().await;
}
