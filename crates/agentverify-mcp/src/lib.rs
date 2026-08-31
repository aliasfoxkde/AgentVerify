//! `AgentVerify` MCP Client
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
//! The initialize handshake must complete before feature calls; the client
//! rejects them with [`McpClientError::NotInitialized`] otherwise, and with
//! [`McpClientError::CapabilityNotSupported`] when the server does not
//! advertise the requested feature.
//!
//! ```ignore
//! use agentverify_mcp::McpClient;
//!
//! let client = McpClient::connect("path/to/mcp-server").await?;
//! client.initialize().await?;
//! let tools = client.list_tools().await?;
//! let result = client.call_tool("my_tool", serde_json::json!({"arg": "value"})).await?;
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
mod client;
mod protocol;
mod transport;

pub use client::{McpClient, McpClientConfig, McpClientError};
pub use protocol::*;
pub use transport::{ChannelTransport, StdioTransport, TransportError};
