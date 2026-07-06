use std::path::Path;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lymals::server;

#[tokio::test(flavor = "current_thread")]
async fn references_include_declaration_for_local_lets_and_aliases() {
    let workspace = fixture_workspace_uri();
    let document_uri = fixture_document_uri("workspace/main.lyma");
    let (mut writer, mut reader, server_task) = start_server().await;

    let initialize = initialize(&mut writer, &mut reader, &workspace).await;
    assert!(
        initialize["result"]["capabilities"]["referencesProvider"].is_object()
            || initialize["result"]["capabilities"]["referencesProvider"] == json!(true)
    );
    initialized(&mut writer, &mut reader).await;

    let text = concat!(
        "@import \"./shared.lyma\" as shared\n",
        "@use \"./modules/network.lyma\" as network\n",
        "let selected = ${shared.region}\n",
        "service_name: ${selected}\n",
        "network_ref: ${network}\n",
        "network_cidr: ${network.cidr}\n"
    );
    open_doc(&mut writer, &mut reader, &document_uri, text).await;

    let with_decl = references(&mut writer, &mut reader, &document_uri, 2, 4, true, 10).await;
    assert_eq!(
        extract_locations(&with_decl),
        vec![
            json!({"uri": document_uri, "range": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 12}}}),
            json!({"uri": document_uri, "range": {"start": {"line": 3, "character": 16}, "end": {"line": 3, "character": 24}}}),
        ]
    );

    let without_decl = references(&mut writer, &mut reader, &document_uri, 2, 4, false, 11).await;
    assert_eq!(
        extract_locations(&without_decl),
        vec![
            json!({"uri": document_uri, "range": {"start": {"line": 3, "character": 16}, "end": {"line": 3, "character": 24}}}),
        ]
    );

    let alias_refs = references(&mut writer, &mut reader, &document_uri, 4, 16, false, 12).await;
    assert_eq!(
        extract_locations(&alias_refs),
        vec![
            json!({"uri": document_uri, "range": {"start": {"line": 4, "character": 15}, "end": {"line": 4, "character": 22}}}),
            json!({"uri": document_uri, "range": {"start": {"line": 5, "character": 16}, "end": {"line": 5, "character": 23}}}),
        ]
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn references_find_multi_file_imports_schemas_tags_and_static_keys() {
    let workspace = fixture_workspace_uri();
    let main_uri = fixture_document_uri("workspace/main.lyma");
    let schema_uri = fixture_document_uri("workspace/schema_refs.lyma");
    let shared_uri = fixture_document_uri("workspace/shared.lyma");
    let (mut writer, mut reader, server_task) = start_server().await;
    let init = initialize(&mut writer, &mut reader, &workspace).await;
    assert!(
        init["result"]["capabilities"]["referencesProvider"].is_object()
            || init["result"]["capabilities"]["referencesProvider"] == json!(true)
    );
    initialized(&mut writer, &mut reader).await;

    open_doc(
        &mut writer,
        &mut reader,
        &main_uri,
        concat!(
            "@import \"./shared.lyma\" as shared\n",
            "@import \"./service.lyma\" as service\n",
            "region_name: ${shared.region}\n",
            "replicas: ${service.replicas}\n"
        ),
    )
    .await;

    open_doc(
        &mut writer,
        &mut reader,
        &schema_uri,
        concat!(
            "@schema Widget\n",
            "component: @Widget\n",
            "tagged: !Widget \"x\"\n",
            "@import \"./shared.lyma\" as shared\n"
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

    let import_refs = references(&mut writer, &mut reader, &main_uri, 0, 12, true, 20).await;
    assert_eq!(
        extract_locations(&import_refs),
        vec![
            json!({"uri": main_uri, "range": {"start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 23}}}),
            json!({"uri": schema_uri, "range": {"start": {"line": 3, "character": 8}, "end": {"line": 3, "character": 23}}}),
        ]
    );

    let key_refs = references(&mut writer, &mut reader, &shared_uri, 0, 1, true, 21).await;
    assert_eq!(
        extract_locations(&key_refs),
        vec![
            json!({"uri": main_uri, "range": {"start": {"line": 2, "character": 22}, "end": {"line": 2, "character": 28}}}),
            json!({"uri": shared_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}}}),
        ]
    );

    let schema_refs = references(&mut writer, &mut reader, &schema_uri, 0, 9, false, 22).await;
    assert_eq!(
        extract_locations(&schema_refs),
        vec![
            json!({"uri": schema_uri, "range": {"start": {"line": 1, "character": 12}, "end": {"line": 1, "character": 18}}}),
            json!({"uri": schema_uri, "range": {"start": {"line": 2, "character": 9}, "end": {"line": 2, "character": 15}}}),
        ]
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
                "workspaceFolders": [{"uri": workspace, "name": "references-fixture"}]
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
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {"uri": uri, "languageId": "lyma", "version": 1, "text": text}
            }
        }),
    )
    .await;
    let _diagnostics = read_message(reader).await;
}

async fn references(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    line: u32,
    character: u32,
    include_declaration: bool,
    id: u64,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/references",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "context": {"includeDeclaration": include_declaration}
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

fn extract_locations(response: &Value) -> Vec<Value> {
    response["result"].as_array().cloned().unwrap_or_default()
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
