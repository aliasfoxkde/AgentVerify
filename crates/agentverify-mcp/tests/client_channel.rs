//! In-process client tests.
//!
//! The server side of these tests is a real JSON-RPC 2.0 peer built from the
//! crate's `protocol` types and driven over `ChannelTransport`, so the client
//! is exercised end to end (initialize, tools, resources, prompts, error
//! handling, response correlation) without spawning a process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use agentverify_mcp::{
    error_codes, CallToolParams, CallToolResult, ChannelTransport, ClientCapabilities,
    ContentBlock, Implementation, InitializeParams, InitializeResult, JsonRpcError, JsonRpcMessage,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, McpClient, McpClientConfig,
    McpClientError, PromptsCapability, ResourcesCapability, ServerCapabilities, Tool,
    ToolAnnotations, ToolsCapability, TransportError, MCP_PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use tokio::task::JoinHandle;

/// How the in-process peer answers requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// Answer every method with its real payload.
    Standard,
    /// Read requests and never answer.
    Silent,
    /// Answer with a JSON-RPC error object.
    JsonRpcError,
    /// Answer with a result payload that does not match the expected shape.
    WrongShape,
    /// Answer a request nobody sent before answering the real one.
    ForeignId,
    /// Wait for two requests, then answer the second one first.
    ReverseOrder,
    /// Answer with hand-written JSON using the MCP specification's
    /// lowerCamelCase keys, as an external peer emits it.
    RawJson,
    /// Complete the handshake advertising only tools, so calls against the
    /// other features can be rejected locally.
    Toolless,
    /// Corrupt only the `initialize` result payload.
    BadInit,
}

/// The capabilities the test client advertises to the peer.
fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        sampling: None,
        elicitation: None,
        roots: Some(json!({"listChanged": true})),
    }
}

/// Configuration for a client talking to an in-process peer.
fn client_config(timeout_secs: u64) -> McpClientConfig {
    McpClientConfig {
        timeout_secs,
        capabilities: client_capabilities(),
        ..McpClientConfig::default()
    }
}

/// The capabilities announced by the in-process peer.
fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        resources: Some(ResourcesCapability {
            subscribe: Some(true),
            list: Some(true),
        }),
        tools: Some(ToolsCapability {}),
        prompts: Some(PromptsCapability { list: Some(true) }),
    }
}

/// The `initialize` result the in-process peer announces.
fn initialize_result() -> InitializeResult {
    InitializeResult {
        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        capabilities: server_capabilities(),
        server_info: Implementation {
            name: "in-process-server".to_string(),
            version: "0.1.0".to_string(),
        },
        instructions: Some("Verify outcomes before retrying.".to_string()),
    }
}

/// The single tool the in-process peer advertises.
fn advertised_tool() -> Tool {
    Tool {
        name: "close_ticket".to_string(),
        description: "Close a support ticket in the system of record.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {"ticket_id": {"type": "string"}},
            "required": ["ticket_id"]
        }),
        annotations: Some(ToolAnnotations {
            read_only_hint: Some(false),
            destructive_hint: Some(true),
            idempotent_hint: Some(false),
            annotation: Some("irreversible".to_string()),
        }),
    }
}

/// The `tools/call` result returned for `close_ticket`.
fn tool_result(params: &Value) -> CallToolResult {
    let call: CallToolParams = serde_json::from_value(params.clone()).unwrap();
    let ticket = call.arguments["ticket_id"].as_str().unwrap_or_default();
    CallToolResult {
        content: vec![
            ContentBlock::Text {
                text: format!("ticket {ticket} closed"),
            },
            ContentBlock::Image {
                data: "aGVsbG8=".to_string(),
                mime_type: "image/png".to_string(),
            },
        ],
        is_error: Some(false),
    }
}

