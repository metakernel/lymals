use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;

use lumals::server;

#[tokio::test(flavor = "current_thread")]
async fn execute_command_provider_registers_safe_commands() {
    let (mut writer, mut reader, server_task) = start_server().await;

    let workspace = tempdir().unwrap();
    let initialize = initialize(&mut writer, &mut reader, workspace.path()).await;
    let commands = initialize["result"]["capabilities"]["executeCommandProvider"]["commands"]
        .as_array()
        .unwrap();

    assert!(commands.iter().any(|value| value == "lumals.restartIndex"));
    assert!(commands.iter().any(|value| value == "lumals.showSyntaxTree"));
    assert!(commands.iter().any(|value| value == "lumals.showConfig"));
    assert!(commands.iter().any(|value| value == "lumals.formatWorkspaceFile"));
    assert!(commands.iter().any(|value| value == "lumals.explainDiagnostic"));

    initialized(&mut writer, &mut reader).await;
    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn execute_command_validates_arguments_and_returns_safe_errors() {
    let (mut writer, mut reader, server_task) = start_server().await;

    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    initialize(&mut writer, &mut reader, workspace.path()).await;
    initialized(&mut writer, &mut reader).await;

    let missing_uri = execute_command(&mut writer, &mut reader, 2, "lumals.showSyntaxTree", json!([])).await;
    assert_eq!(missing_uri["error"]["code"], -32602);
    assert_eq!(
        missing_uri["error"]["message"],
        "lumals.showSyntaxTree expects a single uri argument"
    );

    let outside_uri = tower_lsp::lsp_types::Url::from_file_path(outside.path().join("escape.luma"))
        .unwrap();
    fs::write(outside.path().join("escape.luma"), "root:  bad\n").unwrap();
    let blocked = execute_command(
        &mut writer,
        &mut reader,
        3,
        "lumals.formatWorkspaceFile",
        json!([{ "uri": outside_uri }]),
    )
    .await;
    assert_eq!(blocked["error"]["code"], -32602);
    assert_eq!(
        blocked["error"]["message"],
        "workspace file must stay within configured roots and end with .luma"
    );

    let unknown_code = execute_command(
        &mut writer,
        &mut reader,
        4,
        "lumals.explainDiagnostic",
        json!(["L999"]),
    )
    .await;
    assert_eq!(unknown_code["error"]["code"], -32602);
    assert_eq!(unknown_code["error"]["message"], "unknown diagnostic code");

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn execute_command_returns_parse_only_results() {
    let (mut writer, mut reader, server_task) = start_server().await;

    let workspace = tempdir().unwrap();
    let file_path = workspace.path().join("sample.luma");
    fs::write(&file_path, "root:\n   child:  one  \n").unwrap();
    let uri = tower_lsp::lsp_types::Url::from_file_path(&file_path).unwrap();

    initialize(&mut writer, &mut reader, workspace.path()).await;
    initialized(&mut writer, &mut reader).await;

    open_doc(&mut writer, &mut reader, &uri, "@schema ./schema.json\nroot:\n  child: one\n").await;

    let syntax = execute_command(
        &mut writer,
        &mut reader,
        2,
        "lumals.showSyntaxTree",
        json!([{ "uri": uri }]),
    )
    .await;
    let tree = syntax["result"]["tree"].as_str().unwrap();
    assert!(tree.contains("Directive @schema"), "{tree}");
    assert!(tree.contains("Mapping"), "{tree}");

    let explain = execute_command(
        &mut writer,
        &mut reader,
        3,
        "lumals.explainDiagnostic",
        json!([{ "code": "L003" }]),
    )
    .await;
    assert_eq!(explain["result"]["code"], "L003");
    assert_eq!(explain["result"]["parseOnly"], true);

    let formatted = execute_command(
        &mut writer,
        &mut reader,
        4,
        "lumals.formatWorkspaceFile",
        json!([{ "uri": uri }]),
    )
    .await;
    assert_eq!(formatted["result"]["parseOnly"], true);
    assert_eq!(formatted["result"]["changed"], true);
    assert_eq!(formatted["result"]["text"], "root:\n  child: one\n");

    let config = execute_command(&mut writer, &mut reader, 5, "lumals.showConfig", json!([])).await;
    assert_eq!(config["result"]["command"], "lumals.showConfig");
    assert_eq!(config["result"]["parseOnly"], true);

    let restart = execute_command(&mut writer, &mut reader, 6, "lumals.restartIndex", json!([])).await;
    assert_eq!(restart["result"]["command"], "lumals.restartIndex");
    assert_eq!(restart["result"]["parseOnly"], true);

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

async fn start_server() -> (
    tokio::io::DuplexStream,
    tokio::io::DuplexStream,
    tokio::task::JoinHandle<()>,
) {
    let (client_to_server, server_stdin) = tokio::io::duplex(16 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(16 * 1024);
    let (service, socket) = server::service();

    let server_task = tokio::spawn(async move {
        Server::new(server_stdin, server_stdout, socket)
            .serve(service)
            .await;
    });

    (client_to_server, server_to_client, server_task)
}

async fn initialize(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    workspace: &std::path::Path,
) -> Value {
    let root_uri = tower_lsp::lsp_types::Url::from_directory_path(workspace).unwrap();
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "capabilities": {},
                "workspaceFolders": [{
                    "uri": root_uri,
                    "name": "workspace"
                }]
            }
        }),
    )
    .await;
    read_message(reader).await
}

async fn initialized(writer: &mut tokio::io::DuplexStream, reader: &mut tokio::io::DuplexStream) {
    send_message(
        writer,
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    )
    .await;
    let _initialized_log = read_message(reader).await;
}

async fn open_doc(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &tower_lsp::lsp_types::Url,
    text: &str,
) {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "luma",
                    "version": 1,
                    "text": text
                }
            }
        }),
    )
    .await;
    let _diagnostics = read_message(reader).await;
}

async fn execute_command(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    id: i32,
    command: &str,
    arguments: Value,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "workspace/executeCommand",
            "params": {
                "command": command,
                "arguments": arguments
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
    send_message(writer, &json!({"jsonrpc": "2.0", "method": "exit"})).await;
    writer.shutdown().await.unwrap();
    timeout(Duration::from_secs(2), server_task)
        .await
        .unwrap()
        .unwrap();
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
