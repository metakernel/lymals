use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;

use lumals::config::LumalsConfig;
use lumals::server;

#[test]
fn config_defaults_match_expected_policy() {
    let config = LumalsConfig::default();

    assert!(config.diagnostics.enabled);
    assert!(config.formatting.enabled);
    assert!(config.imports.enabled);
    assert!(config.semantic_tokens.enabled);
    assert!(config.completion.enabled);
    assert!(config.inlay_hints.enabled);
    assert!(!config.evaluation.enabled);
    assert_eq!(config.allowed_schemes, ["file"]);
    assert_eq!(config.max_resolve_depth, 16);
    assert_eq!(config.max_resolved_edges_per_file, 256);
    assert_eq!(config.max_indexed_files_per_workspace, 10_000);
    assert_eq!(config.max_indexed_file_bytes, 1_048_576);
}

#[test]
fn config_overrides_deserialize_from_partial_settings() {
    let config: LumalsConfig = serde_json::from_value(json!({
        "diagnostics": { "enabled": false },
        "semanticTokens": { "enabled": false },
        "completion": { "enabled": false },
        "inlayHints": { "enabled": false },
        "logLevel": "trace",
        "parserBackend": "fallback",
        "allowedSchemes": ["file", "untitled"],
        "allowedRoots": ["file:///workspace"],
        "excludeGlobs": ["**/generated/**"],
        "maxResolveDepth": 4,
        "maxResolvedEdgesPerFile": 32,
        "maxIndexedFilesPerWorkspace": 128,
        "maxIndexedFileBytes": 4096
    }))
    .unwrap();

    assert!(!config.diagnostics.enabled);
    assert!(!config.semantic_tokens.enabled);
    assert!(!config.completion.enabled);
    assert!(!config.inlay_hints.enabled);
    assert_eq!(config.log_level, lumals::config::LogLevel::Trace);
    assert_eq!(
        config.parser_backend,
        lumals::config::ParserBackend::Fallback
    );
    assert_eq!(config.allowed_schemes, ["file", "untitled"]);
    assert_eq!(config.allowed_roots, ["file:///workspace"]);
    assert_eq!(config.exclude_globs, ["**/generated/**"]);
    assert_eq!(config.max_resolve_depth, 4);
    assert_eq!(config.max_resolved_edges_per_file, 32);
    assert_eq!(config.max_indexed_files_per_workspace, 128);
    assert_eq!(config.max_indexed_file_bytes, 4096);
    assert!(!config.evaluation.enabled);
}

#[test]
fn generated_schema_exposes_defaults_and_sections() {
    let schema = lumals::config::config_schema();

    assert_eq!(schema["$id"], "https://lumals.dev/schemas/lumals.json");
    assert_eq!(schema["properties"]["allowedSchemes"]["default"][0], "file");
    assert_eq!(
        schema["properties"]["diagnostics"]["default"]["enabled"],
        true
    );
    assert_eq!(
        schema["properties"]["evaluation"]["default"]["enabled"],
        false
    );
}

#[tokio::test(flavor = "current_thread")]
async fn initializes_with_workspace_configuration_when_client_supports_it() {
    let (client_to_server, server_stdin) = tokio::io::duplex(8 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(8 * 1024);

    let (service, socket) = server::service();
    let backend = service.inner().clone();

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
                "clientInfo": { "name": "lumals-test", "version": "0" },
                "capabilities": {
                    "workspace": {
                        "configuration": true
                    }
                }
            }
        }),
    )
    .await;

    let initialize = read_message(&mut reader).await;
    assert_eq!(initialize["id"], 1);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )
    .await;

    let configuration_request = read_message(&mut reader).await;
    assert_eq!(configuration_request["method"], "workspace/configuration");
    assert_eq!(
        configuration_request["params"]["items"][0]["section"],
        "lumals"
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": configuration_request["id"],
            "result": [{
                "diagnostics": { "enabled": false },
                "evaluation": { "enabled": true },
                "logLevel": "debug",
                "maxResolveDepth": 7
            }]
        }),
    )
    .await;

    let _initialized_log = read_message(&mut reader).await;

    let snapshot = backend.state_snapshot();
    assert!(!snapshot.config.diagnostics.enabled);
    assert!(snapshot.config.evaluation.enabled);
    assert_eq!(snapshot.config.log_level, lumals::config::LogLevel::Debug);
    assert_eq!(snapshot.config.max_resolve_depth, 7);

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

#[tokio::test(flavor = "current_thread")]
async fn falls_back_to_defaults_without_workspace_configuration_support() {
    let (client_to_server, server_stdin) = tokio::io::duplex(8 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(8 * 1024);

    let (service, socket) = server::service();
    let backend = service.inner().clone();

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
                "clientInfo": { "name": "lumals-test", "version": "0" },
                "capabilities": {}
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

    let initialized_log = read_message(&mut reader).await;
    assert_eq!(initialized_log["method"], "window/logMessage");

    let snapshot = backend.state_snapshot();
    assert_eq!(snapshot.config, LumalsConfig::default());

    shutdown_server(&mut writer, &mut reader, server_task).await;
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
            "id": 2,
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
