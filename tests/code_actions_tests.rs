use std::{collections::BTreeMap, path::Path};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::server;

#[tokio::test(flavor = "current_thread")]
async fn code_actions_advertise_supported_kinds_and_return_workspace_edits() {
    let workspace = fixture_workspace_uri();
    let document_uri = fixture_document_uri("workspace/code-actions.luma");
    let (mut writer, mut reader, server_task) = start_server().await;

    let initialize = initialize(&mut writer, &mut reader, &workspace).await;
    assert_eq!(
        initialize["result"]["capabilities"]["codeActionProvider"]["resolveProvider"],
        false
    );
    assert_eq!(
        initialize["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
        json!(["quickfix", "source.organizeImports"])
    );
    initialized(&mut writer, &mut reader).await;

    let publish = open_doc(
        &mut writer,
        &mut reader,
        &document_uri,
        concat!(
            "@profil dev\n",
            "@import \"./b.luma\"\n",
            "@import \"./a.luma\"\n",
            "name:\n",
            "\tenabled: true\n",
            "service: one\n",
            "service: two\n"
        ),
    )
    .await;

    let response = code_actions(
        &mut writer,
        &mut reader,
        &document_uri,
        json!({"start": {"line": 0, "character": 0}, "end": {"line": 6, "character": 12}}),
        publish["params"]["diagnostics"].clone(),
        10,
    )
    .await;

    let titles = action_titles(&response);
    assert!(titles.contains(&"Replace tab with two spaces".to_string()));
    assert!(titles.contains(&"Normalize directive to `@profile`".to_string()));
    assert!(titles.contains(&"Remove duplicate key `service`".to_string()));
    assert!(titles.contains(&"Rename duplicate key to `service_2`".to_string()));
    assert!(titles.contains(&"Insert @luma 0.1".to_string()));
    assert!(titles.contains(&"Organize directives and imports".to_string()));

    let edits = action_edits(&response);
    assert_eq!(
        edits["Normalize directive to `@profile`"],
        vec![json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 7}},
            "newText": "@profile"
        })]
    );
    assert_eq!(
        edits["Replace tab with two spaces"],
        vec![json!({
            "range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 1}},
            "newText": "  "
        })]
    );
    assert_eq!(
        edits["Remove duplicate key `service`"],
        vec![json!({
            "range": {"start": {"line": 6, "character": 0}, "end": {"line": 7, "character": 0}},
            "newText": ""
        })]
    );
    assert_eq!(
        edits["Rename duplicate key to `service_2`"],
        vec![json!({
            "range": {"start": {"line": 6, "character": 0}, "end": {"line": 6, "character": 7}},
            "newText": "service_2"
        })]
    );
    assert_eq!(
        edits["Insert @luma 0.1"],
        vec![json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
            "newText": "@luma 0.1\n"
        })]
    );
    assert_eq!(
        edits["Organize directives and imports"],
        vec![json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 3, "character": 0}},
            "newText": "@import \"./a.luma\"\n@import \"./b.luma\"\n@profil dev\n"
        })]
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn selection_sensitive_code_actions_offer_quote_and_null_fixes() {
    let workspace = fixture_workspace_uri();
    let document_uri = fixture_document_uri("workspace/code-actions-selection.luma");
    let (mut writer, mut reader, server_task) = start_server().await;

    let _initialize = initialize(&mut writer, &mut reader, &workspace).await;
    initialized(&mut writer, &mut reader).await;

    let _publish = open_doc(
        &mut writer,
        &mut reader,
        &document_uri,
        concat!("feature: true\n", "empty:\n"),
    )
    .await;

    let quote = code_actions(
        &mut writer,
        &mut reader,
        &document_uri,
        json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 13}}),
        json!([]),
        11,
    )
    .await;
    let quote_edits = action_edits(&quote);
    assert_eq!(
        quote_edits["Quote ambiguous scalar `true`"],
        vec![json!({
            "range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 13}},
            "newText": "\"true\""
        })]
    );

    let null_fix = code_actions(
        &mut writer,
        &mut reader,
        &document_uri,
        json!({"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 6}}),
        json!([]),
        12,
    )
    .await;
    let null_edits = action_edits(&null_fix);
    assert_eq!(
        null_edits["Convert empty value for `empty` to null"],
        vec![json!({
            "range": {"start": {"line": 1, "character": 6}, "end": {"line": 1, "character": 6}},
            "newText": " null"
        })]
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
                "workspaceFolders": [{"uri": workspace, "name": "code-actions-fixture"}]
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
) -> Value {
    send_message(writer, &json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {"textDocument": {"uri": uri, "languageId": "luma", "version": 1, "text": text}}
    })).await;
    read_message(reader).await
}

async fn code_actions(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    range: Value,
    diagnostics: Value,
    id: u64,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": {"uri": uri},
                "range": range,
                "context": {"diagnostics": diagnostics}
            }
        }),
    )
    .await;
    read_message(reader).await
}

fn action_titles(response: &Value) -> Vec<String> {
    response["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|action| action["title"].as_str().map(ToOwned::to_owned))
        .collect()
}

fn action_edits(response: &Value) -> BTreeMap<String, Vec<Value>> {
    response["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| {
            (
                action["title"].as_str().unwrap().to_string(),
                action["edit"]["changes"]
                    .as_object()
                    .unwrap()
                    .values()
                    .next()
                    .unwrap()
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            )
        })
        .collect()
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
