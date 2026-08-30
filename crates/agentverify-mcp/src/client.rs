//! MCP Client Implementation
//!
//! Provides a high-level async client for interacting with MCP servers.
//! Handles request/response correlation, capability negotiation, and
//! protocol method invocations.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tokio::time::{timeout_at, Duration, Instant};

use crate::protocol::{
    error_codes, CallToolParams, CallToolResult, ClientCapabilities, ContentBlock, Implementation,
    InitializeParams, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, Prompt, Resource, ServerCapabilities, Tool,
    MCP_PROTOCOL_VERSION,
};
use crate::transport::{ChannelTransport, StdioTransport, TransportError};

/// Capacity of the queue holding server notifications that no caller has
/// drained yet. Notifications beyond this bound are dropped rather than
/// allowed to stall response correlation.
const NOTIFICATION_QUEUE_CAPACITY: usize = 64;

/// MCP Client error types
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    /// The underlying transport failed to send or receive a message.
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    /// The server replied with a JSON-RPC error object.
    #[error("JSON-RPC error: {0}")]
    JsonRpc(JsonRpcError),

    /// The request did not complete within the configured timeout.
    #[error("Request timeout")]
    Timeout,

    /// The client has not completed the MCP initialize handshake.
    #[error("Not initialized")]
    NotInitialized,

    /// The server reported an error code and message.
    #[error("Server returned error: {code} {message}")]
    ServerError {
        /// The JSON-RPC error code returned by the server.
        code: i32,
        /// The human-readable error message returned by the server.
        message: String,
    },

    /// The response payload did not match the expected shape.
    #[error("Invalid response: expected {expected}, got {got}")]
    InvalidResponse {
        /// Description of the shape that was expected.
        expected: String,
        /// Description (or error text) of what was actually received.
        got: String,
    },

    /// The server does not advertise the requested capability.
    #[error("Capability not supported: {0}")]
    CapabilityNotSupported(String),

    /// An in-process channel used to correlate responses failed.
    #[error("Channel error: {0}")]
    Channel(String),
}

/// MCP Client configuration
#[derive(Debug, Clone)]
pub struct McpClientConfig {
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Server command
    pub command: String,
    /// Server arguments
    pub args: Vec<String>,
    /// Client info
    pub client_info: Implementation,
    /// Initial capabilities
    pub capabilities: ClientCapabilities,
}

impl Default for McpClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            command: String::new(),
            args: Vec::new(),
            client_info: Implementation {
                name: "agentverify-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            capabilities: ClientCapabilities::default(),
        }
    }
}

/// Pending request handle used to correlate responses
struct PendingRequest {
    response_tx: oneshot::Sender<JsonRpcResponse>,
}

/// Unified transport type for MCP client
enum McpTransport {
    Stdio(StdioTransport),
    Channel(ChannelTransport),
}

/// MCP Client for connecting to MCP servers
pub struct McpClient {
    transport: McpTransport,
    config: McpClientConfig,
    server_capabilities: Arc<RwLock<Option<ServerCapabilities>>>,
    next_request_id: Arc<RwLock<u64>>,
    pending_requests: Arc<RwLock<HashMap<u64, PendingRequest>>>,
    /// Sending half of the queue that buffers server notifications.
    notification_tx: mpsc::Sender<JsonRpcNotification>,
    /// Receiving half of the queue; drained with
    /// [`McpClient::next_notification`].
    notifications: Mutex<mpsc::Receiver<JsonRpcNotification>>,
}

impl McpClient {
    /// Connect to an MCP server via stdio
    ///
    /// # Errors
    ///
    /// Returns [`McpClientError::Transport`] if the server process cannot be
    /// spawned or its stdio streams cannot be acquired.
    pub async fn connect(config: McpClientConfig) -> Result<Self, McpClientError> {
        let args_refs: Vec<&str> = config.args.iter().map(String::as_str).collect();
        let transport = StdioTransport::connect(&config.command, &args_refs)
            .await
            .map_err(McpClientError::Transport)?;

        Ok(Self::with_transport(config, McpTransport::Stdio(transport)))
    }

    /// Create a client together with the peer transport that answers it.
    ///
    /// The client sends requests into the returned [`ChannelTransport`] and
    /// reads replies from it, so a caller can implement an in-process MCP
    /// server with the `protocol` module types and exercise the client end to
    /// end without spawning a process.
    ///
    /// # Errors
    ///
    /// This constructor currently always succeeds; the `Result` exists so the
    /// signature matches the other constructors.
    pub fn with_channel_peer(
        config: McpClientConfig,
    ) -> Result<(Self, ChannelTransport), McpClientError> {
        let (client_side, server_side) = ChannelTransport::channel();
        let client = Self::with_transport(config, McpTransport::Channel(client_side));
        Ok((client, server_side))
    }

