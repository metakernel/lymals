use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::server;

#[tokio::test(flavor = "current_thread")]
async fn hover_capabilities_and_markdown_are_stable() {
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
    let uri = Url::parse("file:///workspace/hover.luma").unwrap();

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "capabilities": {},
                "trace": "off"
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(initialize["result"]["capabilities"]["hoverProvider"], true);

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
        "@import \"./shared.luma\" as shared\n",
        "@profile dev\n",
        "let region = ${shared}\n",
        "service:\n",
        "  retries: 3\n",
        "  retries: 4\n",
        "  enabled: true\n"
    );

    send_message(
        &mut writer,
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
    let _diagnostics = read_message(&mut reader).await;

    let directive = hover_at(&mut writer, &mut reader, &uri, 0, 1, 10).await;
    assert_eq!(
        directive["result"]["range"],
        json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 7 }
        })
    );
    assert_eq!(
        directive_markdown(&directive),
        concat!(
            "**Directive:** `@import`\n\n",
            "Import a local module and optionally bind it to an alias.\n\n",
            "```luma\n@import \"./shared.luma\" as shared\n```\n\n",
            "Spec excerpt: Relative paths and `file:` URIs are allowed; non-`file:` schemes, absolute paths, and parent traversal are rejected by default in parse-only mode.\n\n",
            "**Static node type:** `directive`"
        )
    );

    let alias = hover_at(&mut writer, &mut reader, &uri, 2, 16, 11).await;
    assert_eq!(
        alias["result"]["range"],
        json!({
            "start": { "line": 2, "character": 15 },
            "end": { "line": 2, "character": 21 }
        })
    );
    assert_eq!(
        directive_markdown(&alias),
        concat!(
            "**Alias:** `shared`\n\n",
            "Imported/module alias.\n",
            "Source path: `./shared.luma`\n",
            "Path: `document[1].alias(shared)`\n\n",
            "Available inside parse-only Lua expressions without evaluation.\n\n",
            "**Static node type:** `alias`"
        )
    );

    let duplicate_key = hover_at(&mut writer, &mut reader, &uri, 5, 4, 12).await;
    assert_eq!(
        duplicate_key["result"]["range"],
        json!({
            "start": { "line": 5, "character": 2 },
            "end": { "line": 5, "character": 9 }
        })
    );
    assert_eq!(
        directive_markdown(&duplicate_key),
        concat!(
            "**Key:** `retries`\n\n",
            "Path: `document[1].service.retries`\n",
            "Static value type: `number`\n\n",
            "Spec excerpt: Mapping keys select child nodes using `key: value` syntax and indentation-delimited blocks.\n\n",
            "**Diagnostics**\n",
            "- `L002` duplicate mapping key 'retries'\n",
            "  - line 6 shadows the earlier 'retries' entry"
        )
    );

    let boolean_scalar = hover_at(&mut writer, &mut reader, &uri, 6, 13, 13).await;
    assert_eq!(
        boolean_scalar["result"]["range"],
        json!({
            "start": { "line": 6, "character": 11 },
            "end": { "line": 6, "character": 15 }
        })
    );
    assert_eq!(
        directive_markdown(&boolean_scalar),
        concat!(
            "**Scalar:** `boolean`\n\n",
            "Literal: `true`\n",
            "Static node type: `boolean`\n\n",
            "Spec excerpt: Plain scalars stay static in parse-only mode; hover never evaluates Lua to refine runtime values."
        )
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

fn directive_markdown(value: &Value) -> &str {
    value["result"]["contents"]["value"].as_str().unwrap()
}

async fn hover_at(
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
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
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
