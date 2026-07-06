mod support;

use serde_json::{Value, json};
use tempfile::tempdir;
use tower_lsp::lsp_types::Url;

use support::LspHarness;

#[tokio::test(flavor = "current_thread")]
async fn vscode_compatible_clients_get_static_feature_capabilities() {
    let temp = tempdir().unwrap();
    let root_uri = Url::from_directory_path(temp.path()).unwrap();
    let mut harness = LspHarness::start().await;

    let response = harness
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
                "capabilities": {
                    "workspace": {
                        "configuration": true,
                        "workspaceFolders": true
                    },
                    "textDocument": {
                        "completion": { "completionItem": { "snippetSupport": false } },
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "semanticTokens": {
                            "requests": { "full": true },
                            "tokenTypes": [],
                            "tokenModifiers": [],
                            "formats": ["relative"]
                        }
                    }
                }
            }),
        )
        .await;

    let capabilities = &response["result"]["capabilities"];
    assert_eq!(capabilities["positionEncoding"], "utf-16");
    assert_eq!(capabilities["hoverProvider"], true);
    assert_eq!(capabilities["definitionProvider"], true);
    assert_eq!(capabilities["declarationProvider"], true);
    assert_eq!(capabilities["typeDefinitionProvider"], true);
    assert_eq!(capabilities["implementationProvider"], true);
    assert_eq!(capabilities["documentSymbolProvider"], true);
    assert_eq!(capabilities["workspaceSymbolProvider"], true);
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert_eq!(capabilities["documentRangeFormattingProvider"], true);
    assert_eq!(capabilities["renameProvider"]["prepareProvider"], true);
    assert_eq!(capabilities["inlayHintProvider"], true);
    assert_eq!(capabilities["selectionRangeProvider"], true);
    assert_eq!(capabilities["foldingRangeProvider"], true);
    assert_eq!(capabilities["semanticTokensProvider"]["full"], true);
    assert_eq!(
        capabilities["workspace"]["workspaceFolders"]["supported"],
        true
    );
    assert_array_contains(
        &capabilities["executeCommandProvider"]["commands"],
        "lymals.serverStatus",
    );
    assert_array_contains(
        &capabilities["codeActionProvider"]["codeActionKinds"],
        "quickfix",
    );
    assert_array_contains(
        &capabilities["codeActionProvider"]["codeActionKinds"],
        "source.organizeImports",
    );

    harness.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn neovim_style_clients_negotiate_utf16_and_simple_stdio_registration() {
    let temp = tempdir().unwrap();
    let root_uri = Url::from_directory_path(temp.path()).unwrap();
    let doc_uri = Url::from_file_path(temp.path().join("main.lyma")).unwrap();
    let mut harness = LspHarness::start().await;

    let response = harness
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {
                    "general": { "positionEncodings": ["utf-8", "utf-16"] },
                    "textDocument": {
                        "synchronization": { "didSave": true },
                        "completion": {},
                        "hover": {},
                        "foldingRange": {},
                        "selectionRange": {},
                        "inlayHint": {},
                        "semanticTokens": {
                            "requests": { "full": true },
                            "tokenTypes": [],
                            "tokenModifiers": [],
                            "formats": ["relative"]
                        }
                    }
                }
            }),
        )
        .await;

    let capabilities = &response["result"]["capabilities"];
    assert_eq!(capabilities["positionEncoding"], "utf-16");
    assert_eq!(capabilities["textDocumentSync"]["openClose"], true);
    assert_eq!(capabilities["textDocumentSync"]["change"], 1);
    assert!(capabilities.get("workspace").is_none() || capabilities["workspace"].is_null());

    harness.initialized().await;
    let diagnostics = harness.did_open(&doc_uri, "root:\n  child: true\n").await;
    assert_eq!(diagnostics["method"], "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));

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

    harness.shutdown().await;
}

fn assert_array_contains(array: &Value, expected: &str) {
    assert!(
        array
            .as_array()
            .expect("capability should be an array")
            .iter()
            .any(|item| item.as_str() == Some(expected)),
        "expected {expected:?} in {array}"
    );
}