    /// Create client with a channel transport whose peer half is discarded
    ///
    /// Because the peer is dropped, requests issued through the returned client
    /// fail with [`McpClientError::Channel`]. Use [`McpClient::with_channel_peer`]
    /// when the transport must be answered in-process.
    ///
    /// # Errors
    ///
    /// This constructor currently always succeeds; the `Result` exists so the
    /// signature matches the other constructors.
    ///
    /// The signature is `async` for parity with [`McpClient::connect`], even
    /// though the body is currently synchronous.
    #[allow(clippy::unused_async)]
    pub async fn with_channel_transport(config: McpClientConfig) -> Result<Self, McpClientError> {
        Ok(Self::with_channel_peer(config)?.0)
    }

    /// Create client with a transport enum
    fn with_transport(config: McpClientConfig, transport: McpTransport) -> Self {
        let (notification_tx, notification_rx) = mpsc::channel(NOTIFICATION_QUEUE_CAPACITY);

        Self {
            transport,
            config,
            server_capabilities: Arc::new(RwLock::new(None)),
            next_request_id: Arc::new(RwLock::new(1)),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            notification_tx,
            notifications: Mutex::new(notification_rx),
        }
    }

    /// Initialize the MCP connection and negotiate capabilities
    ///
    /// Sends the `initialize` request, records the server's advertised
    /// capabilities, and emits the `initialized` notification.
    ///
    /// # Errors
    ///
    /// Returns [`McpClientError`] if the request fails, times out, or the
    /// server's response cannot be decoded as an [`InitializeResult`].
    pub async fn initialize(&self) -> Result<InitializeResult, McpClientError> {
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: self.config.capabilities.clone(),
            client_info: self.config.client_info.clone(),
        };

        let result = self
            .request(
                "initialize",
                Some(serde_json::to_value(params).map_err(TransportError::Json)?),
            )
            .await?;

        let response: InitializeResult =
            serde_json::from_value(result).map_err(|e| McpClientError::InvalidResponse {
                expected: "InitializeResult".to_string(),
                got: e.to_string(),
            })?;

        // Store server capabilities
        {
            let mut caps = self.server_capabilities.write().await;
            *caps = Some(response.capabilities.clone());
        }

        // Send initialized notification
        let notification = JsonRpcNotification::new("initialized", None);
        self.send_message(JsonRpcMessage::Notification(notification))
            .await?;

