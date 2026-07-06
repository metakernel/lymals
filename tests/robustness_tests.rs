mod support;

use serde_json::json;
use tempfile::tempdir;
use tower_lsp::lsp_types::Url;

use lymals::{diagnostics, parser, syntax::FileId};
use support::LspHarness;

#[test]
fn malformed_and_deeply_nested_inputs_never_panic() {
    let malformed_inputs = [
        "\0\0\0",
        "root:\n   odd: true\n      jump: true\n",
        "unterminated: \"value\n",
        "script: ```lua\n  if true then\n",
        "root:\n  a:\n    b:\n      c:\n        d:\n          e:\n            f: true\n",
    ];

    for (index, text) in malformed_inputs.iter().enumerate() {
        let parsed = parser::parse_fallback(FileId(index as u32 + 1), "malformed.lyma", text);
        let _ = diagnostics::collect(&parsed);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn rapid_edits_and_cancellation_notifications_do_not_publish_stale_final_diagnostics() {
    let temp = tempdir().unwrap();
    let root_uri = Url::from_directory_path(temp.path()).unwrap();
    let doc_uri = Url::from_file_path(temp.path().join("rapid.lyma")).unwrap();
    let mut harness = LspHarness::start().await;

    harness.initialize(&root_uri).await;
    let _ = harness.initialized().await;
    let _ = harness.did_open(&doc_uri, "root:\n\tbad: true\n").await;

    harness
        .notify("$/cancelRequest", json!({ "id": 999 }))
        .await;
    let diagnostics = harness
        .did_change(&doc_uri, 2, "root:\n  good: true\n")
        .await;

    assert_eq!(diagnostics["method"], "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["version"], 2);
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));

    harness.shutdown().await;
}
