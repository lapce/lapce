//! MCP (Model Context Protocol) client — Claude Code compatible.
//!
//! Implements the MCP JSON-RPC protocol for connecting to external tool servers.
//! Supports stdio and SSE transports. Discovers tools, calls them, and reconnects.
//!
//! ## Protocol (simplified)
//!
//! ```text
//! Client                          Server
//!   |── initialize ────────────────→|
//!   |←── capabilities {tools, ...}─|
//!   |── tools/list ────────────────→|
//!   |←── [Tool, ...] ──────────────|
//!   |── tools/call {name,args} ───→|
//!   |←── {content: [{type:text,text}]} |
//! ```

pub mod active_invoker;
pub mod client;
pub mod types;
pub mod orchestration;
pub mod server;

pub use client::McpClient;
pub use client::{ReconnectPolicy, McpConnectionStats, SseTransport};
pub use types::{
    McpTool, McpServerConfig, McpTransport, McpConnectionStatus,
    McpToolCallResult, McpContentBlock,
};
pub use server::DeepseekMcpServer;
