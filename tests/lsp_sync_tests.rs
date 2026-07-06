use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lymals::server;

#[tokio::test(flavor = "current_thread")]
async fn text_document_sync_handles_open_change_save_and_close() {
    let (client_to_server, server_stdin) = tokio::io::duplex(8 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(8 * 1024);

    let (service, socket) = server::service();
    let backend = service.inner().clone();

    let server_task = tokio::spawn(async move {
        Server::new(server_stdin, server_stdout, socket)
            .serve(service)
            .await;
    });

    let mut writer = client_to_server;
    let mut reader = server_to_client;
    let uri = Url::parse("file:///workspace/test.lyma").unwrap();

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "clientInfo": { "name": "lymals-test", "version": "0" },
                "capabilities": {},
                "trace": "off"
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["textDocumentSync"]["openClose"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["textDocumentSync"]["change"],
        1
    );
    assert_eq!(
        initialize["result"]["capabilities"]["textDocumentSync"]["save"],
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
                    "text": "let broken"
                }
            }
        }),
    )
    .await;

    let open_publish = read_message(&mut reader).await;
    assert_eq!(open_publish["method"], "textDocument/publishDiagnostics");
    assert_eq!(open_publish["params"]["uri"], uri.as_str());
    assert_eq!(open_publish["params"]["version"], 1);
    assert_eq!(diagnostic_count(&open_publish), 1);
    assert!(backend.has_open_document(&uri));
    assert_eq!(backend.open_document_count(), 1);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "version": 2
                },
                "contentChanges": [
                    {
                        "text": "let answer = 1"
                    }
                ]
            }
        }),
    )
    .await;

    let valid_change_publish = read_message(&mut reader).await;
    assert_eq!(valid_change_publish["params"]["version"], 2);
    assert_eq!(diagnostic_count(&valid_change_publish), 0);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "version": 3
                },
                "contentChanges": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 11 },
                            "end": { "line": 0, "character": 12 }
                        },
                        "text": ""
                    }
                ]
            }
        }),
    )
    .await;

    let invalid_incremental_publish = read_message(&mut reader).await;
    assert_eq!(invalid_incremental_publish["params"]["version"], 3);
    assert_eq!(diagnostic_count(&invalid_incremental_publish), 1);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didSave",
            "params": {
                "textDocument": {
                    "uri": uri
                }
            }
        }),
    )
    .await;

    let save_publish = read_message(&mut reader).await;
    assert_eq!(save_publish["params"]["version"], 3);
    assert_eq!(diagnostic_count(&save_publish), 1);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": {
                "textDocument": {
                    "uri": uri
                }
            }
        }),
    )
    .await;

    let close_publish = read_message(&mut reader).await;
    assert_eq!(close_publish["params"]["uri"], uri.as_str());
    assert!(close_publish["params"]["version"].is_null());
    assert_eq!(diagnostic_count(&close_publish), 0);
    assert!(!backend.has_open_document(&uri));
    assert_eq!(backend.open_document_count(), 0);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown"
        }),
    )
    .await;

    let _shutdown_response = read_message(&mut reader).await;
    let _shutdown_log = read_message(&mut reader).await;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
    )
    .await;

    drop(writer);

    timeout(Duration::from_secs(2), server_task)
        .await
        .expect("server task timed out")
        .expect("server task failed");
}

fn diagnostic_count(message: &Value) -> usize {
    message["params"]["diagnostics"].as_array().unwrap().len()
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
