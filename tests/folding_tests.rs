use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::server;

#[tokio::test(flavor = "current_thread")]
async fn folding_ranges_cover_supported_regions_without_invalid_overlap() {
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
    let uri = Url::parse("file:///workspace/folding-any-name-ending.luma").unwrap();

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
        initialize["result"]["capabilities"]["foldingRangeProvider"],
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
        "# top comment one\n",
        "# top comment two\n",
        "@meta owner=team\n",
        "@prelude bootstrap\n",
        "services:\n",
        "  api:\n",
        "    description: |\n",
        "      line one\n",
        "      line two\n",
        "  script: ```lua\n",
        "    print(\"hi\")\n",
        "  ```\n",
        "  tasks:\n",
        "    - first\n",
        "    - second\n",
        "  config:\n",
        "    ?if env == \"prod\"\n",
        "      replicas: 3\n",
        "    ?else\n",
        "      replicas: 1\n",
        "    *for item in items\n",
        "      name: ${item}\n",
        "...\n",
        "---\n",
        "catalog:\n",
        "  - name: widget\n",
        "  - name: gadget\n",
        "# trailing comment one\n",
        "# trailing comment two\n",
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

    let response = folding_ranges(&mut writer, &mut reader, &uri, 2).await;
    assert_eq!(
        response["result"],
        json!([
            { "startLine": 0, "endLine": 23, "kind": "region" },
            { "startLine": 1, "endLine": 4, "kind": "region" },
            { "startLine": 1, "endLine": 2, "kind": "comment" },
            { "startLine": 5, "endLine": 22, "kind": "region" },
            { "startLine": 6, "endLine": 9, "kind": "region" },
            { "startLine": 7, "endLine": 9, "kind": "region" },
            { "startLine": 10, "endLine": 12, "kind": "region" },
            { "startLine": 13, "endLine": 15, "kind": "region" },
            { "startLine": 14, "endLine": 15, "kind": "region" },
            { "startLine": 16, "endLine": 22, "kind": "region" },
            { "startLine": 17, "endLine": 18, "kind": "region" },
            { "startLine": 19, "endLine": 20, "kind": "region" },
            { "startLine": 21, "endLine": 22, "kind": "region" },
            { "startLine": 24, "endLine": 30, "kind": "region" },
            { "startLine": 25, "endLine": 27, "kind": "region" },
            { "startLine": 26, "endLine": 27, "kind": "region" },
            { "startLine": 28, "endLine": 29, "kind": "comment" }
        ])
    );

    let ranges = response["result"].as_array().unwrap();
    for (index, left) in ranges.iter().enumerate() {
        let left_start = left["startLine"].as_u64().unwrap();
        let left_end = left["endLine"].as_u64().unwrap();
        assert!(
            left_end > left_start,
            "range {left:?} should span multiple lines"
        );

        for right in &ranges[index + 1..] {
            let right_start = right["startLine"].as_u64().unwrap();
            let right_end = right["endLine"].as_u64().unwrap();
            let overlaps = left_start <= right_end && right_start <= left_end;
            let nested = (left_start <= right_start && left_end >= right_end)
                || (right_start <= left_start && right_end >= left_end);
            assert!(
                !overlaps || nested,
                "invalid partial overlap between {left:?} and {right:?}"
            );
        }
    }

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

async fn folding_ranges(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    uri: &Url,
    id: u64,
) -> Value {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/foldingRange",
            "params": {
                "textDocument": { "uri": uri }
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