/// The result payload for `method`, as the in-process peer computes it.
fn result_for(method: &str, params: Option<&Value>) -> Result<Value, String> {
    match method {
        "initialize" => serde_json::to_value(initialize_result()).map_err(|e| e.to_string()),
        "tools/list" => {
            serde_json::to_value(json!({ "tools": [advertised_tool()] })).map_err(|e| e.to_string())
        }
        "tools/call" => serde_json::to_value(tool_result(params.unwrap_or(&Value::Null)))
            .map_err(|e| e.to_string()),
        "resources/list" => serde_json::to_value(json!({
            "resources": [{
                "uri": "file:///contracts/close-ticket.json",
                "name": "close-ticket contract",
                "mimeType": "application/json",
            }]
        }))
        .map_err(|e| e.to_string()),
        "prompts/list" => serde_json::to_value(json!({
            "prompts": [{
                "name": "explain_failure",
                "description": "Explain a verification failure.",
                "arguments": [],
            }]
        }))
        .map_err(|e| e.to_string()),
        "prompts/get" => serde_json::to_value(json!({
            "messages": [{
                "role": "assistant",
                "content": {"type": "text", "text": "The postcondition never became true."},
            }]
        }))
        .map_err(|e| e.to_string()),
        other => Err(format!("unsupported method {other}")),
    }
}

/// Result payloads written as literal JSON with the specification's keys.
///
/// Unlike `result_for`, nothing here goes through the crate's own types, so it
/// is what a peer that has never seen `src/protocol.rs` would actually emit.
fn raw_result_for(method: &str, params: Option<&Value>) -> Result<Value, String> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {"tools": {}, "prompts": {"list": true}},
            "serverInfo": {"name": "raw-camel-case-peer", "version": "0.1.0"},
            "instructions": "Hand-written JSON, as an external peer emits it.",
        })),
        "tools/list" => Ok(json!({
            "tools": [{
                "name": "close_ticket",
                "description": "Close a support ticket in the system of record.",
                "inputSchema": {
                    "type": "object",
                    "properties": {"ticket_id": {"type": "string"}},
                    "required": ["ticket_id"],
                },
                "annotations": {
                    "readOnlyHint": false,
                    "destructiveHint": true,
                    "idempotentHint": false,
                },
            }]
        })),
        "tools/call" => {
            let call: CallToolParams =
                serde_json::from_value(params.cloned().unwrap_or(Value::Null))
                    .map_err(|e| e.to_string())?;
            let ticket = call.arguments["ticket_id"].as_str().unwrap_or_default();
            Ok(json!({
                "content": [{"type": "text", "text": format!("ticket {ticket} closed")}],
                "isError": false,
            }))
        }
        other => Err(format!("unsupported method {other}")),
    }
}

/// The result payload `behaviour` produces for `method`.
fn peer_result_for(
    behaviour: Behaviour,
    method: &str,
    params: Option<&Value>,
) -> Result<Value, String> {
    if let Behaviour::RawJson = behaviour {
        return raw_result_for(method, params);
    }
    if let Behaviour::Toolless = behaviour {
        // The handshake advertises tools only; every other feature must be
        // refused by the client before reaching the wire.
        return match method {
            "initialize" => {
                let mut init = initialize_result();
                init.capabilities = ServerCapabilities {
                    resources: None,
                    tools: Some(ToolsCapability {}),
                    prompts: None,
                };
                serde_json::to_value(init).map_err(|e| e.to_string())
            }
            _ => result_for(method, params),
        };
    }
    result_for(method, params)
}

