use std::{fs, path::Path};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, InsertTextFormat, Position, Url,
    WorkspaceFolder,
};

use lumals::{
    completion::{CompletionRequest, complete},
    config::LumalsConfig,
    server,
    syntax::FileId,
};

#[tokio::test(flavor = "current_thread")]
async fn completion_capabilities_and_contextual_items_are_advertised() {
    let workspace = fixture_workspace_uri();
    let document_uri = fixture_document_uri("workspace/main.luma");

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
                    "uri": workspace,
                    "name": "completion-fixture"
                }]
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["completionProvider"]["resolveProvider"],
        false
    );
    assert_eq!(
        initialize["result"]["capabilities"]["completionProvider"]["triggerCharacters"][0],
        "@"
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

    let text = concat!(
        "@im\n",
        "@profile d\n",
        "@import \"./\"\n",
        "@use \"./modules/network.luma\" as ne\n",
        "let region = ${re}\n",
        "let enabled = n\n",
        "rep\n"
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": document_uri,
                    "languageId": "luma",
                    "version": 1,
                    "text": text
                }
            }
        }),
    )
    .await;

    let _diagnostics = read_message(&mut reader).await;

    let directive_items = completion_items(&mut writer, &mut reader, &document_uri, 0, 3, 10).await;
    let import_directive = find_item(&directive_items, "@import");
    assert_eq!(import_directive.kind, Some(CompletionItemKind::KEYWORD));
    assert_eq!(
        import_directive.insert_text_format,
        Some(InsertTextFormat::SNIPPET)
    );
    assert_eq!(
        import_directive.insert_text.as_deref(),
        Some("@import \"${1:./shared.luma}\" as ${2:shared}")
    );
    assert_eq!(
        import_directive.commit_characters.as_ref().unwrap(),
        &[" ".to_string()]
    );

    let profile_items = completion_items(&mut writer, &mut reader, &document_uri, 1, 10, 11).await;
    let dev = find_item(&profile_items, "dev");
    assert_eq!(dev.kind, Some(CompletionItemKind::VALUE));

    let path_items = completion_items(&mut writer, &mut reader, &document_uri, 2, 12, 12).await;
    let shared = find_item(&path_items, "./shared.luma");
    assert_eq!(shared.kind, Some(CompletionItemKind::FILE));
    assert_eq!(shared.insert_text.as_deref(), Some("shared.luma"));
    assert_eq!(
        shared.commit_characters.as_ref().unwrap(),
        &["\"".to_string(), "'".to_string(), " ".to_string()]
    );
    assert!(path_items.iter().all(|item| item.label.ends_with(".luma")));
    assert!(path_items.iter().all(|item| !item.label.contains("..")));
    assert!(
        path_items
            .iter()
            .all(|item| !item.label.contains("README.md"))
    );

    let alias_items = completion_items(&mut writer, &mut reader, &document_uri, 3, 35, 13).await;
    let net = find_item(&alias_items, "network");
    assert_eq!(net.kind, Some(CompletionItemKind::MODULE));

    let lua_items = completion_items(&mut writer, &mut reader, &document_uri, 4, 15, 14).await;
    assert_eq!(
        find_item(&lua_items, "region").kind,
        Some(CompletionItemKind::VARIABLE)
    );
    assert_eq!(
        find_item(&lua_items, "ne").kind,
        Some(CompletionItemKind::MODULE)
    );

    let scalar_items = completion_items(&mut writer, &mut reader, &document_uri, 5, 16, 15).await;
    assert_eq!(
        find_item(&scalar_items, "nil").kind,
        Some(CompletionItemKind::VALUE)
    );
    assert_eq!(
        find_item(&scalar_items, "null").kind,
        Some(CompletionItemKind::VALUE)
    );

    let key_items = completion_items(&mut writer, &mut reader, &document_uri, 6, 3, 16).await;
    let replicas = find_item(&key_items, "replicas");
    assert_eq!(replicas.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(replicas.insert_text.as_deref(), Some("licas: "));
    assert_eq!(
        replicas.commit_characters.as_ref().unwrap(),
        &[":".to_string()]
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[test]
fn import_path_completion_stays_within_safe_local_boundaries() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    fs::create_dir_all(root.join("nested/child")).unwrap();
    fs::create_dir_all(root.join("shared")).unwrap();
    fs::write(root.join("nested/main.luma"), "@import \"./\"").unwrap();
    fs::write(root.join("nested/child/inside.luma"), "inside: true\n").unwrap();
    fs::write(root.join("shared/outside_base_dir.luma"), "outside: true\n").unwrap();
    fs::write(root.join("nested/README.md"), "not a luma file\n").unwrap();

    let uri = Url::from_file_path(root.join("nested/main.luma")).unwrap();
    let response = complete(CompletionRequest {
        uri: &uri,
        text: "@import \"./\"",
        file_id: FileId(0),
        position: Position::new(0, 12),
        workspace_folders: &[WorkspaceFolder {
            uri: Url::from_directory_path(root).unwrap(),
            name: "workspace".to_string(),
        }],
        config: &LumalsConfig::default(),
    })
    .expect("expected path completions");

    let CompletionResponse::Array(items) = response else {
        panic!("unexpected completion response")
    };
    let labels = items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"./child/inside.luma"), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label.contains("..")),
        "{labels:?}"
    );
    assert!(
        !labels.contains(&"./shared/outside_base_dir.luma"),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.ends_with("README.md")),
        "{labels:?}"
    );
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

async fn completion_items(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    line: u32,
    character: u32,
    id: u64,
) -> Vec<CompletionItem> {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }),
    )
    .await;

    let response = read_message(reader).await;
    let result = response["result"].clone();
    let array = if result.is_array() {
        result
    } else {
        result["items"].clone()
    };

    serde_json::from_value(array).unwrap()
}

fn find_item<'a>(items: &'a [CompletionItem], label: &str) -> &'a CompletionItem {
    items
        .iter()
        .find(|item| item.label == label)
        .unwrap_or_else(|| {
            panic!(
                "missing completion item '{label}', available: {:?}",
                items
                    .iter()
                    .map(|item| item.label.as_str())
                    .collect::<Vec<_>>()
            )
        })
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
