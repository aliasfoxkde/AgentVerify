//! MCP Protocol Types
//!
//! Implements JSON-RPC 2.0 message types as specified in the MCP specification.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// The JSON-RPC version MUST be exactly "2.0"
    pub jsonrpc: String,
    /// Request identifier (MUST NOT be null)
    pub id: Value,
    /// The method to invoke
    pub method: String,
    /// Optional parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Create a new request with numeric ID
    #[must_use]
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Value::Number(id.into()),
            method: method.into(),
            params,
        }
    }

    /// Create a new request with string ID
    #[must_use]
    pub fn with_string_id(
        id: impl Into<String>,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Value::String(id.into()),
            method: method.into(),
            params,
        }
    }
}

/// Unified JSON-RPC message type (request, response, or notification)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// A request awaiting a response.
    Request(JsonRpcRequest),
    /// A reply to a previously issued request.
    Response(JsonRpcResponse),
    /// A one-way message that expects no response.
    Notification(JsonRpcNotification),
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcResponse {
    /// Successful response with result
    Success {
        /// The JSON-RPC version, always `"2.0"`.
        jsonrpc: String,
        /// Echoes the `id` of the request being answered.
        id: Value,
        /// The result payload returned by the peer.
        result: Value,
    },
    /// Error response
    Error {
        /// The JSON-RPC version, always `"2.0"`.
        jsonrpc: String,
        /// Echoes the `id` of the request being answered.
        id: Value,
        /// Structured error information.
        error: JsonRpcError,
    },
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Optional additional data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl JsonRpcError {
    /// Create a new error
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create an error with data
    #[must_use]
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

/// JSON-RPC 2.0 Notification (no id, no response expected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// The JSON-RPC version, always `"2.0"`.
    pub jsonrpc: String,
    /// The method being notified.
    pub method: String,
    /// Optional parameters carried by the notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    /// Create a new notification
    #[must_use]
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

// =============================================================================
// MCP Protocol Error Codes
// =============================================================================

/// MCP Protocol error codes (reserved range -32020 to -32099)
pub mod error_codes {
    /// Legacy error code - DO NOT USE
    pub const LEGACY_RANGE_START: i32 = -32000;
    /// Legacy error code - DO NOT USE
    pub const LEGACY_RANGE_END: i32 = -32019;

    /// Base for MCP specification errors
    pub const MCP_ERROR_BASE: i32 = -32020;

    /// Header mismatch error
    pub const HEADER_MISMATCH: i32 = -32020;
    /// Missing required client capability
    pub const MISSING_REQUIRED_CLIENT_CAPABILITY: i32 = -32021;
    /// Unsupported protocol version
    pub const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;

    // Standard JSON-RPC 2.0 error codes
    /// Parse error - Invalid JSON
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid request - Not valid JSON-RPC 2.0
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error
    pub const INTERNAL_ERROR: i32 = -32603;
}

// =============================================================================
// MCP Protocol Versions
// =============================================================================

/// Supported MCP protocol version
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";

// =============================================================================
// MCP Capability Definitions
// =============================================================================

/// Client capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    /// Sampling capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
    /// Elicitation capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<Value>,
    /// Roots capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<Value>,
}

/// Server capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Resources capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    /// Tools capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    /// Prompts capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
}

/// Resources capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesCapability {
    /// Whether the server supports subscribing to resource changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    /// Whether the server supports listing resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<bool>,
}

/// Tools capability
///
/// An empty object indicates that the server exposes tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {}

/// Prompts capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsCapability {
    /// Whether the server supports listing prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<bool>,
}

// =============================================================================
// MCP Protocol Messages
// =============================================================================

/// Initialize request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// The newest MCP protocol version the client supports.
    pub protocol_version: String,
    /// Capabilities the client declares to the server.
    pub capabilities: ClientCapabilities,
    /// Name and version of the client implementation.
    pub client_info: Implementation,
}

/// Initialize request result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The MCP protocol version the server selected.
    pub protocol_version: String,
    /// Capabilities the server declares to the client.
    pub capabilities: ServerCapabilities,
    /// Name and version of the server implementation.
    pub server_info: Implementation,
    /// Optional human-readable usage instructions from the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Implementation info (client or server)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// The product name of the implementation.
    pub name: String,
    /// The version string of the implementation.
    pub version: String,
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Unique name used to invoke the tool.
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's `arguments`.
    pub input_schema: Value,
    /// Optional behavioural hints about the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

/// Tool annotations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAnnotations {
    /// Hint that the tool does not modify its environment.
    #[serde(rename = "readOnlyHint")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// Hint that the tool may perform destructive updates.
    #[serde(rename = "destructiveHint")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Hint that repeating the tool call with the same arguments is safe.
    #[serde(rename = "idempotentHint")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// Free-form annotation string.
    #[serde(rename = "annotation")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
}

/// Resource definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// URI identifying the resource.
    pub uri: String,
    /// Human-readable display name.
    pub name: String,
    /// Optional description of the resource contents.
    pub description: Option<String>,
    /// Optional MIME type of the resource contents.
    pub mime_type: Option<String>,
}

/// Prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    /// Unique name used to retrieve the prompt.
    pub name: String,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Optional list of arguments the prompt accepts.
    pub arguments: Option<Vec<PromptArgument>>,
}

/// Prompt argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Name used to reference the argument in the prompt template.
    pub name: String,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Whether the argument must be supplied.
    pub required: bool,
}

/// Call tool request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    /// Name of the tool to invoke.
    pub name: String,
    /// Arguments passed to the tool, shaped by its input schema.
    pub arguments: Value,
    /// Optional MCP metadata attached to the call.
    ///
    /// The leading underscore is part of the MCP wire format field name and
    /// cannot be removed without breaking interoperability.
    #[allow(clippy::pub_underscore_fields)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Call tool result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    /// Content blocks produced by the tool.
    pub content: Vec<ContentBlock>,
    /// Whether the tool reported an error while executing.
    pub is_error: Option<bool>,
}

/// Content block types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// A plain text block.
    #[serde(rename = "text")]
    Text {
        /// The text content.
        text: String,
    },
    /// A base64-encoded binary block.
    #[serde(rename = "image")]
    Image {
        /// Base64-encoded image data.
        data: String,
        /// MIME type of the image data.
        mime_type: String,
    },
    /// An embedded resource.
    #[serde(rename = "resource")]
    Resource {
        /// Contents of the embedded resource.
        resource: ResourceContents,
    },
}

/// Resource contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContents {
    /// URI identifying the resource.
    pub uri: String,
    /// Optional MIME type of the contents.
    pub mime_type: Option<String>,
    /// Text payload, present when the resource is textual.
    pub text: Option<String>,
    /// Base64-encoded payload, present when the resource is binary.
    pub blob: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"tools/list\""));
    }

    #[test]
    fn test_notification_serialization() {
        let notif = JsonRpcNotification::new("initialized", None);
        let json = serde_json::to_string(&notif).unwrap();
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::INVALID_REQUEST, -32600);
        assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_codes::INVALID_PARAMS, -32602);
        assert_eq!(error_codes::INTERNAL_ERROR, -32603);
    }
}