/// Answer `request` according to `behaviour`.
async fn answer(peer: &mut ChannelTransport, request: &JsonRpcRequest, behaviour: Behaviour) {
    if let Behaviour::Silent = behaviour {
        return;
    }

    let error = |code: i32, message: String| JsonRpcResponse::Error {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        error: JsonRpcError::new(code, message),
    };

    let payload = match behaviour {
        Behaviour::JsonRpcError => JsonRpcResponse::Error {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            error: JsonRpcError::with_data(
                error_codes::UNSUPPORTED_PROTOCOL_VERSION,
                "protocol version not supported",
                json!({"supported": ["2025-06-18"]}),
            ),
        },
        _ => match peer_result_for(behaviour, &request.method, request.params.as_ref()) {
            Ok(result) => JsonRpcResponse::Success {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result,
            },
            Err(message) => error(error_codes::METHOD_NOT_FOUND, message),
        },
    };

    // A result payload the client can never decode. Under `WrongShape` the
    // handshake itself must still succeed, or the client would refuse later
    // calls as `NotInitialized` before their payloads could be decoded;
    // `BadInit` corrupts the handshake alone.
    let corrupt = |id: &Value| JsonRpcResponse::Success {
        jsonrpc: "2.0".to_string(),
        id: id.clone(),
        result: json!({"unexpected": true}),
    };
    let payload = match behaviour {
        Behaviour::WrongShape if request.method != "initialize" => corrupt(&request.id),
        Behaviour::BadInit => corrupt(&request.id),
        _ => payload,
    };

    peer.send(JsonRpcMessage::Response(payload)).await.unwrap();
}

/// Run an in-process MCP peer that behaves according to `behaviour`.
///
/// The returned log records every request the peer accepted, so tests can
/// assert on what the client actually put on the wire.
fn spawn_peer(
    mut peer: ChannelTransport,
    behaviour: Behaviour,
) -> (JoinHandle<()>, Arc<Mutex<Vec<Value>>>) {
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&received);

    let handle = tokio::spawn(async move {
        let mut buffer: Vec<JsonRpcRequest> = Vec::new();
        loop {
            // The client dropping its transport ends the peer.
            let Ok(message) = peer.recv().await else {
                return;
            };
            let JsonRpcMessage::Request(request) = message else {
                continue;
            };

            log.lock()
                .unwrap()
                .push(serde_json::to_value(&request).unwrap());

            if let Behaviour::ReverseOrder = behaviour {
                // Only these two are held back; `initialize` is answered
                // normally so the session can be established first.
                if matches!(request.method.as_str(), "tools/list" | "tools/call") {
                    buffer.push(request);
                    if buffer.len() < 2 {
                        continue;
                    }
                    // Answer out of order to prove responses are correlated by
                    // id rather than by arrival order.
                    answer(&mut peer, &buffer[1], behaviour).await;
                    answer(&mut peer, &buffer[0], behaviour).await;
                    buffer.clear();
                    continue;
                }
            }

            if let Behaviour::ForeignId = behaviour {
                // Replies for requests this client never sent: one carrying a
                // numeric id and one carrying a string id.
                for stale_id in [json!(98_765), json!("stale-request")] {
                    peer.send(JsonRpcMessage::Response(JsonRpcResponse::Success {
                        jsonrpc: "2.0".to_string(),
                        id: stale_id,
                        result: json!({"tools": []}),
                    }))
                    .await
                    .unwrap();
                }
            }

            // Progress notifications are announced ahead of the tool result.
            if request.method == "tools/call" {
                peer.send(JsonRpcMessage::Notification(JsonRpcNotification::new(
                    "notifications/progress",
                    Some(json!({"progressToken": 3, "progress": 25})),
                )))
                .await
                .unwrap();
            }

            answer(&mut peer, &request, behaviour).await;
        }
    });

    (handle, received)
}

/// Connect a client to a freshly spawned in-process peer.
fn connect_with(
    behaviour: Behaviour,
    timeout_secs: u64,
) -> (McpClient, JoinHandle<()>, Arc<Mutex<Vec<Value>>>) {
    let (client, peer) = McpClient::with_channel_peer(client_config(timeout_secs)).unwrap();
    let (handle, received) = spawn_peer(peer, behaviour);
    (client, handle, received)
}

