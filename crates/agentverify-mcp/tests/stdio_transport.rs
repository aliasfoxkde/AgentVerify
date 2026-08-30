//! Integration tests for the stdio transport and the stdio-backed client.
//!
//! Every test talks to a real subprocess peer, `fixtures/mcp_stdio_server.py`,
//! which speaks newline-delimited JSON-RPC 2.0 over stdin/stdout. The framing,
//! child lifetime, and failure modes exercised here are therefore the real
//! ones rather than stand-ins. The peer also uses the MCP specification's
//! lowerCamelCase payload keys and rejects an `initialize` request that
//! carries `snake_case` keys instead, so these tests fail if the client stops
//! speaking the specification's wire format.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::ErrorKind;
use std::time::Duration;

use agentverify_mcp::{
    ContentBlock, JsonRpcError, JsonRpcMessage, JsonRpcRequest, McpClient, McpClientConfig,
    McpClientError, ResourceContents, StdioTransport, TransportError, MCP_PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use tokio::time::{sleep, timeout, Instant};

/// Absolute path to the subprocess peer.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mcp_stdio_server.py"
);

/// Interpreter used to run the fixture.
const PYTHON: &str = "python3";

/// Upper bound for a single exchange with the peer.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(15);

/// Configuration pointing the client at `mode` of the test fixture.
fn client_config(mode: &'static str, timeout_secs: u64) -> McpClientConfig {
    McpClientConfig {
        timeout_secs,
        command: PYTHON.to_string(),
        args: vec!["-u".to_string(), FIXTURE.to_string(), mode.to_string()],
        ..McpClientConfig::default()
    }
}

/// Arguments that run `mode` of the fixture under the interpreter.
fn fixture_args(mode: &'static str) -> [&'static str; 3] {
    ["-u", FIXTURE, mode]
}

/// The error produced by `result`, discarding any success value.
fn error_of<T>(result: Result<T, McpClientError>) -> Option<McpClientError> {
    result.err()
}

/// The JSON-RPC error carried by `err`, or the error itself.
fn json_rpc_error(err: McpClientError) -> Result<JsonRpcError, McpClientError> {
    match err {
        McpClientError::JsonRpc(error) => Ok(error),
        other => Err(other),
    }
}

/// The text of a text content block, or a description of the actual block.
fn text_of(block: &ContentBlock) -> Result<&str, String> {
    match block {
        ContentBlock::Text { text } => Ok(text),
        other => Err(format!("expected a text block, got {other:?}")),
    }
}

/// The embedded resource of a resource content block.
fn resource_of(block: &ContentBlock) -> Result<&ResourceContents, String> {
    match block {
        ContentBlock::Resource { resource } => Ok(resource),
        other => Err(format!("expected a resource block, got {other:?}")),
    }
}

#[tokio::test]
async fn transport_roundtrips_a_request_over_stdio() {
    let transport = StdioTransport::connect(PYTHON, &fixture_args("ok"))
        .await
        .unwrap();
    assert!(transport.is_connected());

    transport
        .send(JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/list",
            None,
        )))
        .await
        .unwrap();

    let reply = timeout(EXCHANGE_TIMEOUT, transport.recv())
        .await
        .unwrap()
        .unwrap();
    let raw = serde_json::to_value(&reply).unwrap();
    assert_eq!(raw["jsonrpc"], json!("2.0"), "unexpected reply: {raw}");
    assert_eq!(raw["id"], json!(1), "unexpected reply: {raw}");
    assert_eq!(raw["result"]["tools"][0]["name"], json!("lookup_order"));

    // Closing the transport makes both directions fail immediately.
    transport.set_connected(false);
    assert!(!transport.is_connected());
    let request = JsonRpcRequest::new(2, "tools/list", None);
    let send = transport
        .send(JsonRpcMessage::Request(request))
        .await
        .unwrap_err();
    assert!(matches!(send, TransportError::NotConnected));
    let recv = transport.recv().await.unwrap_err();
    assert!(matches!(recv, TransportError::NotConnected));
}

