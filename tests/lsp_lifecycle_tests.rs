use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::Command;

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;

use lymals::server;
use lymals::state::LifecyclePhase;

#[tokio::test(flavor = "current_thread")]
async fn json_rpc_lifecycle_initializes_and_shuts_down() {
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

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "clientInfo": { "name": "lymals-test", "version": "0" },
                "capabilities": {
                    "general": {
                        "positionEncodings": ["utf-16", "utf-8"]
                    },
                    "workspace": {
                        "workspaceFolders": true
                    }
                },
                "trace": "verbose"
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "lymals");
    assert_eq!(
        initialize["result"]["serverInfo"]["version"],
        lymals::VERSION
    );
    assert_eq!(
        initialize["result"]["capabilities"]["positionEncoding"],
        "utf-16"
    );
    assert_eq!(
        initialize["result"]["capabilities"]["workspace"]["workspaceFolders"]["supported"],
        true
    );

    let snapshot = backend.state_snapshot();
    assert_eq!(snapshot.trace, tower_lsp::lsp_types::TraceValue::Verbose);
    assert!(
        snapshot
            .client_capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.workspace_folders)
            .unwrap_or(false)
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

    let notifications = read_notifications(&mut reader, 2).await;
    assert_eq!(backend.state_snapshot().phase, LifecyclePhase::Initialized);
    assert_eq!(
        notifications["window/logMessage"]["params"]["message"],
        "lymals initialized"
    );
    assert_eq!(
        notifications["$/logTrace"]["params"]["message"],
        "server initialized"
    );
    assert_eq!(
        notifications["$/logTrace"]["params"]["verbose"],
        "lifecycle=initialized capabilities=minimal"
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown"
        }),
    )
    .await;

    let shutdown_messages = read_messages(&mut reader, 3).await;
    let shutdown_response = shutdown_messages
        .iter()
        .find(|message| message["id"] == 2)
        .cloned()
        .expect("missing shutdown response");
    let shutdown_notification = shutdown_messages
        .iter()
        .find(|message| message["method"] == "window/logMessage")
        .cloned()
        .expect("missing shutdown log message");

    assert_eq!(shutdown_response["id"], 2);
    assert!(shutdown_response["result"].is_null());
    assert_eq!(backend.state_snapshot().phase, LifecyclePhase::Shutdown);
    assert_eq!(shutdown_notification["method"], "window/logMessage");
    assert_eq!(
        shutdown_notification["params"]["message"],
        "lymals shutting down"
    );

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

#[tokio::test(flavor = "current_thread")]
async fn binary_stdio_starts_real_lsp_server_without_stdout_noise() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lymals"))
        .arg("--stdio")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn lymals binary");

    let mut writer = child.stdin.take().expect("missing stdin");
    let mut reader = child.stdout.take().expect("missing stdout");

    send_message_blocking(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "clientInfo": { "name": "lymals-test", "version": "0" },
                "capabilities": {
                    "general": {
                        "positionEncodings": ["utf-8"]
                    }
                }
            }
        }),
    );

    let initialize = read_message_blocking(&mut reader);
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "lymals");
    assert_eq!(
        initialize["result"]["capabilities"]["positionEncoding"],
        "utf-16"
    );

    send_message_blocking(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );

    let initialized_log = read_message_blocking(&mut reader);
    assert_eq!(initialized_log["method"], "window/logMessage");
    assert_eq!(initialized_log["params"]["message"], "lymals initialized");

    send_message_blocking(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown"
        }),
    );

    let shutdown_messages = [
        read_message_blocking(&mut reader),
        read_message_blocking(&mut reader),
    ];
    let shutdown_response = shutdown_messages
        .iter()
        .find(|message| message["id"] == 2)
        .expect("missing shutdown response");
    let shutdown_log = shutdown_messages
        .iter()
        .find(|message| message["method"] == "window/logMessage")
        .expect("missing shutdown log");
    assert_eq!(shutdown_response["id"], 2);
    assert!(shutdown_response["result"].is_null());
    assert_eq!(shutdown_log["method"], "window/logMessage");

    send_message_blocking(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
    );

    drop(writer);

    let status = child.wait().expect("failed to wait for lymals child");
    assert!(status.success(), "lymals child failed: {status}");
}

fn send_message_blocking<W>(stream: &mut W, value: &Value)
where
    W: Write,
{
    let body = value.to_string();
    let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stream.write_all(message.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn read_message_blocking<R>(stream: &mut R) -> Value
where
    R: Read,
{
    let mut headers = Vec::new();
    let mut byte = [0u8; 1];

    loop {
        stream.read_exact(&mut byte).unwrap();
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
    stream.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn send_message<W>(stream: &mut W, value: &Value)
where
    W: AsyncWrite + Unpin,
{
    let body = value.to_string();
    let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stream.write_all(message.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
}

async fn read_message<R>(stream: &mut R) -> Value
where
    R: AsyncRead + Unpin,
{
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

async fn read_notifications(
    stream: &mut (impl AsyncRead + Unpin),
    count: usize,
) -> BTreeMap<String, Value> {
    let mut notifications = BTreeMap::new();
    for _ in 0..count {
        let message = read_message(stream).await;
        let method = message["method"].as_str().unwrap().to_string();
        notifications.insert(method, message);
    }
    notifications
}

async fn read_messages(stream: &mut (impl AsyncRead + Unpin), count: usize) -> Vec<Value> {
    let mut messages = Vec::with_capacity(count);
    for _ in 0..count {
        messages.push(read_message(stream).await);
    }
    messages
}