#[tokio::test]
async fn client_completes_a_full_in_process_session() {
    let (client, handle, received) = connect_with(Behaviour::Standard, 15);
    assert!(client.is_connected());
    assert!(client.get_server_capabilities().await.is_none());

    let init = client.initialize().await.unwrap();
    assert_eq!(init.protocol_version, MCP_PROTOCOL_VERSION);
    assert_eq!(init.server_info.name, "in-process-server");
    assert_eq!(
        init.instructions.as_deref(),
        Some("Verify outcomes before retrying.")
    );
    assert!(init.capabilities.tools.is_some());
    assert_eq!(init.capabilities.prompts.as_ref().unwrap().list, Some(true));
    assert_eq!(
        client
            .get_server_capabilities()
            .await
            .unwrap()
            .resources
            .as_ref()
            .unwrap()
            .list,
        Some(true)
    );

    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].input_schema["required"][0], json!("ticket_id"));
    assert_eq!(
        tools[0].annotations.as_ref().unwrap().annotation.as_deref(),
        Some("irreversible")
    );

    let call = client
        .call_tool("close_ticket", json!({"ticket_id": "T-9"}))
        .await
        .unwrap();
    assert_eq!(call.is_error, Some(false));
    assert_eq!(call.content.len(), 2);

    let resources = client.list_resources().await.unwrap();
    assert_eq!(resources[0].uri, "file:///contracts/close-ticket.json");

    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts[0].name, "explain_failure");

    let prompt = client.get_prompt("explain_failure", None).await.unwrap();
    assert_eq!(prompt.messages[0].role, "assistant");

    // The tool call announced progress before its result arrived, and the
    // client kept it instead of dropping it.
    let progress = client.next_notification().await.unwrap();
    assert_eq!(progress.method, "notifications/progress");
    assert_eq!(progress.params.as_ref().unwrap()["progress"], json!(25));
    assert!(client.next_notification().await.is_none());

    // What the client actually put on the wire, in order.
    let sent = received.lock().unwrap().clone();
    let methods: Vec<&str> = sent
        .iter()
        .map(|request| request["method"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        methods,
        vec![
            "initialize",
            "tools/list",
            "tools/call",
            "resources/list",
            "prompts/list",
            "prompts/get"
        ]
    );
    assert_eq!(sent[0]["jsonrpc"], json!("2.0"));
    assert_eq!(sent[0]["id"], json!(1));
    assert_eq!(
        sent[0]["params"],
        serde_json::to_value(InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: client_capabilities(),
            client_info: McpClientConfig::default().client_info,
        })
        .unwrap()
    );
    assert_eq!(sent[2]["params"]["name"], json!("close_ticket"));
    assert_eq!(
        sent[2]["params"]["arguments"],
        json!({"ticket_id": "T-9"}),
        "tool arguments must reach the peer untouched"
    );

    handle.abort();
}

/// The client interoperates with a peer that speaks only the specification's
/// camelCase wire format, and emits camelCase itself.
#[tokio::test]
async fn client_interoperates_with_a_camel_case_peer() {
    let (client, handle, received) = connect_with(Behaviour::RawJson, 15);

    let init = client.initialize().await.unwrap();
    assert_eq!(init.protocol_version, MCP_PROTOCOL_VERSION);
    assert_eq!(init.server_info.name, "raw-camel-case-peer");
    assert_eq!(
        init.instructions.as_deref(),
        Some("Hand-written JSON, as an external peer emits it.")
    );
    assert!(init.capabilities.tools.is_some());
    assert_eq!(init.capabilities.prompts.as_ref().unwrap().list, Some(true));

    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].input_schema["required"][0], json!("ticket_id"));
    assert_eq!(
        tools[0].annotations.as_ref().unwrap().destructive_hint,
        Some(true)
    );

    let call = client
        .call_tool("close_ticket", json!({"ticket_id": "T-9"}))
        .await
        .unwrap();
    assert_eq!(call.is_error, Some(false));

    // What the client itself put on the wire is camelCase too.
    let sent = received.lock().unwrap().clone();
    let init_params = &sent[0]["params"];
    assert_eq!(
        init_params["protocolVersion"],
        json!(MCP_PROTOCOL_VERSION),
        "unexpected wire params: {init_params}"
    );
    assert_eq!(
        init_params["clientInfo"]["name"],
        json!("agentverify-mcp"),
        "unexpected wire params: {init_params}"
    );
    assert!(
        init_params.get("protocol_version").is_none() && init_params.get("client_info").is_none(),
        "snake_case must not reach the wire: {init_params}"
    );

    let call_params = &sent[2]["params"];
    assert_eq!(call_params["name"], json!("close_ticket"));
    assert_eq!(call_params["arguments"], json!({"ticket_id": "T-9"}));
    assert!(
        call_params.get("input_schema").is_none(),
        "snake_case must not reach the wire: {call_params}"
    );

    handle.abort();
}