        Ok(response)
    }

    /// Send a JSON-RPC message
    async fn send_message(&self, msg: JsonRpcMessage) -> Result<(), McpClientError> {
        match &self.transport {
            McpTransport::Stdio(t) => t.send(msg).await.map_err(Into::into),
            McpTransport::Channel(t) => t.send(msg).await.map_err(Into::into),
        }
    }

    /// Receive a JSON-RPC message
    async fn recv_message(&self) -> Result<JsonRpcMessage, McpClientError> {
        match &self.transport {
            McpTransport::Stdio(t) => t.recv().await.map_err(Into::into),
            McpTransport::Channel(t) => t.recv().await.map_err(Into::into),
        }
    }

    /// Send a JSON-RPC request and wait for the matching response
    ///
    /// While waiting, messages that are not this request's response are
    /// dispatched: responses belonging to other in-flight requests are handed
    /// to their owner, notifications are queued, and server-initiated requests
    /// are rejected with `METHOD_NOT_FOUND`.
    async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, McpClientError> {
        let request_id = {
            let mut id_guard = self.next_request_id.write().await;
            let id = *id_guard;
            *id_guard += 1;
            id
        };

        let request = JsonRpcRequest::new(request_id, method, params);
        let expected_id = request.id.clone();
        let (response_tx, response_rx) = oneshot::channel();

        // Register pending request so concurrent callers can be handed their
        // responses when this loop reads them first.
        {
            let mut pending = self.pending_requests.write().await;
            pending.insert(request_id, PendingRequest { response_tx });
        }

        let deadline = Instant::now() + Duration::from_secs(self.config.timeout_secs);

        let outcome = async {
            self.send_message(JsonRpcMessage::Request(request)).await?;
            self.await_response(&expected_id, deadline, response_rx)
                .await
        }
        .await;

        // Whatever the outcome, this request no longer owns its slot.
        Self::take_pending(&self.pending_requests, request_id).await;

        match outcome? {
            JsonRpcResponse::Success { result, .. } => Ok(result),
            JsonRpcResponse::Error { error, .. } => Err(McpClientError::JsonRpc(error)),
        }
    }

    /// Read messages until the response identified by `expected_id` arrives
    async fn await_response(
        &self,
        expected_id: &serde_json::Value,
        deadline: Instant,
        mut response_rx: oneshot::Receiver<JsonRpcResponse>,
    ) -> Result<JsonRpcResponse, McpClientError> {
        match timeout_at(deadline, async {
            loop {
                // Wait for either the correlated response or the next
                // transport message; a concurrent caller may have routed
                // the response here already.
                let step = tokio::select! {
                    response = &mut response_rx => match response {
                        Ok(response) => Step::Correlated(response),
                        // Defensive: the only sender is this client's own
                        // pending slot, which outlives the wait, so this cannot
                        // fire while a request is in flight.
                        Err(_) => {
                            return Err(McpClientError::Channel(
                                "response channel closed".to_string(),
                            ))
                        }
                    },
                    message = self.recv_message() => match message {
                        Ok(message) => Step::Message(message),
                        Err(err) => return Err(err),
                    },
                };

                match step {
                    Step::Correlated(response) => return Ok(response),
                    Step::Message(message) => {
                        if let Some(response) = self.dispatch(message, expected_id).await? {
                            return Ok(response);
                        }
                    }
                }
            }
        })
        .await
        {
            Ok(response) => response,
            Err(_elapsed) => Err(McpClientError::Timeout),
        }
    }

    /// Route a message that is not known to be the awaited response
    ///
    /// Returns the awaited response when the transport delivers it directly.
    async fn dispatch(
        &self,
        message: JsonRpcMessage,
        expected_id: &serde_json::Value,
    ) -> Result<Option<JsonRpcResponse>, McpClientError> {
        match message {
            JsonRpcMessage::Response(response) => {
                if response_id(&response) == expected_id {
                    return Ok(Some(response));
                }

                // Someone else's response: hand it to the owner if that request
                // is still pending, otherwise the response is stale.
                let Some(owner_id) = response_id(&response).as_u64() else {
                    return Ok(None);
                };
                if let Some(pending) = Self::take_pending(&self.pending_requests, owner_id).await {
                    let _ = pending.response_tx.send(response);
                }
                Ok(None)
            }
            JsonRpcMessage::Notification(notification) => {
                // Bounded queue: a caller that never drains notifications must
                // not be able to stall response correlation.
                let _ = self.notification_tx.try_send(notification);
                Ok(None)
            }
            JsonRpcMessage::Request(incoming) => {
                self.reject_request(incoming.id).await;
                Ok(None)
            }
        }
    }

    /// Reply to a server-initiated request with "method not found"
    ///
    /// This client issues requests but does not implement the server-to-client
    /// methods (`sampling/createMessage`, `roots/list`, ...), so the
    /// specification-compliant answer is a JSON-RPC error.
    async fn reject_request(&self, id: serde_json::Value) {
        let response = JsonRpcResponse::Error {
            jsonrpc: "2.0".to_string(),
            id,
            error: JsonRpcError::new(
                error_codes::METHOD_NOT_FOUND,
                "client does not support server-initiated requests",
            ),
        };
        let _ = self.send_message(JsonRpcMessage::Response(response)).await;
    }

    /// Remove a pending request, returning its response channel if it was
    /// still registered
    async fn take_pending(
        pending: &RwLock<HashMap<u64, PendingRequest>>,
        id: u64,
    ) -> Option<PendingRequest> {
        pending.write().await.remove(&id)
    }

    // =============================================================================
    // Server Features (as client invoking server capabilities)
    // =============================================================================

    /// List available tools from the server
    ///
    /// # Errors
    ///
    /// Returns [`McpClientError`] if the request fails or the response cannot
    /// be decoded as a list of [`Tool`]s.
    pub async fn list_tools(&self) -> Result<Vec<Tool>, McpClientError> {
        let result = self.request("tools/list", None).await?;

        let response: ToolsListResponse =
            serde_json::from_value(result).map_err(|e| McpClientError::InvalidResponse {
                expected: "ToolsListResponse".to_string(),
                got: e.to_string(),
            })?;

        Ok(response.tools)
    }

    /// Call a tool on the server
    ///
    /// # Errors
    ///
    /// Returns [`McpClientError`] if the request fails, the arguments cannot
    /// be serialized, or the response cannot be decoded as a
    /// [`CallToolResult`].
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, McpClientError> {
        let params = CallToolParams {
            name: name.to_string(),
            arguments,
            _meta: None,
        };

        let result = self
            .request(
                "tools/call",
                Some(serde_json::to_value(params).map_err(TransportError::Json)?),
            )
            .await?;

        let response: CallToolResult =
            serde_json::from_value(result).map_err(|e| McpClientError::InvalidResponse {
                expected: "CallToolResult".to_string(),
                got: e.to_string(),
            })?;

        Ok(response)
    }

    /// List available resources from the server
    ///
    /// # Errors
    ///
    /// Returns [`McpClientError`] if the request fails or the response cannot
    /// be decoded as a list of [`Resource`]s.
    pub async fn list_resources(&self) -> Result<Vec<Resource>, McpClientError> {
        let result = self.request("resources/list", None).await?;

        let response: ResourcesListResponse =
            serde_json::from_value(result).map_err(|e| McpClientError::InvalidResponse {
                expected: "ResourcesListResponse".to_string(),
                got: e.to_string(),
            })?;

        Ok(response.resources)
    }

    /// List available prompts from the server
    ///
    /// # Errors
    ///
    /// Returns [`McpClientError`] if the request fails or the response cannot
    /// be decoded as a list of [`Prompt`]s.
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>, McpClientError> {
        let result = self.request("prompts/list", None).await?;

        let response: PromptsListResponse =
            serde_json::from_value(result).map_err(|e| McpClientError::InvalidResponse {
                expected: "PromptsListResponse".to_string(),
                got: e.to_string(),
            })?;

        Ok(response.prompts)
    }

    /// Get a specific prompt by name
    ///
    /// # Errors
    ///
    /// Returns [`McpClientError`] if the request fails or the response cannot
    /// be decoded as a `GetPromptResult`.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<HashMap<String, String>>,
    ) -> Result<GetPromptResult, McpClientError> {
        let params = GetPromptParams {
            name: name.to_string(),
            arguments,
        };

        let result = self
            .request(
                "prompts/get",
                Some(serde_json::to_value(params).map_err(TransportError::Json)?),
            )
            .await?;

        let response: GetPromptResult =
            serde_json::from_value(result).map_err(|e| McpClientError::InvalidResponse {
                expected: "GetPromptResult".to_string(),
                got: e.to_string(),
            })?;

        Ok(response)
    }

    /// Check if transport is connected
    pub fn is_connected(&self) -> bool {
        match &self.transport {
            McpTransport::Stdio(t) => t.is_connected(),
            McpTransport::Channel(t) => t.is_connected(),
        }
    }

    /// Close the transport
    ///
    /// After this returns, [`McpClient::is_connected`] reports `false` and any
    /// further request fails with
    /// [`McpClientError::Transport`](`TransportError::NotConnected`).
    ///
    /// A stdio server process is not signalled directly; it exits when the
    /// transport is dropped and its stdin pipe closes.
    pub fn shutdown(&self) {
        match &self.transport {
            McpTransport::Stdio(t) => t.set_connected(false),
            McpTransport::Channel(t) => t.set_connected(false),
        }
    }

    /// Take the next notification the server sent, if one is queued
    ///
    /// Notifications that arrive while the client is correlating a response are
    /// buffered here; the queue is bounded and drops notifications once the
    /// capacity (`NOTIFICATION_QUEUE_CAPACITY`) is reached. This returns
    /// immediately rather than waiting, because notifications are only queued
    /// while a request is in flight.
    pub async fn next_notification(&self) -> Option<JsonRpcNotification> {
        self.notifications.lock().await.try_recv().ok()
    }

    /// Get server capabilities (after initialization)
    pub async fn get_server_capabilities(&self) -> Option<ServerCapabilities> {
        let caps = self.server_capabilities.read().await;
        caps.clone()
    }
}

