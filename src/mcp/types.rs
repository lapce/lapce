//! MCP protocol types — JSON-RPC messages and configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Transport method for MCP connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum McpTransport {
    #[default]
    /// Spawn a subprocess and communicate via stdin/stdout.
    Stdio,
    /// Connect via HTTP SSE (Server-Sent Events).
    Sse,
}

/// Configuration for an MCP server connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique name for this server.
    pub name: String,
    /// Transport method.
    #[serde(default)]
    pub transport: McpTransport,
    /// For stdio: the command to execute.
    pub command: Option<String>,
    /// For stdio: command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// For stdio: environment variables.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// For SSE: the URL to connect to.
    pub url: Option<String>,
    /// Maximum reconnect attempts (-1 = infinite).
    #[serde(default = "default_max_reconnect")]
    pub max_reconnect: i32,
    /// Whether this server is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            transport: McpTransport::Stdio,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            max_reconnect: 3,
            enabled: true,
        }
    }
}

fn default_max_reconnect() -> i32 { 3 }
fn default_true() -> bool { true }

/// Connection status for an MCP server.
#[derive(Debug, Clone, PartialEq)]
pub enum McpConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Failed { reason: String },
}

/// A tool discovered from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// JSON Schema for input parameters.
    #[serde(default)]
    pub input_schema: serde_json::Value,
    /// Which server this tool belongs to.
    #[serde(default, rename = "serverName")]
    pub server_name: String,
}

/// Result of calling an MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallResult {
    /// Text content blocks.
    pub content: Vec<McpContentBlock>,
    /// Whether this result represents an error.
    #[serde(default)]
    pub is_error: bool,
}

/// A content block in an MCP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: String,
}

// ============================================================================
// JSON-RPC Message Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
    #[serde(rename = "capabilities")]
    pub capabilities: ClientCapabilities,
}

#[derive(Debug, Serialize)]
pub struct ClientInfo {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "version")]
    pub version: String,
}

#[derive(Debug, Serialize)]
pub struct ClientCapabilities {
    #[serde(default, rename = "tools")]
    pub tools: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ToolsListResult {
    pub tools: Vec<McpTool>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallParams {
    pub name: String,
    pub arguments: serde_json::Value,
}
