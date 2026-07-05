use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::server;

#[tokio::test(flavor = "current_thread")]
async fn selection_ranges_return_nested_parent_chains() {
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
    let uri = Url::parse("file:///workspace/selection-range-any-name-ending.luma").unwrap();

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": "file:///workspace",
                "capabilities": {}
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["selectionRangeProvider"],
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
        "---\n",
        "# heading comment\n",
        "@meta owner=team\n",
        "@profile dev\n",
        "service:\n",
        "  script: ${shared}\n",
        "  description: |\n",
        "    line one\n",
        "    line two\n",
        "  config:\n",
        "    env: prod\n",
        "...\n",
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

    let positions = [
        position_of(text, "env"),
        position_of(text, "prod"),
        position_of(text, "heading comment"),
        position_of(text, "line two"),
        position_of(text, "${shared}"),
    ];
    let response = selection_ranges(&mut writer, &mut reader, &uri, &positions, 2).await;
    let result = response["result"].as_array().unwrap();

    assert_eq!(
        chain_ranges(&result[0]),
        vec![
            range_json(text, span_of(text, "env")),
            range_json(text, span_of(text, "    env: prod")),
            range_json(text, span_of(text, "  config:\n    env: prod")),
            range_json(
                text,
                span_of(
                    text,
                    "service:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod"
                )
            ),
            range_json(
                text,
                span_of(
                    text,
                    "# heading comment\n@meta owner=team\n@profile dev\nservice:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod\n..."
                )
            ),
            range_json(text, span_of(text, text)),
        ]
    );

    assert_eq!(
        chain_ranges(&result[1]),
        vec![
            range_json(text, span_of(text, "prod")),
            range_json(text, span_of(text, "    env: prod")),
            range_json(text, span_of(text, "  config:\n    env: prod")),
            range_json(
                text,
                span_of(
                    text,
                    "service:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod"
                )
            ),
            range_json(
                text,
                span_of(
                    text,
                    "# heading comment\n@meta owner=team\n@profile dev\nservice:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod\n..."
                )
            ),
            range_json(text, span_of(text, text)),
        ]
    );

    assert_eq!(
        chain_ranges(&result[2]),
        vec![
            range_json(text, span_of(text, "# heading comment")),
            range_json(
                text,
                span_of(text, "# heading comment\n@meta owner=team\n@profile dev")
            ),
            range_json(
                text,
                span_of(
                    text,
                    "# heading comment\n@meta owner=team\n@profile dev\nservice:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod\n..."
                )
            ),
            range_json(text, span_of(text, text)),
        ]
    );

    assert_eq!(
        chain_ranges(&result[3]),
        vec![
            range_json(text, span_of_nth(text, "line", 2)),
            range_json(
                text,
                span_of(text, "description: |\n    line one\n    line two")
            ),
            range_json(
                text,
                span_of(text, "  description: |\n    line one\n    line two")
            ),
            range_json(
                text,
                span_of(
                    text,
                    "service:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod"
                )
            ),
            range_json(
                text,
                span_of(
                    text,
                    "# heading comment\n@meta owner=team\n@profile dev\nservice:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod\n..."
                )
            ),
            range_json(text, span_of(text, text)),
        ]
    );

    assert_eq!(
        chain_ranges(&result[4]),
        vec![
            range_json(text, span_of(text, "${shared}")),
            range_json(text, span_of(text, "  script: ${shared}")),
            range_json(
                text,
                span_of(
                    text,
                    "service:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod"
                )
            ),
            range_json(
                text,
                span_of(
                    text,
                    "# heading comment\n@meta owner=team\n@profile dev\nservice:\n  script: ${shared}\n  description: |\n    line one\n    line two\n  config:\n    env: prod\n..."
                )
            ),
            range_json(text, span_of(text, text)),
        ]
    );

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

async fn selection_ranges(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    positions: &[(u32, u32)],
    id: u64,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/selectionRange",
            "params": {
                "textDocument": { "uri": uri },
                "positions": positions.iter().map(|(line, character)| json!({
                    "line": line,
                    "character": character,
                })).collect::<Vec<_>>()
            }
        }),
    )
    .await;

    read_message(reader).await
}

fn chain_ranges(value: &Value) -> Vec<Value> {
    let mut chain = Vec::new();
    let mut current = Some(value);
    while let Some(item) = current {
        chain.push(item["range"].clone());
        current = item.get("parent");
    }
    chain
}

fn span_of(text: &str, needle: &str) -> (usize, usize) {
    let start = text.find(needle).unwrap();
    (start, start + needle.len())
}

fn span_of_nth(text: &str, needle: &str, occurrence: usize) -> (usize, usize) {
    let mut search_from = 0usize;
    for index in 0..occurrence {
        let offset = text[search_from..].find(needle).unwrap();
        let start = search_from + offset;
        if index + 1 == occurrence {
            return (start, start + needle.len());
        }
        search_from = start + needle.len();
    }
    unreachable!()
}

fn position_of(text: &str, needle: &str) -> (u32, u32) {
    let (start, _) = span_of(text, needle);
    offset_to_position(text, start)
}

fn range_json(text: &str, span: (usize, usize)) -> Value {
    let (start_line, start_character) = offset_to_position(text, span.0);
    let (end_line, end_character) = offset_to_position(text, span.1);
    json!({
        "start": { "line": start_line, "character": start_character },
        "end": { "line": end_line, "character": end_character },
    })
}

fn offset_to_position(text: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut character = 0u32;
    for ch in text[..offset].chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }
    (line, character)
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
