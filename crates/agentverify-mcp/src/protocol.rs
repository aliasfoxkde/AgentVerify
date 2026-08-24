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
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Value::Number(id.into()),
            method: method.into(),
            params,
        }
    }

    /// Create a new request with string ID
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
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcResponse {
    /// Successful response with result
    Success {
        jsonrpc: String,
        id: Value,
        result: Value,
    },
    /// Error response
    Error {
        jsonrpc: String,
        id: Value,
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
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create an error with data
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
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    /// Create a new notification
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<bool>,
}

/// Tools capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    // Empty object means tools are supported
}

/// Prompts capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<bool>,
}

// =============================================================================
// MCP Protocol Messages
// =============================================================================

/// Initialize request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: Implementation,
}

/// Initialize request result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Implementation info (client or server)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    pub name: String,
    pub version: String,
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

/// Tool annotations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAnnotations {
    #[serde(rename = "readOnlyHint")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(rename = "destructiveHint")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(rename = "idempotentHint")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(rename = "annotation")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
}

/// Resource definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// Prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
}

/// Prompt argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

/// Call tool request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    pub arguments: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Call tool result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    pub is_error: Option<bool>,
}

/// Content block types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, mime_type: String },
    #[serde(rename = "resource")]
    Resource { resource: ResourceContents },
}

/// Resource contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContents {
    pub uri: String,
    pub mime_type: Option<String>,
    pub text: Option<String>,
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