#[tokio::test]
async fn client_surfaces_json_rpc_errors_from_the_peer() {
    let (client, handle, _) = connect_with(Behaviour::JsonRpcError, 15);

    let err = client.initialize().await.unwrap_err();
    assert!(
        matches!(&err, McpClientError::JsonRpc(error)
            if error.code == error_codes::UNSUPPORTED_PROTOCOL_VERSION),
        "unexpected error: {err:?}"
    );

    handle.abort();
}

#[tokio::test]
async fn client_reports_an_initialize_payload_it_cannot_decode() {
    let (client, handle, _) = connect_with(Behaviour::BadInit, 15);

    assert!(matches!(
        client.initialize().await.unwrap_err(),
        McpClientError::InvalidResponse { ref expected, .. } if expected == "InitializeResult"
    ));

    handle.abort();
}

#[tokio::test]
async fn client_reports_result_payloads_it_cannot_decode() {
    // The handshake succeeds here (`WrongShape` only corrupts feature
    // payloads), so each assertion below reaches its own response decode.
    let (client, handle, _) = connect_with(Behaviour::WrongShape, 15);
    client.initialize().await.unwrap();

    assert!(matches!(
        client.list_tools().await.unwrap_err(),
        McpClientError::InvalidResponse { ref expected, .. } if expected == "ToolsListResponse"
    ));
    assert!(matches!(
        client.call_tool("close_ticket", json!({"ticket_id": "T-9"}))
            .await
            .unwrap_err(),
        McpClientError::InvalidResponse { ref expected, .. } if expected == "CallToolResult"
    ));
    assert!(matches!(
        client.list_resources().await.unwrap_err(),
        McpClientError::InvalidResponse { ref expected, .. } if expected == "ResourcesListResponse"
    ));
    assert!(matches!(
        client.list_prompts().await.unwrap_err(),
        McpClientError::InvalidResponse { ref expected, .. } if expected == "PromptsListResponse"
    ));
    assert!(matches!(
        client.get_prompt("explain_failure", None).await.unwrap_err(),
        McpClientError::InvalidResponse { ref expected, .. } if expected == "GetPromptResult"
    ));

    handle.abort();
}

#[tokio::test]
async fn client_ignores_responses_for_requests_it_never_sent() {
    let (client, handle, _) = connect_with(Behaviour::ForeignId, 15);
    client.initialize().await.unwrap();

    let tools = client.list_tools().await.unwrap();
    // The stale replies are dropped and the real answer still arrives.
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "close_ticket");

    handle.abort();
}

#[tokio::test]
async fn client_correlates_out_of_order_responses() {
    let (client, handle, _) = connect_with(Behaviour::ReverseOrder, 15);
    client.initialize().await.unwrap();

    let (tools, call) = tokio::join!(
        client.list_tools(),
        client.call_tool("close_ticket", json!({"ticket_id": "T-9"}))
    );
    // Each request receives its own answer although the peer reversed them.
    assert_eq!(tools.unwrap()[0].name, "close_ticket");
    assert_eq!(call.unwrap().is_error, Some(false));

    handle.abort();
}

