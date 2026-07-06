use std::path::Path;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lymals::server;

#[tokio::test(flavor = "current_thread")]
async fn definition_and_declaration_navigate_local_and_workspace_symbols() {
    let workspace = fixture_workspace_uri();
    let document_uri = fixture_document_uri("workspace/main.lyma");

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
                "capabilities": {},
                "workspaceFolders": [{
                    "uri": workspace,
                    "name": "navigation-fixture"
                }]
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["definitionProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["declarationProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["typeDefinitionProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["implementationProvider"],
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
        "@import \"./service.lyma\" as service\n",
        "@include \"./partials/base.lyma\"\n",
        "@use \"./modules/network.lyma\" as network\n",
        "let selected = ${shared.region}\n",
        "service_name: ${selected}\n",
        "replica_count: ${service.replicas}\n",
        "network_cidr: ${network.cidr}\n",
        "module_ref: ${network}\n"
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": document_uri,
                    "languageId": "lyma",
                    "version": 1,
                    "text": text
                }
            }
        }),
    )
    .await;
    let _diagnostics = read_message(&mut reader).await;

    let local_decl = goto(
        &mut writer,
        &mut reader,
        "textDocument/declaration",
        &document_uri,
        5,
        16,
        10,
    )
    .await;
    assert_eq!(
        extract_locations(&local_decl),
        vec![json!({
            "uri": document_uri,
            "range": {
                "start": { "line": 4, "character": 4 },
                "end": { "line": 4, "character": 12 }
            }
        })]
    );

    let workspace_def = goto(
        &mut writer,
        &mut reader,
        "textDocument/definition",
        &document_uri,
        6,
        27,
        11,
    )
    .await;
    assert_eq!(
        extract_locations(&workspace_def),
        vec![json!({
            "uri": fixture_document_uri("workspace/service.lyma"),
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 8 }
            }
        })]
    );

    let alias_decl = goto(
        &mut writer,
        &mut reader,
        "textDocument/declaration",
        &document_uri,
        8,
        16,
        12,
    )
    .await;
    assert_eq!(
        extract_locations(&alias_decl),
        vec![json!({
            "uri": document_uri,
            "range": {
                "start": { "line": 3, "character": 33 },
                "end": { "line": 3, "character": 40 }
            }
        })]
    );

    let path_def = goto(
        &mut writer,
        &mut reader,
        "textDocument/definition",
        &document_uri,
        2,
        12,
        13,
    )
    .await;
    assert_eq!(
        extract_locations(&path_def),
        vec![json!({
            "uri": fixture_document_uri("workspace/partials/base.lyma"),
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 11 }
            }
        })]
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn type_definition_and_implementation_follow_static_navigation_semantics() {
    let workspace = fixture_workspace_uri();
    let document_uri = fixture_document_uri("workspace/main.lyma");

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
                "capabilities": {},
                "workspaceFolders": [{
                    "uri": workspace,
                    "name": "navigation-fixture"
                }]
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["typeDefinitionProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["implementationProvider"],
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
        "@profile dev\n",
        "@schema Widget\n",
        "@import \"./service.lyma\" as service\n",
        "@use \"./modules/network.lyma\" as network\n",
        "component: @Widget\n",
        "profile_name: @dev\n",
        "tagged: !Widget \"card\"\n",
        "service_impl: ${service.replicas}\n",
        "network_impl: ${network}\n",
        "local_value: plain\n"
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": document_uri,
                    "languageId": "lyma",
                    "version": 1,
                    "text": text
                }
            }
        }),
    )
    .await;
    let _diagnostics = read_message(&mut reader).await;

    let metadata_type = goto(
        &mut writer,
        &mut reader,
        "textDocument/typeDefinition",
        &document_uri,
        4,
        13,
        20,
    )
    .await;
    assert_eq!(
        extract_locations(&metadata_type),
        vec![json!({
            "uri": document_uri,
            "range": {
                "start": { "line": 1, "character": 8 },
                "end": { "line": 1, "character": 14 }
            }
        })]
    );

    let profile_type = goto(
        &mut writer,
        &mut reader,
        "textDocument/typeDefinition",
        &document_uri,
        5,
        16,
        21,
    )
    .await;
    assert_eq!(
        extract_locations(&profile_type),
        vec![json!({
            "uri": document_uri,
            "range": {
                "start": { "line": 0, "character": 9 },
                "end": { "line": 0, "character": 12 }
            }
        })]
    );

    let type_empty = goto(
        &mut writer,
        &mut reader,
        "textDocument/typeDefinition",
        &document_uri,
        7,
        18,
        22,
    )
    .await;
    assert!(
        type_empty["result"].is_null(),
        "expected null typeDefinition result: {type_empty}"
    );

    let implementation = goto(
        &mut writer,
        &mut reader,
        "textDocument/implementation",
        &document_uri,
        7,
        18,
        23,
    )
    .await;
    assert_eq!(
        extract_locations(&implementation),
        vec![json!({
            "uri": fixture_document_uri("workspace/service.lyma"),
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 8 }
            }
        })]
    );

    let implementation_empty = goto(
        &mut writer,
        &mut reader,
        "textDocument/implementation",
        &document_uri,
        4,
        13,
        24,
    )
    .await;
    assert!(
        implementation_empty["result"].is_null(),
        "expected null implementation result: {implementation_empty}"
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
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

async fn goto(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    method: &str,
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
            "method": method,
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }),
    )
    .await;

    read_message(reader).await
}

fn extract_locations(response: &Value) -> Vec<Value> {
    match &response["result"] {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![response["result"].clone()],
        other => panic!("unexpected goto result: {other}"),
    }
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
