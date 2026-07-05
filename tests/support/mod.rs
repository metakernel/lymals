use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::server;

pub struct LspHarness {
    writer: tokio::io::DuplexStream,
    reader: tokio::io::DuplexStream,
    server_task: tokio::task::JoinHandle<()>,
    next_id: u64,
}

impl LspHarness {
    pub async fn start() -> Self {
        let (client_to_server, server_stdin) = tokio::io::duplex(16 * 1024);
        let (server_stdout, server_to_client) = tokio::io::duplex(16 * 1024);
        let (service, socket) = server::service();

        let server_task = tokio::spawn(async move {
            Server::new(server_stdin, server_stdout, socket)
                .serve(service)
                .await;
        });

        Self {
            writer: client_to_server,
            reader: server_to_client,
            server_task,
            next_id: 1,
        }
    }

    pub async fn initialize(&mut self, root_uri: &Url) -> Value {
        self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {}
            }),
        )
        .await
    }

    pub async fn initialized(&mut self) -> Value {
        self.notify("initialized", json!({})).await;
        self.read_message().await
    }

    pub async fn did_open(&mut self, uri: &Url, text: &str) -> Value {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "luma",
                    "version": 1,
                    "text": text
                }
            }),
        )
        .await;
        self.read_message().await
    }

    pub async fn did_change(&mut self, uri: &Url, version: i32, text: &str) -> Value {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }]
            }),
        )
        .await;
        self.read_message().await
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.alloc_id();
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;
        self.read_response(id).await
    }

    pub async fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await;
    }

    pub async fn read_message(&mut self) -> Value {
        read_message(&mut self.reader).await
    }

    pub async fn read_response(&mut self, id: u64) -> Value {
        loop {
            let message = self.read_message().await;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                return message;
            }
        }
    }

    pub fn normalize_workspace(value: Value, root_uri: &Url) -> Value {
        let rendered = serde_json::to_string(&value).expect("json should serialize");
        serde_json::from_str(&rendered.replace(root_uri.as_str(), "file:///<WORKSPACE>"))
            .expect("normalized json should parse")
    }

    pub async fn shutdown(mut self) {
        let id = self.alloc_id();
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": "shutdown" }))
            .await;
        let _ = self.read_response(id).await;
        self.send(&json!({ "jsonrpc": "2.0", "method": "exit" }))
            .await;
        self.writer.shutdown().await.unwrap();
        timeout(Duration::from_secs(2), self.server_task)
            .await
            .expect("server task timed out")
            .expect("server task failed");
    }

    async fn send(&mut self, value: &Value) {
        let body = value.to_string();
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.writer.write_all(message.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
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