// =============================================================================
// Response Types
// =============================================================================

/// The echoed request id carried by a JSON-RPC response
fn response_id(response: &JsonRpcResponse) -> &serde_json::Value {
    match response {
        JsonRpcResponse::Success { id, .. } | JsonRpcResponse::Error { id, .. } => id,
    }
}

/// What the next event produced while waiting for a response
enum Step {
    /// The awaited response arrived on its correlated channel.
    Correlated(JsonRpcResponse),
    /// The next transport message still needs dispatching.
    Message(JsonRpcMessage),
}

/// Tools list response
#[derive(Debug, serde::Deserialize)]
pub struct ToolsListResponse {
    /// The tools advertised by the server.
    pub tools: Vec<Tool>,
}

/// Resources list response
#[derive(Debug, serde::Deserialize)]
pub struct ResourcesListResponse {
    /// The resources advertised by the server.
    pub resources: Vec<Resource>,
}

/// Prompts list response
#[derive(Debug, serde::Deserialize)]
pub struct PromptsListResponse {
    /// The prompts advertised by the server.
    pub prompts: Vec<Prompt>,
}

/// Get prompt parameters
#[derive(Debug, serde::Serialize)]
pub struct GetPromptParams {
    /// Name of the prompt to retrieve.
    pub name: String,
    /// Arguments to substitute into the prompt template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, String>>,
}

