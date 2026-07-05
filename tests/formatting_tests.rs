use std::{
    fs,
    path::{Path, PathBuf},
};

use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;

use lumals::{
    formatting, parser, server,
    syntax::{FileId, SourceSpan},
};

#[test]
fn golden_document_formatting_is_idempotent() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/formatting");
    for input in fixture_paths(&fixture_dir) {
        let name = input.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&input).unwrap();
        let parsed = parser::parse(FileId(1), &name, &text);
        let formatted = formatting::format_text(FileId(1), &name, parsed.backend, &text).text;
        let reformatted =
            formatting::format_text(FileId(1), &name, parsed.backend, &formatted).text;
        let expected = fs::read_to_string(input.with_extension("golden")).unwrap();

        assert_eq!(formatted, expected, "fixture {name}");
        assert_eq!(reformatted, expected, "fixture {name} should be idempotent");
    }
}

#[test]
fn range_formatting_returns_minimal_edit() {
    let text = concat!(
        "root:\n",
        "   child:  one  \n",
        "   other: two\n",
        "tail: value\n"
    );
    let edit = formatting::format_range(text, FileId(9), SourceSpan::new(FileId(9), 6, 23))
        .expect("expected range formatting edit");

    assert_eq!(edit.range, SourceSpan::new(FileId(9), 6, 23));
    assert_eq!(edit.text, "  child: one\n");
}

#[test]
fn formatting_preserves_lua_blocks_and_crlf() {
    let input = concat!(
        "script: |\r\n",
        "    local value = 1  \r\n",
        "    print(value)\r\n",
        "snippet: ```lua\r\n",
        "  if value then   \r\n",
        "    print(value)\r\n",
        "```\r\n",
        "value:  ok  \r\n"
    );
    let parsed = parser::parse(FileId(2), "crlf.luma", input);
    let formatted = formatting::format_text(FileId(2), "crlf.luma", parsed.backend, input).text;

    assert!(formatted.contains("local value = 1  \r\n"));
    assert!(formatted.contains("  if value then   \r\n"));
    assert!(formatted.contains("value: ok\r\n"));
    assert!(!formatted.contains("value:  ok  \r\n"));
    assert!(!formatted.contains("\nvalue: ok\n"));
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_formatting_and_range_formatting_are_advertised_and_minimal() {
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
    let uri = "file:///formatting-test.luma";

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"processId": null, "capabilities": {}}
        }),
    )
    .await;
    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["documentFormattingProvider"],
        true
    );
    assert_eq!(
        initialize["result"]["capabilities"]["documentRangeFormattingProvider"],
        true
    );

    send_message(
        &mut writer,
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    )
    .await;
    let _initialized_log = read_message(&mut reader).await;

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
                    "text": "root:\n   child:  one  \n   other: two\n"
                }
            }
        }),
    )
    .await;
    let _diagnostics = read_message(&mut reader).await;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/rangeFormatting",
            "params": {
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 1, "character": 15}
                },
                "options": {"tabSize": 2, "insertSpaces": true}
            }
        }),
    )
    .await;
    let range_response = read_message(&mut reader).await;
    assert_eq!(
        range_response["result"],
        json!([{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 2, "character": 0}}, "newText": "  child: one\n"}])
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": {"uri": uri},
                "options": {"tabSize": 2, "insertSpaces": true}
            }
        }),
    )
    .await;
    let format_response = read_message(&mut reader).await;
    assert_eq!(
        format_response["result"],
        json!([{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 3, "character": 0}}, "newText": "root:\n  child: one\n  other: two\n"}])
    );

    send_message(
        &mut writer,
        &json!({"jsonrpc": "2.0", "id": 99, "method": "shutdown"}),
    )
    .await;
    let _shutdown_response = read_message(&mut reader).await;
    let _shutdown_log = read_message(&mut reader).await;
    send_message(&mut writer, &json!({"jsonrpc": "2.0", "method": "exit"})).await;
    writer.shutdown().await.unwrap();
    timeout(Duration::from_secs(2), server_task)
        .await
        .unwrap()
        .unwrap();
}

fn fixture_paths(dir: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "luma"))
        .collect::<Vec<_>>();
    entries.sort();
    entries
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
