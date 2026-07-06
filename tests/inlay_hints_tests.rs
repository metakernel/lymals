use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lymals::server;

#[tokio::test(flavor = "current_thread")]
async fn inlay_hints_are_configurable_stable_and_quiet_by_default() {
    let (client_to_server, server_stdin) = tokio::io::duplex(16 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(16 * 1024);
    let (service, socket) = server::service();
    let server_task = tokio::spawn(async move {
        Server::new(server_stdin, server_stdout, socket)
            .serve(service)
            .await;
    });

    let mut writer = client_to_server;
    let mut reader = server_to_client;
    let workspace_dir = tempdir().unwrap();
    let workspace = Url::from_directory_path(workspace_dir.path()).unwrap();
    let uri = Url::from_file_path(workspace_dir.path().join("inlay-hints.lyma")).unwrap();
    let shared = Url::from_file_path(workspace_dir.path().join("shared.lyma")).unwrap();

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": workspace,
                "capabilities": {}
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["inlayHintProvider"],
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
    let _initialized_log = read_message(&mut reader).await;

    let text = concat!(
        "@import \"./shared.lyma\" as shared\n",
        "@profile dev\n",
        "let region = ${shared}\n",
        "service:\n",
        "  retries: 3\n",
        "  enabled: true\n",
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "lyma",
                    "version": 1,
                    "text": text
                }
            }
        }),
    )
    .await;
    let _diagnostics = read_message(&mut reader).await;

    let quiet = inlay_hints(&mut writer, &mut reader, &uri, 0, 0, 5, 15, 2).await;
    assert_eq!(quiet["result"], json!([]));

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeConfiguration",
            "params": {
                "settings": {
                    "lymals": {
                        "inlayHints": {
                            "enabled": true,
                            "inferredTypes": true,
                            "keyPaths": true,
                            "letBindings": true,
                            "profileEffects": true,
                            "importResolution": true
                        }
                    }
                }
            }
        }),
    )
    .await;

    let first = inlay_hints(&mut writer, &mut reader, &uri, 0, 0, 5, 15, 3).await;
    let second = inlay_hints(&mut writer, &mut reader, &uri, 0, 0, 5, 15, 4).await;
    assert_eq!(first["result"], second["result"]);
    assert_eq!(
        first["result"],
        json!([
            {
                "position": { "line": 0, "character": 22 },
                "label": format!("→ {shared}"),
                "kind": 2,
                "paddingLeft": true
            },
            {
                "position": { "line": 1, "character": 12 },
                "label": "activates `dev` profile",
                "kind": 2,
                "paddingLeft": true
            },
            {
                "position": { "line": 2, "character": 10 },
                "label": "= ${shared} : lua-expression",
                "kind": 1,
                "paddingLeft": true
            },
            {
                "position": { "line": 3, "character": 7 },
                "label": "document[1].service",
                "kind": 2,
                "paddingLeft": true
            },
            {
                "position": { "line": 4, "character": 9 },
                "label": "document[1].service.retries",
                "kind": 2,
                "paddingLeft": true
            },
            {
                "position": { "line": 4, "character": 12 },
                "label": ": number",
                "kind": 1,
                "paddingLeft": true
            },
            {
                "position": { "line": 5, "character": 9 },
                "label": "document[1].service.enabled",
                "kind": 2,
                "paddingLeft": true
            },
            {
                "position": { "line": 5, "character": 15 },
                "label": ": boolean",
                "kind": 1,
                "paddingLeft": true
            }
        ])
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeConfiguration",
            "params": {
                "settings": {
                    "lymals": {
                        "inlayHints": {
                            "enabled": false
                        }
                    }
                }
            }
        }),
    )
    .await;

    let disabled = inlay_hints(&mut writer, &mut reader, &uri, 0, 0, 5, 15, 5).await;
    assert!(disabled["result"].is_null());

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[allow(clippy::too_many_arguments)]
async fn inlay_hints(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
    id: u64,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/inlayHint",
            "params": {
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_character },
                    "end": { "line": end_line, "character": end_character }
                }
            }
        }),
    )
    .await;

    read_message(reader).await
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
            "id": 99,
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
