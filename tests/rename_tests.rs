use std::{collections::BTreeMap, path::Path};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::server;

#[tokio::test(flavor = "current_thread")]
async fn prepare_rename_and_workspace_edit_cover_lets_aliases_and_static_keys() {
    let workspace = fixture_workspace_uri();
    let main_uri = fixture_document_uri("workspace/main.luma");
    let shared_uri = fixture_document_uri("workspace/shared.luma");
    let (mut writer, mut reader, server_task) = start_server().await;

    let initialize = initialize(&mut writer, &mut reader, &workspace).await;
    assert_eq!(
        initialize["result"]["capabilities"]["renameProvider"]["prepareProvider"],
        true
    );
    initialized(&mut writer, &mut reader).await;

    open_doc(
        &mut writer,
        &mut reader,
        &main_uri,
        concat!(
            "@import \"./shared.luma\" as shared\n",
            "let selected = ${shared.region}\n",
            "service_name: ${selected}\n",
            "selected_copy: ${selected}\n",
            "region_name: ${shared.region}\n"
        ),
    )
    .await;
    open_doc(
        &mut writer,
        &mut reader,
        &shared_uri,
        concat!("region: us-east-1\n", "enabled: true\n"),
    )
    .await;

    let prepare_let = prepare_rename(&mut writer, &mut reader, &main_uri, 1, 4, 10).await;
    assert_eq!(
        prepare_let["result"],
        json!({
            "range": {
                "start": {"line": 1, "character": 4},
                "end": {"line": 1, "character": 12}
            },
            "placeholder": "selected"
        })
    );

    let rename_let = rename(&mut writer, &mut reader, &main_uri, 2, 16, "active", 11).await;
    assert_eq!(
        extract_edits(&rename_let),
        BTreeMap::from([(
            main_uri.to_string(),
            vec![
                json!({"range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 12}}, "newText": "active"}),
                json!({"range": {"start": {"line": 2, "character": 16}, "end": {"line": 2, "character": 24}}, "newText": "active"}),
                json!({"range": {"start": {"line": 3, "character": 17}, "end": {"line": 3, "character": 25}}, "newText": "active"}),
            ],
        )])
    );

    let prepare_alias = prepare_rename(&mut writer, &mut reader, &main_uri, 0, 27, 12).await;
    assert_eq!(prepare_alias["result"]["placeholder"], "shared");

    let rename_alias = rename(&mut writer, &mut reader, &main_uri, 4, 15, "catalog", 13).await;
    assert_eq!(
        extract_edits(&rename_alias),
        BTreeMap::from([(
            main_uri.to_string(),
            vec![
                json!({"range": {"start": {"line": 0, "character": 27}, "end": {"line": 0, "character": 33}}, "newText": "catalog"}),
                json!({"range": {"start": {"line": 1, "character": 17}, "end": {"line": 1, "character": 23}}, "newText": "catalog"}),
                json!({"range": {"start": {"line": 4, "character": 15}, "end": {"line": 4, "character": 21}}, "newText": "catalog"}),
            ],
        )])
    );

    let prepare_key = prepare_rename(&mut writer, &mut reader, &shared_uri, 0, 1, 14).await;
    assert_eq!(prepare_key["result"]["placeholder"], "region");

    let rename_key = rename(&mut writer, &mut reader, &shared_uri, 0, 1, "zone", 15).await;
    assert_eq!(
        extract_edits(&rename_key),
        BTreeMap::from([
            (
                main_uri.to_string(),
                vec![
                    json!({"range": {"start": {"line": 1, "character": 24}, "end": {"line": 1, "character": 30}}, "newText": "zone"}),
                    json!({"range": {"start": {"line": 4, "character": 22}, "end": {"line": 4, "character": 28}}, "newText": "zone"}),
                ],
            ),
            (
                shared_uri.to_string(),
                vec![
                    json!({"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}, "newText": "zone"})
                ],
            ),
        ])
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn rename_rejects_conflicts_and_unsupported_lua_ranges() {
    let workspace = fixture_workspace_uri();
    let main_uri = fixture_document_uri("workspace/main.luma");
    let (mut writer, mut reader, server_task) = start_server().await;

    let _initialize = initialize(&mut writer, &mut reader, &workspace).await;
    initialized(&mut writer, &mut reader).await;

    open_doc(
        &mut writer,
        &mut reader,
        &main_uri,
        concat!(
            "@import \"./shared.luma\" as shared\n",
            "let selected = ${shared.region + fn_call(unknown)}\n",
            "let alias = plain\n",
            "region_name: ${selected}\n"
        ),
    )
    .await;

    let unsupported_prepare = prepare_rename(&mut writer, &mut reader, &main_uri, 1, 33, 20).await;
    assert!(
        unsupported_prepare["result"].is_null(),
        "expected null prepareRename result: {unsupported_prepare}"
    );

    let unsupported_rename = rename(&mut writer, &mut reader, &main_uri, 1, 33, "safe", 21).await;
    assert_eq!(unsupported_rename["error"]["code"], -32602);

    let conflict_rename = rename(&mut writer, &mut reader, &main_uri, 1, 4, "alias", 22).await;
    assert_eq!(conflict_rename["error"]["code"], -32602);
    assert_eq!(
        conflict_rename["error"]["message"],
        "rename would conflict with an existing symbol"
    );

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
    workspace: &Url,
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
                "workspaceFolders": [{"uri": workspace, "name": "rename-fixture"}]
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
    uri: &Url,
    text: &str,
) {
    send_message(writer, &json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {"textDocument": {"uri": uri, "languageId": "luma", "version": 1, "text": text}}
    })).await;
    let _diagnostics = read_message(reader).await;
}

async fn prepare_rename(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    line: u32,
    character: u32,
    id: u64,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/prepareRename",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }
        }),
    )
    .await;
    read_message(reader).await
}

async fn rename(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    line: u32,
    character: u32,
    new_name: &str,
    id: u64,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/rename",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "newName": new_name
            }
        }),
    )
    .await;
    read_message(reader).await
}

fn fixture_workspace_uri() -> Url {
    Url::from_directory_path(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/completion/workspace"),
    )
    .unwrap()
}

fn fixture_document_uri(relative: &str) -> Url {
    Url::from_file_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/completion")
            .join(relative),
    )
    .unwrap()
}

fn extract_edits(response: &Value) -> BTreeMap<String, Vec<Value>> {
    response["result"]["changes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(uri, edits)| (uri.clone(), edits.as_array().cloned().unwrap_or_default()))
        .collect()
}

async fn shutdown_server(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    server_task: tokio::task::JoinHandle<()>,
) {
    send_message(
        writer,
        &json!({"jsonrpc": "2.0", "id": 99, "method": "shutdown"}),
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
