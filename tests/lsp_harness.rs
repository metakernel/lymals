mod support;

use serde_json::json;
use tempfile::tempdir;
use tower_lsp::lsp_types::Url;

use support::LspHarness;

#[tokio::test(flavor = "current_thread")]
async fn harness_drives_lifecycle_sync_diagnostics_and_requests() {
    let temp = tempdir().unwrap();
    let root_uri = Url::from_directory_path(temp.path()).unwrap();
    let doc_uri = Url::from_file_path(temp.path().join("main.lyma")).unwrap();
    let mut harness = LspHarness::start().await;

    let initialize = harness.initialize(&root_uri).await;
    assert_eq!(initialize["result"]["serverInfo"]["name"], "lymals");

    let initialized_log = harness.initialized().await;
    assert_eq!(initialized_log["method"], "window/logMessage");

    let diagnostics = harness.did_open(&doc_uri, "root:\n\tbad: true\n").await;
    assert_eq!(diagnostics["method"], "textDocument/publishDiagnostics");
    assert!(
        diagnostics["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "L003")
    );

    let cleared = harness
        .did_change(&doc_uri, 2, "root:\n  good: true\n")
        .await;
    assert_eq!(cleared["params"]["diagnostics"], json!([]));

    let hover = harness
        .request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": doc_uri },
                "position": { "line": 0, "character": 1 }
            }),
        )
        .await;
    assert!(hover["result"].is_object());

    let normalized = LspHarness::normalize_workspace(diagnostics, &root_uri);
    assert!(
        normalized["params"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("file:///<WORKSPACE>")
    );

    harness.shutdown().await;
}