#[tokio::test]
async fn client_times_out_when_the_peer_never_answers() {
    let (client, handle, _) = connect_with(Behaviour::Silent, 1);

    let err = client.initialize().await.unwrap_err();
    assert!(
        matches!(err, McpClientError::Timeout),
        "unexpected error: {err:?}"
    );

    handle.abort();
}

#[tokio::test]
async fn client_stops_requesting_after_shutdown() {
    let (client, handle, _) = connect_with(Behaviour::Standard, 15);
    client.initialize().await.unwrap();

    client.shutdown();
    assert!(!client.is_connected());
    let err = client.list_tools().await.unwrap_err();
    assert!(
        matches!(err, McpClientError::Transport(TransportError::NotConnected)),
        "unexpected error: {err:?}"
    );

    handle.abort();
}

#[tokio::test]
async fn with_channel_transport_cannot_be_answered() {
    // The peer half of this constructor's channel is discarded, so a request
    // fails on the transport instead of hanging until the timeout. The
    // handshake itself is sent (it needs no prior session), so the transport
    // failure — not the initialize guard — is what surfaces.
    let client = McpClient::with_channel_transport(client_config(1))
        .await
        .unwrap();
    assert!(client.is_connected());

    let err = client.initialize().await.unwrap_err();
    assert!(
        matches!(
            &err,
            McpClientError::Transport(TransportError::Channel(message))
                if message == "Receiver dropped"
        ),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn client_requires_initialize_before_feature_calls() {
    let (client, handle, _) = connect_with(Behaviour::Standard, 15);

    // Every feature call is refused locally; nothing reaches the peer.
    assert!(matches!(
        client.list_tools().await.unwrap_err(),
        McpClientError::NotInitialized
    ));
    assert!(matches!(
        client
            .call_tool("close_ticket", json!({"ticket_id": "T-9"}))
            .await
            .unwrap_err(),
        McpClientError::NotInitialized
    ));
    assert!(matches!(
        client.list_resources().await.unwrap_err(),
        McpClientError::NotInitialized
    ));
    assert!(matches!(
        client.list_prompts().await.unwrap_err(),
        McpClientError::NotInitialized
    ));
    assert!(matches!(
        client
            .get_prompt("explain_failure", None)
            .await
            .unwrap_err(),
        McpClientError::NotInitialized
    ));
    assert!(client.get_server_capabilities().await.is_none());

    // Once the handshake completes, the same calls go through.
    client.initialize().await.unwrap();
    assert_eq!(client.list_tools().await.unwrap().len(), 1);

    handle.abort();
}

#[tokio::test]
async fn client_rejects_features_the_server_does_not_advertise() {
    let (client, handle, _) = connect_with(Behaviour::Toolless, 15);
    client.initialize().await.unwrap();

    // Tools are advertised and work.
    assert_eq!(client.list_tools().await.unwrap().len(), 1);

    // Resources and prompts were not advertised, so the client refuses them
    // instead of spending a round trip on `METHOD_NOT_FOUND`.
    let resources = client.list_resources().await.unwrap_err();
    assert!(
        matches!(&resources, McpClientError::CapabilityNotSupported(feature)
            if feature == "resources"),
        "unexpected error: {resources:?}"
    );
    let prompts = client.list_prompts().await.unwrap_err();
    assert!(
        matches!(&prompts, McpClientError::CapabilityNotSupported(feature)
            if feature == "prompts"),
        "unexpected error: {prompts:?}"
    );
    assert!(matches!(
        client
            .get_prompt("explain_failure", None)
            .await
            .unwrap_err(),
        McpClientError::CapabilityNotSupported(_)
    ));

    handle.abort();
}
