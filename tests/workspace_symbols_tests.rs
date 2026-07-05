use std::{fs, path::Path};

use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::server;

#[tokio::test(flavor = "current_thread")]
async fn workspace_symbols_cover_workspace_files_open_docs_and_kind_path_queries() {
    let workspace_one = TempDir::new().unwrap();
    let workspace_two = TempDir::new().unwrap();
    let excluded = TempDir::new().unwrap();

    write_workspace_file(
        workspace_one.path(),
        "main.luma",
        concat!(
            "@schema Service\n",
            "@import \"./shared.luma\" as shared\n",
            "let selected = ${shared.region}\n",
            "service: ${selected}\n",
            "!Service \"api\"\n"
        ),
    );
    write_workspace_file(workspace_one.path(), "shared.luma", "region: us-east-1\n");
    write_workspace_file(
        workspace_two.path(),
        "paths.luma",
        "@include \"./partials/base.luma\"\n",
    );
    write_workspace_file(workspace_two.path(), "partials/base.luma", "title: base\n");
    write_workspace_file(excluded.path(), "outside.luma", "outside_only: true\n");

    let (mut writer, mut reader, server_task) = spawn_server().await;
    let initialize = initialize(
        &mut writer,
        &mut reader,
        &[
            workspace_uri(workspace_one.path()),
            workspace_uri(workspace_two.path()),
        ],
    )
    .await;
    assert_eq!(
        initialize["result"]["capabilities"]["workspaceSymbolProvider"],
        true
    );

    let open_uri = file_uri(workspace_one.path(), "scratch.luma");
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": open_uri,
                    "languageId": "luma",
                    "version": 1,
                    "text": "open_only: yes\n"
                }
            }
        }),
    )
    .await;
    let _diagnostics = read_message(&mut reader).await;

    let schema = workspace_symbol(&mut writer, &mut reader, 10, "schema").await;
    assert_has_symbol(&schema, "Service", "main.luma");

    let fuzzy = workspace_symbol(&mut writer, &mut reader, 11, "slctd").await;
    assert_has_symbol(&fuzzy, "selected", "main.luma");

    let path = workspace_symbol(&mut writer, &mut reader, 12, "base.luma").await;
    assert_has_symbol(&path, "./partials/base.luma", "paths.luma");

    let tag = workspace_symbol(&mut writer, &mut reader, 13, "tag").await;
    assert_has_symbol(&tag, "!Service", "main.luma");

    let open = workspace_symbol(&mut writer, &mut reader, 14, "open_only").await;
    assert_has_symbol(&open, "open_only", "scratch.luma");

    let filtered = workspace_symbol(&mut writer, &mut reader, 15, "outside_only").await;
    assert!(
        filtered["result"].as_array().unwrap().is_empty(),
        "excluded workspace leaked into results: {filtered}"
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_symbols_drop_deleted_files_after_watched_invalidation() {
    let workspace = TempDir::new().unwrap();
    write_workspace_file(workspace.path(), "stale.luma", "stale_key: true\n");

    let stale_uri = file_uri(workspace.path(), "stale.luma");
    let (mut writer, mut reader, server_task) = spawn_server().await;
    let _initialize =
        initialize(&mut writer, &mut reader, &[workspace_uri(workspace.path())]).await;

    let before = workspace_symbol(&mut writer, &mut reader, 20, "stale_key").await;
    assert_has_symbol(&before, "stale_key", "stale.luma");

    fs::remove_file(workspace.path().join("stale.luma")).unwrap();
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [{
                    "uri": stale_uri,
                    "type": 3
                }]
            }
        }),
    )
    .await;

    let after = workspace_symbol(&mut writer, &mut reader, 21, "stale_key").await;
    assert!(
        after["result"].as_array().unwrap().is_empty(),
        "deleted file remained in workspace symbol results: {after}"
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

async fn spawn_server() -> (
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
    workspace_folders: &[Url],
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "capabilities": {},
                "workspaceFolders": workspace_folders
                    .iter()
                    .enumerate()
                    .map(|(index, uri)| json!({ "uri": uri, "name": format!("workspace-{index}") }))
                    .collect::<Vec<_>>()
            }
        }),
    )
    .await;

    let initialize = read_message(reader).await;

    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )
    .await;

    let _initialized_log = read_message(reader).await;
    initialize
}

async fn workspace_symbol(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    id: u64,
    query: &str,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "workspace/symbol",
            "params": { "query": query }
        }),
    )
    .await;
    read_message(reader).await
}

fn assert_has_symbol(response: &Value, expected_name: &str, expected_path_suffix: &str) {
    let items = response["result"].as_array().unwrap();
    assert!(
        items.iter().any(|item| {
            item["name"] == expected_name
                && item["location"]["uri"].as_str().is_some_and(|uri| {
                    uri.ends_with(expected_path_suffix.replace('\\', "/").as_str())
                })
        }),
        "missing symbol `{expected_name}` in {response}"
    );
}

fn write_workspace_file(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

fn workspace_uri(path: &Path) -> Url {
    Url::from_directory_path(path).unwrap()
}

fn file_uri(root: &Path, relative: &str) -> Url {
    Url::from_file_path(root.join(relative)).unwrap()
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
