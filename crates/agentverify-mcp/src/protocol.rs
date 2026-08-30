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
    use serde_json::json;

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

    #[test]
    fn request_omits_absent_params() {
        let req = JsonRpcRequest::new(7, "tools/list", None);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 7);
        assert_eq!(json["method"], "tools/list");
        assert!(
            json.get("params").is_none(),
            "absent params must be omitted"
        );
    }

    #[test]
    fn request_roundtrips_numeric_id_and_params() {
        let req = JsonRpcRequest::new(12, "tools/call", Some(json!({"name": "t"})));
        let json = serde_json::to_string(&req).unwrap();
        let back: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, json!(12));
        assert_eq!(back.method, "tools/call");
        assert_eq!(back.params.unwrap()["name"], "t");
    }

    #[test]
    fn request_roundtrips_string_id() {
        let req = JsonRpcRequest::with_string_id("session-9", "initialize", None);
        let json = serde_json::to_string(&req).unwrap();
        let back: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, json!("session-9"));
    }

    #[test]
    fn request_accepts_unknown_fields() {
        let back: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"m","extra":42}"#).unwrap();
        assert_eq!(back.method, "m");
        assert!(back.params.is_none());
    }

    #[test]
    fn message_deserializes_each_shape() {
        let request: JsonRpcMessage =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#).unwrap();
        assert!(matches!(request, JsonRpcMessage::Request(_)));

        let response: JsonRpcMessage =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[]}}"#).unwrap();
        assert!(matches!(response, JsonRpcMessage::Response(_)));

        let notification: JsonRpcMessage =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"initialized"}"#).unwrap();
        assert!(matches!(notification, JsonRpcMessage::Notification(_)));
    }

    #[test]
    fn message_roundtrips_every_variant() {
        for message in [
            JsonRpcMessage::Request(JsonRpcRequest::new(1, "tools/list", None)),
            JsonRpcMessage::Response(JsonRpcResponse::Success {
                jsonrpc: "2.0".to_string(),
                id: json!(1),
                result: json!({"tools": []}),
            }),
            JsonRpcMessage::Notification(JsonRpcNotification::new("initialized", None)),
        ] {
            let wire = serde_json::to_string(&message).unwrap();
            let back: JsonRpcMessage = serde_json::from_str(&wire).unwrap();
            let rewire = serde_json::to_string(&back).unwrap();
            assert_eq!(wire, rewire);
        }
    }

    #[test]
    fn message_rejects_non_json_rpc_payloads() {
        assert!(serde_json::from_str::<JsonRpcMessage>("{}").is_err());
        assert!(serde_json::from_str::<JsonRpcMessage>("null").is_err());
        assert!(serde_json::from_str::<JsonRpcMessage>("[1,2]").is_err());
    }

    /// The id and result of a successful response.
    fn success_of(response: JsonRpcResponse) -> Option<(Value, Value)> {
        match response {
            JsonRpcResponse::Success { id, result, .. } => Some((id, result)),
            JsonRpcResponse::Error { .. } => None,
        }
    }

    /// The error of an error response.
    fn error_of(response: JsonRpcResponse) -> Option<JsonRpcError> {
        match response {
            JsonRpcResponse::Error { error, .. } => Some(error),
            JsonRpcResponse::Success { .. } => None,
        }
    }

    /// The contents of an embedded-resource content block.
    fn embedded_resource_of(block: ContentBlock) -> Option<ResourceContents> {
        match block {
            ContentBlock::Resource { resource } => Some(resource),
            _ => None,
        }
    }

    #[test]
    fn response_success_roundtrips() {
        let response = JsonRpcResponse::Success {
            jsonrpc: "2.0".to_string(),
            id: json!(4),
            result: json!({"ok": true}),
        };
        let json = serde_json::to_string(&response).unwrap();
        let back: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        let (id, result) = success_of(back).expect("success must not decode as an error");
        assert_eq!(id, json!(4));
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn response_error_roundtrips() {
        let response = JsonRpcResponse::Error {
            jsonrpc: "2.0".to_string(),
            id: json!(5),
            error: JsonRpcError::with_data(-32601, "no such method", json!({"hint": "list tools"})),
        };
        let json = serde_json::to_string(&response).unwrap();
        let back: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        let error = error_of(back).expect("error must not decode as a success");
        assert_eq!(error.code, -32601);
        assert_eq!(error.data.unwrap()["hint"], "list tools");
    }

    #[test]
    fn response_error_omits_absent_data() {
        let response = JsonRpcResponse::Error {
            jsonrpc: "2.0".to_string(),
            id: Value::Null,
            error: JsonRpcError::new(-32603, "boom"),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert!(json["error"].get("data").is_none());
        assert_eq!(json["id"], Value::Null);
    }

    #[test]
    fn error_display_includes_code_and_message() {
        let error = JsonRpcError::new(-32601, "method not found");
        assert_eq!(error.to_string(), "JSON-RPC error -32601: method not found");
    }

    #[test]
    fn error_constructors_set_data() {
        let plain = JsonRpcError::new(1, "a");
        assert!(plain.data.is_none());
        assert_eq!(plain.code, 1);
        assert_eq!(plain.message, "a");

        let with_data = JsonRpcError::with_data(2, "b", json!([1, 2]));
        assert_eq!(with_data.data.unwrap(), json!([1, 2]));
    }

    #[test]
    fn notification_carries_params() {
        let notif =
            JsonRpcNotification::new("notifications/progress", Some(json!({"progress": 1})));
        let json = serde_json::to_string(&notif).unwrap();
        let back: JsonRpcNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.method, "notifications/progress");
        assert_eq!(back.params.unwrap()["progress"], 1);
        assert_eq!(back.jsonrpc, "2.0");
    }

    #[test]
    fn mcp_error_code_range_constants_agree_with_the_spec() {
        assert_eq!(error_codes::MCP_ERROR_BASE, -32020);
        assert_eq!(error_codes::HEADER_MISMATCH, -32020);
        assert_eq!(error_codes::MISSING_REQUIRED_CLIENT_CAPABILITY, -32021);
        assert_eq!(error_codes::UNSUPPORTED_PROTOCOL_VERSION, -32022);
        assert_eq!(error_codes::LEGACY_RANGE_START, -32000);
        assert_eq!(error_codes::LEGACY_RANGE_END, -32019);
        assert_eq!(MCP_PROTOCOL_VERSION, "2026-07-28");
    }

    #[test]
    fn client_capabilities_roundtrip() {
        let empty = ClientCapabilities::default();
        let json = serde_json::to_value(&empty).unwrap();
        assert_eq!(json, json!({}), "absent capabilities must not be emitted");
        let back: ClientCapabilities = serde_json::from_value(json).unwrap();
        assert!(back.sampling.is_none() && back.elicitation.is_none() && back.roots.is_none());

        let full = ClientCapabilities {
            sampling: Some(json!({})),
            elicitation: Some(json!({})),
            roots: Some(json!({"listChanged": true})),
        };
        let json = serde_json::to_value(&full).unwrap();
        assert_eq!(json["roots"]["listChanged"], true);
        let back: ClientCapabilities = serde_json::from_value(json).unwrap();
        assert!(back.roots.is_some());
    }

    #[test]
    fn server_capabilities_roundtrip() {
        let caps = ServerCapabilities {
            resources: Some(ResourcesCapability {
                subscribe: Some(true),
                list: None,
            }),
            tools: Some(ToolsCapability {}),
            prompts: Some(PromptsCapability { list: Some(false) }),
        };
        let json = serde_json::to_value(&caps).unwrap();
        assert_eq!(json["tools"], json!({}));
        assert_eq!(json["resources"]["subscribe"], true);
        assert!(json["resources"].get("list").is_none());
        assert_eq!(json["prompts"]["list"], false);

        let back: ServerCapabilities = serde_json::from_value(json).unwrap();
        assert_eq!(back.resources.unwrap().subscribe, Some(true));
        assert_eq!(back.prompts.unwrap().list, Some(false));
    }

    #[test]
    fn initialize_params_and_result_roundtrip() {
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "agentverify-mcp".to_string(),
                version: "0.1.0".to_string(),
            },
        };
        let json = serde_json::to_string(&params).unwrap();
        let back: InitializeParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back.client_info.name, "agentverify-mcp");

        let result = InitializeResult {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                resources: None,
                tools: Some(ToolsCapability {}),
                prompts: None,
            },
            server_info: Implementation {
                name: "srv".to_string(),
                version: "1.0.0".to_string(),
            },
            instructions: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("instructions"),
            "absent instructions are omitted"
        );
        let back: InitializeResult = serde_json::from_str(&json).unwrap();
        assert!(back.instructions.is_none());
        assert!(back.capabilities.tools.is_some());
    }

    #[test]
    fn tool_roundtrips_with_and_without_annotations() {
        let tool = Tool {
            name: "t".to_string(),
            description: "d".to_string(),
            input_schema: json!({"type": "object"}),
            annotations: Some(ToolAnnotations {
                read_only_hint: Some(true),
                destructive_hint: None,
                idempotent_hint: Some(false),
                annotation: None,
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        // Annotations use the camelCase names from the MCP specification.
        assert_eq!(json["annotations"]["readOnlyHint"], true);
        assert_eq!(json["annotations"]["idempotentHint"], false);
        assert!(json["annotations"].get("destructiveHint").is_none());
        let back: Tool = serde_json::from_value(json).unwrap();
        assert_eq!(back.annotations.unwrap().read_only_hint, Some(true));

        let bare = Tool {
            name: "t".to_string(),
            description: "d".to_string(),
            input_schema: json!({"type": "object"}),
            annotations: None,
        };
        let json = serde_json::to_value(&bare).unwrap();
        assert!(json.get("annotations").is_none());
        let back: Tool = serde_json::from_value(json).unwrap();
        assert!(back.annotations.is_none());
    }

    #[test]
    fn resource_roundtrips_optional_fields() {
        let resource = Resource {
            uri: "file:///a.json".to_string(),
            name: "a".to_string(),
            description: Some("desc".to_string()),
            mime_type: Some("application/json".to_string()),
        };
        let json = serde_json::to_string(&resource).unwrap();
        let back: Resource = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mime_type.as_deref(), Some("application/json"));

        let bare: Resource = serde_json::from_str(r#"{"uri":"u","name":"n"}"#).unwrap();
        assert!(bare.description.is_none() && bare.mime_type.is_none());
    }

    #[test]
    fn prompt_and_argument_roundtrip() {
        let prompt = Prompt {
            name: "p".to_string(),
            description: Some("d".to_string()),
            arguments: Some(vec![PromptArgument {
                name: "order_id".to_string(),
                description: None,
                required: true,
            }]),
        };
        let json = serde_json::to_string(&prompt).unwrap();
        let back: Prompt = serde_json::from_str(&json).unwrap();
        let argument = &back.arguments.unwrap()[0];
        assert_eq!(argument.name, "order_id");
        assert!(argument.required);
        assert!(argument.description.is_none());

        let bare: Prompt = serde_json::from_str(r#"{"name":"p"}"#).unwrap();
        assert!(bare.description.is_none() && bare.arguments.is_none());
    }

    #[test]
    fn call_tool_params_and_result_roundtrip() {
        let params = CallToolParams {
            name: "t".to_string(),
            arguments: json!({"order_id": "A-1"}),
            _meta: Some(json!({"progressToken": 1})),
        };
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["name"], "t");
        assert_eq!(json["_meta"]["progressToken"], 1);
        let back: CallToolParams = serde_json::from_value(json).unwrap();
        assert_eq!(back.arguments["order_id"], "A-1");

        let without_meta = CallToolParams {
            name: "t".to_string(),
            arguments: json!({}),
            _meta: None,
        };
        let json = serde_json::to_value(&without_meta).unwrap();
        assert!(json.get("_meta").is_none());

        let result = CallToolResult {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            is_error: Some(true),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["is_error"], true);
        assert_eq!(json["content"][0]["type"], "text");
        let back: CallToolResult = serde_json::from_value(json).unwrap();
        assert_eq!(back.is_error, Some(true));
    }

    #[test]
    fn content_block_decodes_every_tagged_variant() {
        let text: ContentBlock =
            serde_json::from_value(json!({"type": "text", "text": "hi"})).unwrap();
        assert!(matches!(text, ContentBlock::Text { .. }));

        let image: ContentBlock = serde_json::from_value(
            json!({"type": "image", "data": "aGk=", "mime_type": "image/png"}),
        )
        .unwrap();
        assert!(matches!(image, ContentBlock::Image { .. }));

        let resource: ContentBlock = serde_json::from_value(json!({
            "type": "resource",
            "resource": {"uri": "file:///a", "text": "body", "blob": null}
        }))
        .unwrap();
        let embedded = embedded_resource_of(resource).expect("expected an embedded resource");
        assert_eq!(embedded.uri, "file:///a");
        assert_eq!(embedded.text.as_deref(), Some("body"));
        assert!(embedded.blob.is_none());
        assert!(embedded.mime_type.is_none());

        // Every variant also survives a full round trip.
        for block in [
            ContentBlock::Text {
                text: "hi".to_string(),
            },
            ContentBlock::Image {
                data: "aGk=".to_string(),
                mime_type: "image/png".to_string(),
            },
            ContentBlock::Resource {
                resource: ResourceContents {
                    uri: "file:///a".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    text: None,
                    blob: Some("Ym9keQ==".to_string()),
                },
            },
        ] {
            let json = serde_json::to_value(&block).unwrap();
            let back: ContentBlock = serde_json::from_value(json).unwrap();
            assert_eq!(
                serde_json::to_string(&back).unwrap(),
                serde_json::to_string(&block).unwrap()
            );
        }
    }

    #[test]
    fn response_helpers_discriminate_shapes() {
        let success = JsonRpcResponse::Success {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            result: json!({}),
        };
        let error = JsonRpcResponse::Error {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            error: JsonRpcError::new(-32603, "boom"),
        };

        assert!(success_of(error).is_none());
        assert!(error_of(success).is_none());
        assert!(embedded_resource_of(ContentBlock::Text {
            text: "body".to_string()
        })
        .is_none());
    }

    #[test]
    fn content_block_rejects_an_unknown_tag() {
        assert!(serde_json::from_value::<ContentBlock>(json!({"type": "audio"})).is_err());
        assert!(serde_json::from_value::<ContentBlock>(json!("text")).is_err());
    }
}
