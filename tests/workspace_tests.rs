use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::{Url, WorkspaceFolder};

use lymals::config::LymalsConfig;
use lymals::imports::{ImportPolicyError, resolve_guarded_import};
use lymals::server;
use lymals::workspace::{self, WorkspaceLymaFilePolicyError};

#[test]
fn blocks_path_traversal_during_import_resolution() {
    let err = resolve_guarded_import(
        &Url::parse("file:///workspace/pkg/main.lyma").unwrap(),
        "../escape.lyma",
        &[WorkspaceFolder {
            uri: Url::parse("file:///workspace").unwrap(),
            name: "workspace".to_string(),
        }],
        &LymalsConfig::default(),
    )
    .unwrap_err();

    assert_eq!(err, ImportPolicyError::ParentTraversal);
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_root_changes_and_file_watches_update_state_and_invalidate_open_documents() {
    let workspace_a = tempfile::tempdir().unwrap();
    let workspace_added = tempfile::tempdir().unwrap();
    let workspace_a_uri = Url::from_directory_path(workspace_a.path()).unwrap();
    let workspace_added_uri = Url::from_directory_path(workspace_added.path()).unwrap();
    let doc_uri = Url::from_file_path(workspace_added.path().join("src/main.lyma")).unwrap();

    let (client_to_server, server_stdin) = tokio::io::duplex(16 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(16 * 1024);

    let (service, socket) = server::service();
    let backend = service.inner().clone();

    let server_task = tokio::spawn(async move {
        Server::new(server_stdin, server_stdout, socket)
            .serve(service)
            .await;
    });

    let mut writer = client_to_server;
    let mut reader = server_to_client;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "capabilities": {
                    "workspace": {
                        "workspaceFolders": true,
                        "didChangeWatchedFiles": {
                            "dynamicRegistration": true,
                            "relativePatternSupport": true
                        }
                    }
                },
                "workspaceFolders": [{
                    "uri": workspace_a_uri,
                    "name": "workspace-a"
                }]
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["workspace"]["workspaceFolders"]["changeNotifications"],
        true
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )
    .await;

    let register_request = read_message(&mut reader).await;
    assert_eq!(register_request["method"], "client/registerCapability");
    assert_eq!(
        register_request["params"]["registrations"][0]["registerOptions"]["watchers"][0]["globPattern"]
            ["pattern"],
        "**/*.lyma"
    );
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": register_request["id"],
            "result": null
        }),
    )
    .await;

    let _initialized_log = read_message(&mut reader).await;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWorkspaceFolders",
            "params": {
                "event": {
                    "added": [{ "uri": workspace_added_uri, "name": "workspace-added" }],
                    "removed": [{ "uri": workspace_a_uri, "name": "workspace-a" }]
                }
            }
        }),
    )
    .await;

    tokio::task::yield_now().await;
    let snapshot = backend.state_snapshot();
    assert_eq!(snapshot.workspace.folders.len(), 1);
    assert_eq!(snapshot.workspace.folders[0].uri, workspace_added_uri);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": doc_uri,
                    "languageId": "lyma",
                    "version": 1,
                    "text": "let answer = 1"
                }
            }
        }),
    )
    .await;

    let open_publish = read_message(&mut reader).await;
    assert_eq!(open_publish["method"], "textDocument/publishDiagnostics");
    assert_eq!(open_publish["params"]["version"], 1);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [{
                    "uri": doc_uri,
                    "type": 2
                }]
            }
        }),
    )
    .await;

    let watched_publish = read_message(&mut reader).await;
    assert_eq!(watched_publish["method"], "textDocument/publishDiagnostics");
    assert_eq!(watched_publish["params"]["uri"], doc_uri.as_str());

    let snapshot = backend.state_snapshot();
    assert_eq!(snapshot.workspace.watched_file_invalidations, 1);
    assert_eq!(
        snapshot.workspace.last_invalidated_files,
        vec![doc_uri.clone()]
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[test]
fn workspace_lyma_validator_rejects_canonical_targets_outside_roots() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("escape.lyma");
    std::fs::write(&outside_path, "root: true\n").unwrap();

    let validated = workspace::validate_workspace_lyma_file_uri(
        &Url::from_file_path(&outside_path).unwrap(),
        &[WorkspaceFolder {
            uri: Url::from_directory_path(workspace.path()).unwrap(),
            name: "workspace".to_string(),
        }],
        &LymalsConfig::default(),
    );

    assert_eq!(
        validated.unwrap_err(),
        WorkspaceLymaFilePolicyError::OutsideAllowedRoots
    );
}

#[test]
fn workspace_lyma_validator_rejects_symlink_escape_when_supported() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("escape.lyma");
    std::fs::write(&outside_path, "root: true\n").unwrap();

    let link_path = workspace.path().join("linked.lyma");
    if try_create_file_symlink(&outside_path, &link_path).is_err() {
        return;
    }

    let validated = workspace::validate_workspace_lyma_file_uri(
        &Url::from_file_path(&link_path).unwrap(),
        &[WorkspaceFolder {
            uri: Url::from_directory_path(workspace.path()).unwrap(),
            name: "workspace".to_string(),
        }],
        &LymalsConfig::default(),
    );

    assert_eq!(
        validated.unwrap_err(),
        WorkspaceLymaFilePolicyError::OutsideAllowedRoots
    );
}

async fn shutdown_server(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    server_task: tokio::task::JoinHandle<()>,
) {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown"
        }),
    )
    .await;

    let _shutdown_response = read_message(reader).await;
    let _shutdown_log = read_message(reader).await;

    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
    )
    .await;
    writer.shutdown().await.unwrap();

    timeout(Duration::from_secs(2), server_task)
        .await
        .expect("server task timed out")
        .expect("server task failed");
}

async fn send_message(stream: &mut tokio::io::DuplexStream, value: &Value) {
    let body = value.to_string();
    let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stream.write_all(message.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
}

async fn read_message(stream: &mut tokio::io::DuplexStream) -> Value {
    let mut headers = Vec::new();
    let mut byte = [0u8; 1];

    loop {
        stream.read_exact(&mut byte).await.unwrap();
        headers.push(byte[0]);
        if headers.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let headers = String::from_utf8(headers).unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .unwrap()
        .parse::<usize>()
        .unwrap();

    let mut body = vec![0; content_length];
    stream.read_exact(&mut body).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[cfg(unix)]
fn try_create_file_symlink(
    original: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn try_create_file_symlink(
    original: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(original, link)
}
