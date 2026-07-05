use std::{
    fs,
    path::{Path, PathBuf},
};

use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::Url;

use lumals::{parser, server, syntax::ParserBackend};

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_publish_matches_golden_fixtures() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diagnostics");

    for input in fixture_paths(&fixture_dir) {
        let name = input.file_name().unwrap().to_string_lossy().into_owned();
        let text = load_fixture_text(&input);
        let diagnostics = publish_diagnostics(&name, &text).await;
        let actual = summarize(&diagnostics);
        let golden = golden_path(&input);

        if std::env::var_os("UPDATE_GOLDENS").is_some() {
            fs::write(&golden, &actual).unwrap();
        }

        let expected = fs::read_to_string(&golden).unwrap();
        assert_eq!(actual, expected, "fixture {name}");
    }
}

fn golden_path(input: &Path) -> PathBuf {
    match parser::backend() {
        ParserBackend::Fallback => input.with_extension("golden"),
        ParserBackend::UpstreamLuma => input.with_file_name(format!(
            "{}.upstream.golden",
            input.file_stem().unwrap().to_string_lossy()
        )),
    }
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

fn load_fixture_text(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap()
        .replace("<TAB>", "\t")
        .replace("<NUL>", "\0")
}

fn summarize(message: &Value) -> String {
    let diagnostics = message["params"]["diagnostics"].as_array().unwrap();
    let mut out = String::new();
    out.push_str(&format!("count: {}\n", diagnostics.len()));

    for diagnostic in diagnostics {
        out.push_str(&format!(
            "- severity={} code={} source={} range={}:{}-{}:{}\n",
            diagnostic["severity"].as_u64().unwrap(),
            diagnostic["code"].as_str().unwrap(),
            diagnostic["source"].as_str().unwrap(),
            diagnostic["range"]["start"]["line"].as_u64().unwrap(),
            diagnostic["range"]["start"]["character"].as_u64().unwrap(),
            diagnostic["range"]["end"]["line"].as_u64().unwrap(),
            diagnostic["range"]["end"]["character"].as_u64().unwrap(),
        ));
        out.push_str(&format!(
            "  message={}\n",
            diagnostic["message"].as_str().unwrap().replace('\n', "\\n")
        ));

        if let Some(tags) = diagnostic["tags"].as_array() {
            out.push_str("  tags=");
            for (index, tag) in tags.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&tag.as_u64().unwrap().to_string());
            }
            out.push('\n');
        }

        if let Some(related) = diagnostic["relatedInformation"].as_array() {
            for item in related {
                out.push_str(&format!(
                    "  related={}:{}-{}:{} {}\n",
                    item["location"]["range"]["start"]["line"].as_u64().unwrap(),
                    item["location"]["range"]["start"]["character"]
                        .as_u64()
                        .unwrap(),
                    item["location"]["range"]["end"]["line"].as_u64().unwrap(),
                    item["location"]["range"]["end"]["character"]
                        .as_u64()
                        .unwrap(),
                    item["message"].as_str().unwrap(),
                ));
            }
        }
    }

    out
}

async fn publish_diagnostics(name: &str, text: &str) -> Value {
    let (client_to_server, server_stdin) = tokio::io::duplex(8 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(8 * 1024);
    let (service, socket) = server::service();

    let server_task = tokio::spawn(async move {
        Server::new(server_stdin, server_stdout, socket)
            .serve(service)
            .await;
    });

    let mut writer = client_to_server;
    let mut reader = server_to_client;
    let uri = Url::parse(&format!("file:///workspace/{name}")).unwrap();

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "clientInfo": { "name": "lumals-test", "version": "0" },
                "capabilities": {},
                "trace": "off"
            }
        }),
    )
    .await;
    let _initialize = read_message(&mut reader).await;

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
    let publish = read_message(&mut reader).await;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown"
        }),
    )
    .await;
    let _shutdown_response = read_message(&mut reader).await;
    let _shutdown_log = read_message(&mut reader).await;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
    )
    .await;

    drop(writer);
    timeout(Duration::from_secs(2), server_task)
        .await
        .expect("server task timed out")
        .expect("server task failed");

    publish
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
