//! AgentVerify MCP Client
//!
//! Provides a Model Context Protocol (MCP) client implementation for connecting
//! to MCP servers and accessing tools, resources, and prompts.
//!
//! # Protocol
//!
//! This implementation follows the [MCP Specification](https://modelcontextprotocol.io)
//! using JSON-RPC 2.0 over stdio transport.
//!
//! # Example
//!
//! ```ignore
//! use agentverify_mcp::McpClient;
//!
//! let client = McpClient::connect("path/to/mcp-server").await?;
//! let tools = client.list_tools().await?;
//! let result = client.call_tool("my_tool", serde_json::json!({"arg": "value"})).await?;
//! ```

mod client;
mod protocol;
mod transport;

pub use client::{McpClient, McpClientError, McpClientConfig};
pub use protocol::*;
pub use transport::{StdioTransport, ChannelTransport};
