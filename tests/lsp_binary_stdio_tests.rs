use std::process::Stdio;

use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{Duration, sleep, timeout};

#[tokio::test(flavor = "current_thread")]
async fn real_binary_stdio_lifecycle_handles_lsp_logs_without_protocol_noise() {
    let temp = tempdir().expect("failed to create tempdir");
    let log_path = temp.path().join("lymals.log");

    let mut child = Command::new(env!("CARGO_BIN_EXE_lymals"))
        .arg("--stdio")
        .arg("--log-file")
        .arg(&log_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn lymals binary");

    let mut stdin = child.stdin.take().expect("missing child stdin");
    let mut stdout = child.stdout.take().expect("missing child stdout");
    let stderr = child.stderr.take().expect("missing child stderr");

    let stderr_task = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        stderr
            .read_to_end(&mut bytes)
            .await
            .expect("failed to read child stderr");
        String::from_utf8(bytes).expect("child stderr was not utf-8")
    });

    send_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "clientInfo": { "name": "lymals-binary-test", "version": "0" },
                "capabilities": {
                    "general": {
                        "positionEncodings": ["utf-8"]
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

    let initialize = timeout(Duration::from_secs(5), read_message(&mut stdout))
        .await
        .expect("timed out waiting for initialize response");
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "lymals");
    assert_eq!(initialize["result"]["capabilities"]["positionEncoding"], "utf-16");
    assert_eq!(
        initialize["result"]["capabilities"]["workspace"]["workspaceFolders"]["supported"],
        true
    );

    send_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )
    .await;

    let initialized_messages = timeout(Duration::from_secs(5), read_messages(&mut stdout, 2))
        .await
        .expect("timed out waiting for initialized notifications");
    let initialized_log = find_message(&initialized_messages, "window/logMessage");
    let initialized_trace = find_message(&initialized_messages, "$/logTrace");
    assert_eq!(initialized_log["params"]["message"], "lymals initialized");
    assert_eq!(initialized_trace["params"]["message"], "server initialized");
    assert_eq!(
        initialized_trace["params"]["verbose"],
        "lifecycle=initialized capabilities=minimal"
    );

    send_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown"
        }),
    )
    .await;

    let shutdown_messages = timeout(Duration::from_secs(5), read_messages(&mut stdout, 3))
        .await
        .expect("timed out waiting for shutdown response");
    let shutdown_response = shutdown_messages
        .iter()
        .find(|message| message["id"] == 2)
        .expect("missing shutdown response");
    let shutdown_log = find_message(&shutdown_messages, "window/logMessage");
    let shutdown_trace = find_message(&shutdown_messages, "$/logTrace");
    assert!(shutdown_response["result"].is_null());
    assert_eq!(shutdown_log["params"]["message"], "lymals shutting down");
    assert_eq!(shutdown_trace["params"]["message"], "server shutting down");
    assert_eq!(shutdown_trace["params"]["verbose"], "lifecycle=shutdown");

    send_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
    )
    .await;

    drop(stdin);

    let status = wait_for_child_exit(&mut child).await;
    let stderr = timeout(Duration::from_secs(5), stderr_task)
        .await
        .expect("timed out waiting for stderr reader")
        .expect("stderr reader task failed");

    assert!(status.success(), "lymals child failed: {status}");
    assert!(stderr.is_empty(), "unexpected stderr output: {stderr}");
}

fn find_message<'a>(messages: &'a [Value], method: &str) -> &'a Value {
    messages
        .iter()
        .find(|message| message["method"] == method)
        .unwrap_or_else(|| panic!("missing {method} message in {messages:?}"))
}

async fn wait_for_child_exit(child: &mut tokio::process::Child) -> std::process::ExitStatus {
    for _ in 0..40 {
        if let Some(status) = child.try_wait().expect("failed to poll lymals child") {
            return status;
        }
        sleep(Duration::from_millis(50)).await;
    }

    panic!("lymals child did not exit within 2 seconds after exit notification");
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

async fn read_messages(stream: &mut (impl AsyncRead + Unpin), count: usize) -> Vec<Value> {
    let mut messages = Vec::with_capacity(count);
    for _ in 0..count {
        messages.push(read_message(stream).await);
    }
    messages
}
