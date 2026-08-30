//! MCP Client Implementation
//!
//! Provides a high-level async client for interacting with MCP servers.
//! Handles request/response correlation, capability negotiation, and
//! protocol method invocations.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch, RwLock};
use tokio::time::{timeout, Duration};

use crate::protocol::{
    CallToolParams, CallToolResult, ClientCapabilities, ContentBlock, Implementation,
    InitializeParams, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, Prompt, Resource, ServerCapabilities, Tool,
    MCP_PROTOCOL_VERSION,
};
use crate::transport::{ChannelTransport, StdioTransport, TransportError};

/// MCP Client error types
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("JSON-RPC error: {0}")]
    JsonRpc(JsonRpcError),

    #[error("Request timeout")]
    Timeout,

    #[error("Not initialized")]
    NotInitialized,

    #[error("Server returned error: {code} {message}")]
    ServerError { code: i32, message: String },

    #[error("Invalid response: expected {expected}, got {got}")]
    InvalidResponse { expected: String, got: String },

    #[error("Capability not supported: {0}")]
    CapabilityNotSupported(String),

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

/// Pending request handle for correlating responses
struct PendingRequest {
    _response_tx: oneshot::Sender<JsonRpcResponse>,
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
    #[allow(dead_code)]
    notification_tx: mpsc::Sender<JsonRpcNotification>,
    _shutdown_tx: Arc<watch::Sender<bool>>,
}

impl McpClient {
    /// Connect to an MCP server via stdio
    pub async fn connect(config: McpClientConfig) -> Result<Self, McpClientError> {
        let args_refs: Vec<&str> = config.args.iter().map(String::as_str).collect();
        let transport = StdioTransport::connect(&config.command, &args_refs)
            .await
            .map_err(McpClientError::Transport)?;

        Self::with_transport(config, McpTransport::Stdio(transport)).await
    }

    /// Create client with a custom channel transport (for testing)
    pub async fn with_channel_transport(config: McpClientConfig) -> Result<Self, McpClientError> {
        let transport = ChannelTransport::channel();
        let transport = McpTransport::Channel(transport.0);
        Self::with_transport(config, transport).await
    }

    /// Create client with a transport enum
    async fn with_transport(
        config: McpClientConfig,
        transport: McpTransport,
    ) -> Result<Self, McpClientError> {
        let (notification_tx, _) = mpsc::channel(100);
        let (shutdown_tx, _) = watch::channel(false);

        Ok(Self {
            transport,
            config,
            server_capabilities: Arc::new(RwLock::new(None)),
            next_request_id: Arc::new(RwLock::new(1)),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            notification_tx,
            _shutdown_tx: Arc::new(shutdown_tx),
        })
    }

    /// Initialize the MCP connection and negotiate capabilities
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
    #[allow(dead_code)]
    async fn recv_message(&self) -> Result<JsonRpcMessage, McpClientError> {
        match &self.transport {
            McpTransport::Stdio(t) => t.recv().await.map_err(Into::into),
            McpTransport::Channel(t) => t.recv().await.map_err(Into::into),
        }
    }

    /// Send a JSON-RPC request and wait for response
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
        let (response_tx, response_rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending_requests.write().await;
            pending.insert(
                request_id,
                PendingRequest {
                    _response_tx: response_tx,
                },
            );
        }

        // Send request
        self.send_message(JsonRpcMessage::Request(request)).await?;

        // Wait for response with timeout
        let timeout_result: Result<
            Result<JsonRpcResponse, oneshot::error::RecvError>,
            tokio::time::error::Elapsed,
        > = timeout(Duration::from_secs(self.config.timeout_secs), response_rx).await;

        // Remove pending request
        {
            let mut pending = self.pending_requests.write().await;
            pending.remove(&request_id);
        }

        let response: JsonRpcResponse = match timeout_result {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => {
                return Err(McpClientError::Channel(
                    "Response receiver dropped".to_string(),
                ))
            }
            Err(_) => return Err(McpClientError::Timeout),
        };

        match response {
            JsonRpcResponse::Success { result, .. } => Ok(result),
            JsonRpcResponse::Error { error, .. } => Err(McpClientError::JsonRpc(error)),
        }
    }

    // =============================================================================
    // Server Features (as client invoking server capabilities)
    // =============================================================================

    /// List available tools from the server
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

    /// Get server capabilities (after initialization)
    pub async fn get_server_capabilities(&self) -> Option<ServerCapabilities> {
        let caps = self.server_capabilities.read().await;
        caps.clone()
    }
}

// =============================================================================
// Response Types
// =============================================================================

/// Tools list response
#[derive(Debug, serde::Deserialize)]
pub struct ToolsListResponse {
    pub tools: Vec<Tool>,
}

/// Resources list response
#[derive(Debug, serde::Deserialize)]
pub struct ResourcesListResponse {
    pub resources: Vec<Resource>,
}

/// Prompts list response
#[derive(Debug, serde::Deserialize)]
pub struct PromptsListResponse {
    pub prompts: Vec<Prompt>,
}

/// Get prompt parameters
#[derive(Debug, serde::Serialize)]
pub struct GetPromptParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, String>>,
}

/// Get prompt result
#[derive(Debug, serde::Deserialize)]
pub struct GetPromptResult {
    pub messages: Vec<PromptMessage>,
}

/// Prompt message
#[derive(Debug, serde::Deserialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: ContentBlock,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = McpClientConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.client_info.name, "agentverify-mcp");
    }
}