/// Get prompt result
#[derive(Debug, serde::Deserialize)]
pub struct GetPromptResult {
    /// The rendered prompt messages, in order.
    pub messages: Vec<PromptMessage>,
}

/// Prompt message
#[derive(Debug, serde::Deserialize)]
pub struct PromptMessage {
    /// The speaker role, such as `"user"` or `"assistant"`.
    pub role: String,
    /// The message body.
    pub content: ContentBlock,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_client_config_default() {
        let config = McpClientConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.client_info.name, "agentverify-mcp");
        assert_eq!(config.client_info.version, env!("CARGO_PKG_VERSION"));
        assert!(config.command.is_empty());
        assert!(config.args.is_empty());
        assert!(config.capabilities.sampling.is_none());
    }

    #[test]
    fn error_messages_are_stable() {
        let transport = McpClientError::Transport(TransportError::NotConnected);
        assert_eq!(transport.to_string(), "Transport error: Not connected");

        let timeout = McpClientError::Timeout;
        assert_eq!(timeout.to_string(), "Request timeout");

        let not_initialized = McpClientError::NotInitialized;
        assert_eq!(not_initialized.to_string(), "Not initialized");

        let json_rpc = McpClientError::JsonRpc(JsonRpcError::new(-32601, "no such method"));
        assert_eq!(
            json_rpc.to_string(),
            "JSON-RPC error: JSON-RPC error -32601: no such method"
        );

        let server = McpClientError::ServerError {
            code: -32022,
            message: "unsupported version".to_string(),
        };
        assert_eq!(
            server.to_string(),
            "Server returned error: -32022 unsupported version"
        );

        let invalid = McpClientError::InvalidResponse {
            expected: "InitializeResult".to_string(),
            got: "missing field `serverInfo`".to_string(),
        };
        assert_eq!(
            invalid.to_string(),
            "Invalid response: expected InitializeResult, got missing field `serverInfo`"
        );

        let capability = McpClientError::CapabilityNotSupported("sampling".to_string());
        assert_eq!(capability.to_string(), "Capability not supported: sampling");

        let channel = McpClientError::Channel("Receiver dropped".to_string());
        assert_eq!(channel.to_string(), "Channel error: Receiver dropped");
    }

    #[test]
    fn transport_errors_convert_into_client_errors() {
        let err: McpClientError = TransportError::NotConnected.into();
        assert!(matches!(
            err,
            McpClientError::Transport(TransportError::NotConnected)
        ));

        let parse = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: McpClientError = TransportError::from(parse).into();
        assert!(matches!(
            err,
            McpClientError::Transport(TransportError::Json(_))
        ));

        let io = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe closed");
        let err: McpClientError = TransportError::from(io).into();
        assert!(
            matches!(err, McpClientError::Transport(TransportError::Io(ref e))
                if e.kind() == std::io::ErrorKind::BrokenPipe)
        );
    }

    #[test]
    fn response_ids_are_read_from_both_response_shapes() {
        let success = JsonRpcResponse::Success {
            jsonrpc: "2.0".to_string(),
            id: json!(3),
            result: json!({}),
        };
        let error = JsonRpcResponse::Error {
            jsonrpc: "2.0".to_string(),
            id: json!("abc"),
            error: JsonRpcError::new(-32603, "boom"),
        };

        assert_eq!(response_id(&success), &json!(3));
        assert_eq!(response_id(&error), &json!("abc"));
        // Numeric ids match the client's own request ids.
        assert_eq!(response_id(&success).as_u64(), Some(3));
        assert_eq!(response_id(&error).as_u64(), None);
    }
}