#[tokio::test]
async fn transport_reports_malformed_output_as_a_json_error() {
    let transport = StdioTransport::connect(PYTHON, &fixture_args("bad-json"))
        .await
        .unwrap();

    let err = timeout(EXCHANGE_TIMEOUT, transport.recv())
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(err, TransportError::Json(_)));
}

#[tokio::test]
async fn transport_reports_a_blank_line_as_a_closed_connection() {
    let transport = StdioTransport::connect(PYTHON, &fixture_args("blank"))
        .await
        .unwrap();

    let err = timeout(EXCHANGE_TIMEOUT, transport.recv())
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(err, TransportError::NotConnected));
}

#[tokio::test]
async fn transport_reports_a_dead_peer_on_send() {
    let transport = StdioTransport::connect(PYTHON, &fixture_args("crash"))
        .await
        .unwrap();

    // The fixture exits before reading anything, so the write eventually hits a
    // closed pipe; poll until the kernel reports it.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_err = None;
    while Instant::now() < deadline {
        let request = JsonRpcRequest::new(1, "tools/list", None);
        match transport.send(JsonRpcMessage::Request(request)).await {
            Ok(()) => sleep(Duration::from_millis(25)).await,
            Err(err) => {
                last_err = Some(err);
                break;
            }
        }
    }

    let err = last_err.expect("send should fail once the peer has exited");
    assert!(
        matches!(err, TransportError::Io(ref io) if io.kind() == ErrorKind::BrokenPipe),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn client_reports_a_connection_closed_mid_request() {
    // The `blank` fixture breaks the framing with an empty line before
    // answering, so the request cannot complete.
    let client = McpClient::connect(client_config("blank", 15))
        .await
        .unwrap();

    let err = error_of(client.list_tools().await).expect("request should fail");
    assert!(
        matches!(err, McpClientError::Transport(TransportError::NotConnected)),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn client_reports_a_command_that_cannot_be_spawned() {
    let config = McpClientConfig {
        command: "agentverify-mcp-no-such-binary".to_string(),
        ..McpClientConfig::default()
    };

    let err = error_of(McpClient::connect(config).await).expect("connect should fail");
    assert!(
        matches!(err, McpClientError::Transport(TransportError::Io(ref io))
            if io.kind() == ErrorKind::NotFound),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn client_completes_a_full_session_over_stdio() {
    let client = McpClient::connect(client_config("ok", 15)).await.unwrap();
    assert!(client.is_connected());
    assert!(client.get_server_capabilities().await.is_none());

    let init = client.initialize().await.unwrap();
    assert_eq!(init.protocol_version, MCP_PROTOCOL_VERSION);
    assert_eq!(init.server_info.name, "agentverify-test-server");
    assert_eq!(
        init.instructions.as_deref(),
        Some("Verify the order before retrying the write.")
    );
    assert!(init.capabilities.tools.is_some());
    assert_eq!(
        init.capabilities.resources.as_ref().unwrap().subscribe,
        Some(true)
    );
    assert!(client.get_server_capabilities().await.is_some());

    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "lookup_order");
    let annotations = tools[0].annotations.as_ref().unwrap();
    assert_eq!(annotations.read_only_hint, Some(true));
    assert_eq!(annotations.destructive_hint, Some(false));
    assert_eq!(annotations.idempotent_hint, Some(true));

    let call = client
        .call_tool("lookup_order", json!({"order_id": "A-1"}))
        .await
        .unwrap();
    assert_eq!(call.is_error, Some(false));
    assert_eq!(call.content.len(), 2);
    assert!(
        text_of(&call.content[0])
            .unwrap()
            .contains("A-1 is VERIFIED"),
        "unexpected content: {:?}",
        call.content[0]
    );
    let resource = resource_of(&call.content[1]).unwrap();
    assert_eq!(resource.mime_type.as_deref(), Some("application/json"));
    assert_eq!(resource.text.as_deref(), Some("{\"state\": \"verified\"}"));
    assert!(resource.blob.is_none());

    let resources = client.list_resources().await.unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].mime_type.as_deref(), Some("application/json"));

    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].arguments.as_ref().unwrap()[0].required);

    let mut arguments = HashMap::new();
    arguments.insert("order_id".to_string(), "A-1".to_string());
    let prompt = client
        .get_prompt("summarise_order", Some(arguments))
        .await
        .unwrap();
    assert_eq!(prompt.messages.len(), 1);
    assert_eq!(prompt.messages[0].role, "user");

    // The server announced progress while the tool ran; the client queued it
    // instead of losing it.
    let progress = client.next_notification().await.unwrap();
    assert_eq!(progress.method, "notifications/progress");
    assert_eq!(progress.params.as_ref().unwrap()["progress"], json!(50));
}

#[tokio::test]
async fn client_answers_server_initiated_requests_with_method_not_found() {
    let client = McpClient::connect(client_config("ping-client", 15))
        .await
        .unwrap();

    let init = client.initialize().await.unwrap();
    assert_eq!(init.server_info.name, "agentverify-test-server");

    // The fixture only announces `notifications/ack` once it has seen the
    // client's -32601 reply to its `sampling/createMessage` request, so the
    // next exchange both observes and drains it.
    client.list_tools().await.unwrap();

    let ack = client.next_notification().await.unwrap();
    assert_eq!(ack.method, "notifications/ack");
    assert_eq!(ack.params.as_ref().unwrap()["rejected"], json!(4242));
}

#[tokio::test]
async fn client_surfaces_json_rpc_errors() {
    let client = McpClient::connect(client_config("error", 15))
        .await
        .unwrap();

    let err = client.initialize().await.unwrap_err();
    let error = json_rpc_error(err)
        .map_err(|other| format!("unexpected error: {other:?}"))
        .unwrap();
    assert_eq!(error.code, -32601);
    assert_eq!(error.message, "method initialize not found");
    assert_eq!(error.data.as_ref().unwrap()["method"], json!("initialize"));
}

#[tokio::test]
async fn client_times_out_against_a_silent_server() {
    let client = McpClient::connect(client_config("silent", 1))
        .await
        .unwrap();

    let err = timeout(EXCHANGE_TIMEOUT, client.initialize())
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(err, McpClientError::Timeout),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn client_stops_requesting_after_shutdown() {
    let client = McpClient::connect(client_config("ok", 15)).await.unwrap();
    client.shutdown();

    assert!(!client.is_connected());
    let err = client.list_tools().await.unwrap_err();
    assert!(
        matches!(err, McpClientError::Transport(TransportError::NotConnected)),
        "unexpected error: {err:?}"
    );
}

/// The raw JSON produced by a received message, for wire-level assertions.
fn to_wire(message: &JsonRpcMessage) -> Value {
    serde_json::to_value(message).unwrap()
}

#[tokio::test]
async fn transport_preserves_string_request_ids() {
    let transport = StdioTransport::connect(PYTHON, &fixture_args("ok"))
        .await
        .unwrap();

    // Hand-written params, exactly as the specification writes them.
    let params = json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": "agentverify-mcp", "version": "0.1.0"}
    });
    transport
        .send(JsonRpcMessage::Request(JsonRpcRequest::with_string_id(
            "session-1",
            "initialize",
            Some(params),
        )))
        .await
        .unwrap();

    let reply = timeout(EXCHANGE_TIMEOUT, transport.recv())
        .await
        .unwrap()
        .unwrap();
    let raw = to_wire(&reply);
    assert_eq!(raw["id"], json!("session-1"), "unexpected reply: {raw}");
    assert_eq!(
        raw["result"]["protocolVersion"],
        json!(MCP_PROTOCOL_VERSION),
        "the peer speaks the specification's camelCase: {raw}"
    );
}
