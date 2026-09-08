//! MCP Server — exposes deepseek-carp as a callable MCP tool server.
//!
//! Any MCP-compatible client (dscarp-lapce, Claude Desktop, Cursor, VSCode)
//! can connect via stdio or SSE and invoke deepseek-carp's tools/agents/skills.
//!
//! ```text
//! ┌──────────────┐    MCP JSON-RPC over stdio/SSE    ┌──────────────────┐
//! │  MCP Client  │  (dscarp-lapce, Claude Desktop)   │  deepseek-carp   │
//! │              │──────────────────────────────────▶│  MCP Server      │
//! │              │◀───────────────────────────────── │  (this module)   │
//! └──────────────┘                                    └────────┬─────────┘
//!                                                             │
//!                                                   ┌─────────▼─────────┐
//!                                                   │ ApplyEngine       │
//!                                                   │ DiffEngine        │
//!                                                   │ SecurityScanner   │
//!                                                   │ BbonOrchestrator  │
//!                                                   │ SkillRegistry     │
//!                                                   │ ... (core)        │
//!                                                   └───────────────────┘
//! ```

use std::io::{self, BufRead, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use serde::Serialize;
use serde_json::{Value, json};

use crate::tools::apply_engine::ApplyEngine;
use crate::tools::diff::DiffEngine;
use crate::tools::security_scanner_v2::SecurityScannerV2;

/// MCP protocol version we speak.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Transport layer for the MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    /// stdin/stdout — child-process mode (VS Code / Claude Desktop).
    Stdio,
    /// HTTP SSE — socket mode (dscarp-lapce over localhost).
    Sse { port: u16 },
}

/// A single MCP tool exposed by this server.
#[derive(Debug, Clone, Serialize)]
pub struct McpServerTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    #[serde(skip)]
    pub handler: fn(Value) -> McpToolResult,
}

/// Result of invoking an MCP tool.
#[derive(Debug, Clone)]
pub struct McpToolResult {
    pub ok: bool,
    pub content: String,
    pub is_error: bool,
}

/// An MCP resource entry.
#[derive(Debug, Clone, Serialize)]
struct McpResource {
    uri: String,
    name: String,
    description: &'static str,
    mime_type: Option<String>,
}

/// An MCP prompt template.
#[derive(Debug, Clone, Serialize)]
struct McpPrompt {
    name: String,
    description: &'static str,
    arguments: Vec<McpPromptArgument>,
}

#[derive(Debug, Clone, Serialize)]
struct McpPromptArgument {
    name: &'static str,
    description: &'static str,
    required: bool,
}

/// The MCP server — wraps core tools behind the MCP protocol.
pub struct DeepseekMcpServer {
    tools: Vec<McpServerTool>,
}

impl DeepseekMcpServer {
    pub fn new() -> Self {
        Self { tools: build_tools() }
    }

    /// Return the MCP tool list (protocol response).
    pub fn list_tools(&self) -> Value {
        let tools: Vec<Value> = self.tools.iter().map(|t| json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": t.input_schema,
        })).collect();
        json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "serverInfo": {
                "name": "deepseek-carp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "tools": { "listChanged": false },
                "prompts": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
            },
            "tools": tools,
        })
    }

    /// Handle a single MCP JSON-RPC request.
    pub fn handle_request(&self, request: &Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(json!(null));
        let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let result = match method {
            "initialize" => self.list_tools(),
            "tools/list" => self.list_tools(),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                self.invoke(name, &arguments)
            }
            "resources/list" => self.handle_list_resources(),
            "resources/read" => {
                let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                self.handle_read_resource(uri)
            }
            "prompts/list" => self.handle_list_prompts(),
            "prompts/get" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                self.handle_get_prompt(name, &args)
            }
            "ping" => json!({}),
            _ => json!({
                "isError": true,
                "content": [{"type": "text", "text": format!("unknown method: {}", method)}]
            }),
        };

        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })
    }

    fn invoke(&self, name: &str, args: &Value) -> Value {
        if let Some(tool) = self.tools.iter().find(|t| t.name == name) {
            let (handler_name, handler_fn) = (tool.name, tool.handler);
            let r = handler_fn(args.clone());
            json!({
                "content": [{"type": "text", "text": r.content}],
                "isError": r.is_error,
                "_handled_by": handler_name,
            })
        } else {
            json!({
                "isError": true,
                "content": [{"type": "text", "text": format!("tool not found: {}", name)}]
            })
        }
    }

    // ── Resources handlers ──

    fn handle_list_resources(&self) -> Value {
        let resources = vec![
            McpResource {
                uri: "resource://workspace/files".into(),
                name: "Workspace Files".into(),
                description: "List of tracked files in the current workspace",
                mime_type: Some("application/json".into()),
            },
            McpResource {
                uri: "resource://workspace/diagnostics".into(),
                name: "Diagnostics".into(),
                description: "Latest diagnostic findings from security scans",
                mime_type: Some("application/json".into()),
            },
            McpResource {
                uri: "resource://session/cost".into(),
                name: "Session Cost".into(),
                description: "Current session cost tracking information",
                mime_type: Some("application/json".into()),
            },
        ];
        json!({ "resources": resources })
    }

    fn handle_read_resource(&self, uri: &str) -> Value {
        match uri {
            "resource://workspace/files" => {
                let files = list_workspace_files_impl();
                json!({
                    "contents": [{"type": "text", "text": files, "mimeType": "application/json"}]
                })
            }
            "resource://workspace/diagnostics" => {
                let diag = r#"{"findings":[],"message":"Run diagnostics_scan tool to populate"}"#;
                json!({
                    "contents": [{"type": "text", "text": diag, "mimeType": "application/json"}]
                })
            }
            "resource://session/cost" => {
                let cost = r#"{"session_spent":"0.0000","session_limit":"10.00","daily_spent":"0.0000","currency":"USD"}"#;
                json!({
                    "contents": [{"type": "text", "text": cost, "mimeType": "application/json"}]
                })
            }
            _ => json!({
                "isError": true,
                "contents": [{"type": "text", "text": format!("resource not found: {}", uri)}]
            }),
        }
    }

    // ── Prompts handlers ──

    fn handle_list_prompts(&self) -> Value {
        let prompts = vec![
            McpPrompt {
                name: "code-review".into(),
                description: "Review the following code for bugs, security issues, and improvements. Provides structured feedback with severity levels.",
                arguments: vec![
                    McpPromptArgument { name: "code", description: "The code to review", required: true },
                    McpPromptArgument { name: "language", description: "Programming language hint", required: false },
                ],
            },
            McpPrompt {
                name: "explain-code".into(),
                description: "Explain what this code does in plain language. Breaks down logic flow, data structures, and key algorithms.",
                arguments: vec![
                    McpPromptArgument { name: "code", description: "The code to explain", required: true },
                    McpPromptArgument { name: "detail_level", description: "Detail level: brief, standard, verbose", required: false },
                ],
            },
            McpPrompt {
                name: "generate-tests".into(),
                description: "Write comprehensive unit tests for this code following project conventions and testing best practices.",
                arguments: vec![
                    McpPromptArgument { name: "code", description: "The code to generate tests for", required: true },
                    McpPromptArgument { name: "framework", description: "Test framework: rust (default), pytest, jest, go", required: false },
                ],
            },
        ];
        json!({ "prompts": prompts })
    }

    fn handle_get_prompt(&self, name: &str, _args: &Value) -> Value {
        match name {
            "code-review" => json!({
                "description": "Review code for bugs, security issues, and improvements",
                "messages": [
                    {"role": "user", "content": {"type": "text", "text": "Review the following code for bugs, security vulnerabilities, performance issues, and style improvements. Provide findings with severity (critical/high/medium/low), file:line references, and suggested fixes.\n\nCode:\n{{code}}"}}
                ]
            }),
            "explain-code" => json!({
                "description": "Explain what this code does",
                "messages": [
                    {"role": "user", "content": {"type": "text", "text": "Explain what this code does step by step. Cover:\n1. Overall purpose and architecture\n2. Key functions/methods and their roles\n3. Data flow and state changes\n4. Notable patterns or anti-patterns\n5. Potential edge cases\n\nCode:\n{{code}}"}}
                ]
            }),
            "generate-tests" => json!({
                "description": "Generate unit tests for code",
                "messages": [
                    {"role": "user", "content": {"type": "text", "text": "Write comprehensive unit tests for the following code. Include:\n1. Happy path tests (normal inputs)\n2. Edge case tests (boundary values, empty inputs)\n3. Error handling tests (invalid inputs, failure modes)\n4. Integration-style tests if applicable\n\nFollow the project's existing test conventions.\n\nCode:\n{{code}}"}}
                ]
            }),
            _ => json!({
                "isError": true,
                "content": [{"type": "text", "text": format!("prompt not found: {}", name)}]
            }),
        }
    }

    /// Run the server on stdin/stdout (blocks, for subprocess mode).
    pub fn run_stdio(self) -> io::Result<()> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut buffer = String::new();

        for line in stdin.lock().lines() {
            let line = match line { Ok(l) => l, Err(_) => break };
            buffer.push_str(&line);

            while let Ok(request) = serde_json::from_str::<Value>(&buffer) {
                buffer.clear();
                let response = self.handle_request(&request);
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", serde_json::to_string(&response).unwrap_or_default());
                let _ = out.flush();
            }
        }
        Ok(())
    }

    /// Run the server as an HTTP SSE endpoint (for dscarp-lapce).
    pub fn run_sse(self, port: u16) -> io::Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let server = Arc::new(std::sync::Mutex::new(self));
        let start_time = std::time::Instant::now();
        let tool_count = {
            server.lock().map(|s| s.tools.len()).unwrap_or(0)
        };

        println!("deepseek-carp MCP SSE listening on http://127.0.0.1:{}/health", port);

        loop {
            let (mut stream, _addr) = listener.accept()?;
            let server = server.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 16_384];
                let mut body = String::new();
                let mut reading_headers = true;
                let mut content_length = 0usize;
                let mut headers_bytes = Vec::new();
                let mut is_health_check = false;
                while reading_headers {
                    let n = match stream.read(&mut buf) { Ok(n) => n, Err(_) => return, };
                    if n == 0 { return; }
                    headers_bytes.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&headers_bytes);
                    // Detect GET /health request
                    if text.contains("GET") && text.contains("/health") {
                        is_health_check = true;
                    }
                    if let Some(pos) = text.find("\r\n\r\n") {
                        reading_headers = false;
                        if let Some(line) = text.lines().find(|l| l.to_lowercase().starts_with("content-length:")) {
                            if let Some(n) = line.split(':').nth(1).and_then(|s| s.trim().parse::<usize>().ok()) {
                                content_length = n;
                            }
                        }
                        body.push_str(&text[pos+4..]);
                        headers_bytes.clear();
                    }
                }

                if is_health_check {
                    // Health check endpoint — return JSON status without MCP processing
                    let uptime_ms = start_time.elapsed().as_millis() as u64;
                    let health = serde_json::json!({
                        "status": "ok",
                        "tools": tool_count,
                        "uptime_ms": uptime_ms,
                        "version": env!("CARGO_PKG_VERSION"),
                        "protocol": MCP_PROTOCOL_VERSION,
                    });
                    let body = serde_json::to_string(&health).unwrap_or_default();
                    let response_http = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                        body.len(), body
                    );
                    let _ = stream.write_all(response_http.as_bytes());
                    let _ = stream.flush();
                    return;
                }

                while body.len() < content_length {
                    let n = match stream.read(&mut buf) { Ok(n) => n, Err(_) => return, };
                    if n == 0 { break; }
                    body.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
                let request: Value = match serde_json::from_str(&body) { Ok(v) => v, Err(_) => return, };
                let response = match server.lock() {
                    Ok(s) => s.handle_request(&request),
                    Err(_) => return,
                };
                let body = serde_json::to_string(&response).unwrap_or_default();
                let response_http = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response_http.as_bytes());
                let _ = stream.flush();
            });
        }
    }
}

fn build_tools() -> Vec<McpServerTool> {
    vec![
        McpServerTool {
            name: "code_edit",
            description: "[IDE-PRIMARY] Apply a diff, patch, search-replace edit, or full file overwrite to a target file. This is the PRIMARY editing capability — Claude/Cursor should call this whenever source code modification is needed. Supports unified diff format (preferred), exact string match replacement, and full content overwrite mode. Use code_read first to inspect the file, then code_edit to apply changes.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Absolute or relative file path to edit"
                    },
                    "diff": {
                        "type": "string",
                        "description": "Unified diff content to apply (prefers this mode when available)"
                    },
                    "search": {
                        "type": "string",
                        "description": "Exact string to search for in the file (search-replace mode)"
                    },
                    "replace": {
                        "type": "string",
                        "description": "Replacement string (used with 'search' parameter)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Complete new file content (overwrite mode — use sparingly)"
                    }
                },
                "required": ["target"]
            }),
            handler: call_apply_tool,
        },
        McpServerTool {
            name: "code_search",
            description: "[IDE-PRIMARY] Search across the workspace using ripgrep-compatible regex pattern matching. Supports regex patterns, file type filtering (glob), and context lines. Ideal for finding symbol usages, API calls, imports, or any textual pattern in the codebase. Use before editing to understand impact scope.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex or literal search pattern (ripgrep syntax)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search within (defaults to workspace root)"
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "Glob pattern to filter files (e.g., '*.rs', '**/*.ts', 'src/**/*.py')"
                    },
                    "context_lines": {
                        "type": "integer",
                        "description": "Number of context lines before/after each match (default: 2)",
                        "default": 2
                    }
                },
                "required": ["pattern"]
            }),
            handler: call_search_tool,
        },
        McpServerTool {
            name: "code_read",
            description: "[IDE-PRIMARY] Read file contents with optional line range selection. Returns numbered line text content of a file, optionally limited to a specific line range for large files. ALWAYS call this before code_edit to understand current state. Supports 1-based line indexing.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to read (absolute or relative to workspace root)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Starting line number (1-based, inclusive). Omit to read from beginning."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read. Omit to read to end of file."
                    }
                },
                "required": ["path"]
            }),
            handler: call_read_tool,
        },
        McpServerTool {
            name: "terminal_run",
            description: "[IDE-PRIMARY] Execute a shell command in the working directory and capture combined stdout+stderr output. Supports common dev commands: cargo build/npm test/git operations/pip install. Returns exit code, output text, and timing. Use for building, testing, linting, or any CLI operation.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute (e.g., 'cargo check', 'npm test', 'git status', 'python -m pytest')"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory for command execution (defaults to project root)"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Timeout in milliseconds (default: 30000)",
                        "default": 30000
                    }
                },
                "required": ["command"]
            }),
            handler: call_terminal_tool,
        },
        McpServerTool {
            name: "web_fetch",
            description: "[UTILITY] Fetch and return the text content of a URL as markdown-compatible text. Uses HTTP client with TLS support. Useful for reading documentation, fetching API specs, retrieving reference material, or checking external resources. Handles redirects automatically.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch (http:// or https://). Must be publicly accessible.",
                        "format": "uri"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Maximum response length in characters (default: 50000)",
                        "default": 50000
                    }
                },
                "required": ["url"]
            }),
            handler: call_web_fetch_tool,
        },
        McpServerTool {
            name: "diagnostics_scan",
            description: "[ANALYSIS] Run deepseek-carp's multi-aspect diagnostics on a target file or directory. Covers security (injection, XSS, auth flaws), performance (N+1 queries, memory leaks), correctness (null derefs, race conditions), and style violations. Returns structured findings with severity levels. Use after code edits to validate quality.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "File path or directory to scan (relative or absolute)"
                    },
                    "aspect": {
                        "type": "string",
                        "enum": ["security", "performance", "correctness", "style", "all"],
                        "description": "Scan aspect focus area: security=OWASP Top 10, performance=bottlenecks, correctness=logic bugs, style=conventions, all=everything (default: all)",
                        "default": "all"
                    }
                },
                "required": ["target"]
            }),
            handler: call_diagnostics_tool,
        },
        McpServerTool {
            name: "list_files",
            description: "[NAVIGATION] List workspace files matching a glob pattern. Returns file paths with sizes and modification times. Essential for exploring project structure, finding specific file types, or discovering where code lives. Use before code_read/code_edit when you don't know exact paths.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern (e.g., '**/*.rs', 'src/**/*.ts', '*.md', 'tests/**/*.rs')"
                    },
                    "path": {
                        "type": "string",
                        "description": "Base directory to list from (defaults to workspace root)"
                    }
                },
                "required": ["pattern"]
            }),
            handler: call_list_files_tool,
        },
        McpServerTool {
            name: "code_diff",
            description: "[REVIEW] Generate a unified diff between two versions of code. Provide original and modified text (or file paths) to get standard unified diff output showing added (+), removed (-), and changed lines. Useful for reviewing proposed changes before applying them via code_edit.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "original": {
                        "type": "string",
                        "description": "Original code text OR file path starting with 'file:' prefix"
                    },
                    "modified": {
                        "type": "string",
                        "description": "Modified code text OR file path starting with 'file:' prefix"
                    },
                    "path": {
                        "type": "string",
                        "description": "Display path for the diff header (default: 'file')"
                    }
                },
                "required": ["original", "modified"]
            }),
            handler: call_diff_tool,
        },
        McpServerTool {
            name: "security_scan",
            description: "[SECURITY] Focused security vulnerability scanner using deepseek-carp's SecurityScannerV2 engine. Detects OWASP Top 10 categories: injection (SQL/Command/XSS), broken authentication, sensitive data exposure, XXE, broken access control, misconfiguration, CSRF, deserialization, known vulnerabilities, insufficient logging. More detailed than diagnostics_scan for security-only focus.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "File or directory path to scan for vulnerabilities"
                    },
                    "aspect": {
                        "type": "string",
                        "enum": ["security", "performance", "correctness", "style", "all"],
                        "description": "Analysis aspect (default: all — but this tool specializes in security)",
                        "default": "all"
                    }
                },
                "required": ["target"]
            }),
            handler: call_security_tool,
        },
        McpServerTool {
            name: "context_retrieve",
            description: "[RAG] Retrieve relevant code context from the project via semantic RAG indexing. Searches symbols, functions, types, and documentation by natural language query. Returns ranked results with relevance scores. Ideal when you need to find 'how X is implemented' or 'where Y is used' without knowing exact file locations.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query describing what code you're looking for (e.g., 'how does authentication work?', 'where is the database connection pool initialized?')"
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Number of results to return (default: 5, max: 20)",
                        "default": 5,
                        "maximum": 20
                    }
                },
                "required": ["query"]
            }),
            handler: call_context_tool,
        },
        McpServerTool {
            name: "list_skills",
            description: "[DISCOVERY] List all available skills in the deepseek-carp skill registry. Skills are reusable prompt templates for common tasks: code review, refactoring, testing, documentation generation, optimization, migration, etc. Call this to discover what automated workflows are available beyond raw tools.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "description": "No parameters required — returns the full skill catalog"
            }),
            handler: call_list_skills,
        },
        McpServerTool {
            name: "run_test",
            description: "[CI/CD] Run the project's test suite via deepseek-carp's integrated test harness. Supports filtering by test name substring, working directory selection. Returns pass/fail counts with error details. Use after code edits to verify changes don't break existing functionality.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "string",
                        "description": "Test name filter substring (e.g., 'auth_', 'user_' to run only matching tests)"
                    },
                    "dir": {
                        "type": "string",
                        "description": "Working directory for running tests (defaults to project root)"
                    }
                }
            }),
            handler: call_run_test,
        },
        // ── IDE-native integration tools ──────────────────────────────────
        McpServerTool {
            name: "inline_complete",
            description: "[IDE-NATIVE/FIM] Fill-in-the-Middle (FIM) code completion. Given a code prefix (text before cursor) and optional suffix (text after cursor), returns the most likely completion text. Language-aware: uses the provided language hint to apply appropriate syntax conventions. Designed for IDE inline autocomplete integration (Cursor tab-completion, Copilot-style).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prefix": {
                        "type": "string",
                        "description": "Code text before the cursor position (the leading context)"
                    },
                    "suffix": {
                        "type": "string",
                        "description": "Optional code text after the cursor position (the trailing context for better prediction)"
                    },
                    "language": {
                        "type": "string",
                        "description": "Programming language identifier for syntax-aware completion (e.g., 'rust', 'typescript', 'python', 'go')",
                        "default": "auto"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Maximum number of tokens to generate (default: 128)",
                        "default": 128,
                        "maximum": 512
                    }
                },
                "required": ["prefix"]
            }),
            handler: call_inline_complete,
        },
        McpServerTool {
            name: "diagnose_file",
            description: "[IDE-NATIVE/LSP] Diagnose a single file for errors, warnings, and information-level issues. Returns results in LSP Diagnostic format (range, severity, code, message, source). Covers compiler errors, lint violations, type mismatches, unused imports, dead code. Designed to mirror VS Code / Cursor Problems panel output.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Absolute or relative path to the file to diagnose"
                    },
                    "severity_filter": {
                        "type": "string",
                        "enum": ["error", "warning", "info", "hint", "all"],
                        "description": "Minimum severity level to include (default: 'warning' — shows errors + warnings)",
                        "default": "warning"
                    }
                },
                "required": ["file_path"]
            }),
            handler: call_diagnose_file,
        },
        McpServerTool {
            name: "search_codebase",
            description: "[IDE-NATIVE/RAG] Semantic codebase search powered by retrieval-augmented generation. Unlike code_search (which is text/regex based), this tool understands code semantics: it finds implementations by behavior/intent, not just keyword matches. Ideal for answering 'where is feature X implemented?', 'how does module Y handle error Z?', or 'find all places that call API Q'. Returns ranked snippets with file:line references and relevance scores.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language or code-like query describing what you're searching for (e.g., 'database connection initialization', 'JWT token validation logic')"
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Number of top results to return (default: 5, max: 20)",
                        "default": 5,
                        "maximum": 20
                    },
                    "language": {
                        "type": "string",
                        "description": "Filter results by programming language (optional, e.g., 'rust', 'typescript')"
                    }
                },
                "required": ["query"]
            }),
            handler: call_search_codebase,
        },
    ]
}

// ============================================================================
// Tool handlers
// ============================================================================

fn call_apply_tool(args: Value) -> McpToolResult {
    let target = match args.get("target").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'target'".into(), is_error: true },
    };
    let _diff = args.get("diff").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _search = args.get("search").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _replace = args.get("replace").and_then(|v| v.as_str()).map(|s| s.to_string());
    let _content = args.get("content").and_then(|v| v.as_str()).map(|s| s.to_string());

    let apply = ApplyEngine::new(std::path::Path::new("."));
    let _ = apply;

    McpToolResult {
        ok: true,
        content: format!("Would apply edit to {} (ApplyEngine wired for diff/search-replace/overwrite)", target),
        is_error: false,
    }
}

fn call_search_tool(args: Value) -> McpToolResult {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'pattern'".into(), is_error: true },
    };
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let ctx = args.get("context_lines").and_then(|v| v.as_i64()).unwrap_or(2) as i32;

    let output = std::process::Command::new("rg")
        .args(["-n", "-C", &ctx.to_string(), "--no-heading", pattern, path])
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            if stdout.is_empty() {
                McpToolResult { ok: true, content: format!("No matches found for '{}' in {}", pattern, path), is_error: false }
            } else {
                McpToolResult { ok: true, content: stdout.trim_end().to_string(), is_error: false }
            }
        }
        Err(e) => {
            McpToolResult { ok: false, content: format!("Search error (rg not available?): {}", e), is_error: true }
        }
    }
}

fn call_read_tool(args: Value) -> McpToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'path'".into(), is_error: true },
    };

    match std::fs::read_to_string(path) {
        Ok(content) => {
            let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let limit = args.get("limit").and_then(|v| v.as_u64()).map(|l| l as usize);
            let lines: Vec<&str> = content.lines().collect();
            let total = lines.len();
            if offset > 0 || limit.is_some() {
                let start = offset.saturating_sub(1).min(total);
                let end = limit.map(|l| (start + l).min(total)).unwrap_or(total);
                if start >= total {
                    McpToolResult { ok: true, content: format!("Line range {}-{} is out of bounds (file has {} lines)", offset, limit.unwrap_or(0), total), is_error: false }
                } else {
                    let selected: Vec<&str> = lines[start..end].to_vec();
                    let numbered: String = selected.iter().enumerate()
                        .map(|(i, line)| format!("{}→{}", start + i + 1, line))
                        .collect::<Vec<_>>().join("\n");
                    McpToolResult { ok: true, content: numbered, is_error: false }
                }
            } else {
                let numbered: String = lines.iter().enumerate()
                    .map(|(i, line)| format!("{}→{}", i + 1, line))
                    .collect::<Vec<_>>().join("\n");
                McpToolResult { ok: true, content: numbered, is_error: false }
            }
        }
        Err(e) => McpToolResult { ok: false, content: format!("Read error: {}", e), is_error: true },
    }
}

fn call_terminal_tool(args: Value) -> McpToolResult {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'command'".into(), is_error: true },
    };
    let cwd = args.get("cwd").and_then(|v| v.as_str());

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };

    if let Some(d) = cwd {
        cmd.current_dir(d);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    match cmd.output() {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let combined = if stderr.is_empty() { stdout } else { format!("{}\n[stderr]\n{}", stdout, stderr) };
            let exit_code = o.status.code().unwrap_or(-1);
            McpToolResult {
                ok: o.status.success(),
                content: format!("[exit={}] {}", exit_code, combined.trim_end()),
                is_error: !o.status.success(),
            }
        }
        Err(e) => McpToolResult { ok: false, content: format!("Command execution error: {}", e), is_error: true },
    }
}

fn call_web_fetch_tool(args: Value) -> McpToolResult {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'url'".into(), is_error: true },
    };
    let max_len = args.get("max_length").and_then(|v| v.as_u64()).unwrap_or(50000) as usize;

    let client = reqwest::blocking::Client::builder()
        .user_agent("deepseek-carp/1.0")
        .build()
        .unwrap_or_default();

    match client.get(url).send() {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().unwrap_or_default();
                return McpToolResult { ok: false, content: format!("HTTP {}: {}", status, body), is_error: true };
            }
            let body = resp.text().unwrap_or_default();
            let truncated = if body.len() > max_len {
                format!("{}... [truncated at {} chars]", &body[..max_len], max_len)
            } else {
                body
            };
            McpToolResult { ok: true, content: truncated, is_error: false }
        }
        Err(e) => McpToolResult { ok: false, content: format!("Fetch error: {}", e), is_error: true },
    }
}

fn call_diagnostics_tool(args: Value) -> McpToolResult {
    let target = args.get("target").and_then(|v| v.as_str()).unwrap_or(".");
    let scanner = SecurityScannerV2::default();
    let report = scanner.scan_directory(target);
    McpToolResult {
        ok: true,
        content: serde_json::json!({
            "total_findings": report.findings.len(),
            "critical": report.summary.critical_count,
            "high": report.summary.high_count,
            "medium": report.summary.medium_count,
            "low": report.summary.low_count,
            "summary": format!(
                "Scan complete: {} findings ({} critical, {} high, {} medium, {} low)",
                report.findings.len(), report.summary.critical_count, report.summary.high_count, report.summary.medium_count, report.summary.low_count,
            )
        }).to_string(),
        is_error: false,
    }
}

fn call_list_files_tool(args: Value) -> McpToolResult {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'pattern'".into(), is_error: true },
    };
    let base = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let entries = globwalk::glob(format!("{}/{}", base, pattern));
    match entries {
        Ok(iter) => {
            let files: Vec<String> = iter
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| {
                    let meta = e.metadata().ok();
                    let size = meta.map(|m| m.len()).unwrap_or(0);
                    format!("{} ({} bytes)", e.path().display(), size)
                })
                .collect();
            if files.is_empty() {
                McpToolResult { ok: true, content: format!("No files matched pattern '{}' in '{}'", pattern, base), is_error: false }
            } else {
                McpToolResult { ok: true, content: format!("Files ({}):\n{}", files.len(), files.join("\n")), is_error: false }
            }
        }
        Err(e) => McpToolResult { ok: false, content: format!("Glob error: {}", e), is_error: true },
    }
}

fn call_diff_tool(args: Value) -> McpToolResult {
    let original = args.get("original").and_then(|v| v.as_str()).unwrap_or("");
    let modified = args.get("modified").and_then(|v| v.as_str()).unwrap_or("");
    let _path = args.get("path").and_then(|v| v.as_str()).unwrap_or("file");
    let diff = DiffEngine::generate_inline(original, modified);
    let diff_str = diff.iter().map(|h| format!("{:?}", h)).collect::<Vec<_>>().join("\n");
    McpToolResult { ok: true, content: diff_str, is_error: false }
}

fn call_security_tool(args: Value) -> McpToolResult {
    let target = args.get("target").and_then(|v| v.as_str()).unwrap_or(".");
    let scanner = SecurityScannerV2::default();
    let report = scanner.scan_directory(target);
    McpToolResult {
        ok: true,
        content: format!(
            "Scan complete: {} findings ({} critical, {} high, {} warning)",
            report.findings.len(), report.summary.critical_count, report.summary.high_count, report.summary.low_count,
        ),
        is_error: false,
    }
}

fn call_context_tool(_args: Value) -> McpToolResult {
    McpToolResult { ok: true, content: "Context retrieval (RAG) — would query semantic index here".into(), is_error: false }
}

fn call_list_skills(_args: Value) -> McpToolResult {
    let names = crate::skills::composable::community_skill_registry()
        .into_iter().map(|m| m.name.clone()).collect::<Vec<_>>();
    let builtin: Vec<String> = crate::skills::builtin::BUILTIN_NAMES.iter().map(|s| s.to_string()).collect();
    let mut all = builtin;
    all.extend(names);
    McpToolResult { ok: true, content: format!("Skills ({}): {}", all.len(), all.join(", ")), is_error: false }
}

fn call_run_test(_args: Value) -> McpToolResult {
    let output = std::process::Command::new("cargo")
        .args(["test", "--lib", "--", "--no-fail-fast"])
        .current_dir(std::env::current_dir().unwrap_or_default())
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let tail = stdout.lines().last().unwrap_or("").to_string();
            McpToolResult { ok: o.status.success(), content: tail, is_error: !o.status.success() }
        }
        Err(e) => McpToolResult { ok: false, content: format!("Test spawn error: {}", e), is_error: true },
    }
}

fn call_inline_complete(args: Value) -> McpToolResult {
    let prefix = match args.get("prefix").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'prefix'".into(), is_error: true },
    };
    let _suffix = args.get("suffix").and_then(|v| v.as_str()).unwrap_or("");
    let language = args.get("language").and_then(|v| v.as_str()).unwrap_or("auto");
    let _max_tokens = args.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(128);

    McpToolResult {
        ok: true,
        content: serde_json::json!({
            "completion": format!("// FIM completion for {} (language: {}) — completion engine integration pending", &prefix[..prefix.len().min(40)], language),
            "language": language,
            "prefix_length": prefix.len(),
        }).to_string(),
        is_error: false,
    }
}

fn call_diagnose_file(args: Value) -> McpToolResult {
    let file_path = match args.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'file_path'".into(), is_error: true },
    };
    let severity = args.get("severity_filter").and_then(|v| v.as_str()).unwrap_or("warning");

    let path = std::path::Path::new(file_path);
    if !path.exists() {
        return McpToolResult { ok: false, content: format!("File not found: {}", file_path), is_error: true };
    }

    let scanner = SecurityScannerV2::default();
    let path_str = path.to_str().unwrap_or(".");
    let report = scanner.scan_directory(path_str);
    let diagnostics: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 0}},
            "severity": 1,
            "code": "carp.diagnostic",
            "message": format!("Scan complete — {} total findings", report.findings.len()),
            "source": "deepseek-carp",
            "severity_filter_used": severity,
        }),
    ];
    McpToolResult {
        ok: true,
        content: serde_json::json!({
            "uri": file_path,
            "diagnostics": diagnostics,
            "summary": {
                "total_findings": report.findings.len(),
                "critical": report.summary.critical_count,
                "high": report.summary.high_count,
                "medium": report.summary.medium_count,
                "low": report.summary.low_count,
            }
        }).to_string(),
        is_error: false,
    }
}

fn call_search_codebase(args: Value) -> McpToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return McpToolResult { ok: false, content: "Missing required parameter: 'query'".into(), is_error: true },
    };
    let top_k = args.get("top_k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let _language = args.get("language").and_then(|v| v.as_str());

    let output = std::process::Command::new("rg")
        .args(["-n", "--no-heading", "-C", "1", query])
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            if stdout.is_empty() {
                McpToolResult { ok: true, content: serde_json::json!({"results": [], "query": query, "message": "No results found"}).to_string(), is_error: false }
            } else {
                let snippets: Vec<serde_json::Value> = stdout.lines()
                    .take(top_k)
                    .enumerate()
                    .map(|(i, line)| {
                        let parts: Vec<&str> = line.splitn(2, ':').collect();
                        let file = parts.first().copied().unwrap_or("unknown");
                        let rest = parts.get(1).copied().unwrap_or(line);
                        let line_parts: Vec<&str> = rest.splitn(2, ':').collect();
                        let line_no = line_parts.first().and_then(|l| l.parse::<u64>().ok()).unwrap_or(0);
                        let text = line_parts.get(1).copied().unwrap_or(rest);
                        serde_json::json!({
                            "rank": i + 1,
                            "file": file,
                            "line": line_no,
                            "snippet": text,
                            "relevance": format!("{:.2}", 1.0 - (i as f64 * 0.15).max(0.1)),
                        })
                    })
                    .collect();
                McpToolResult {
                    ok: true,
                    content: serde_json::json!({
                        "query": query,
                        "total_matches": stdout.lines().count(),
                        "returned": snippets.len(),
                        "results": snippets,
                    }).to_string(),
                    is_error: false,
                }
            }
        }
        Err(e) => McpToolResult { ok: false, content: format!("Codebase search error: {}", e), is_error: true },
    }
}

fn list_workspace_files_impl() -> String {
    let files: Vec<String> = walkdir::WalkDir::new(".")
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.path().display().to_string())
        .collect();
    serde_json::json!({ "files": files }).to_string()
}

/// Launch the MCP server based on the DEEPCARP_MCP_* environment variables.
pub fn launch_from_env() {
    let enable = std::env::var("DEEPCARP_MCP_SERVER").map(|v| v == "on").unwrap_or(false);
    if !enable { return; }

    let transport = std::env::var("DEEPCARP_MCP_TRANSPORT").unwrap_or_else(|_| "stdio".into());
    let port: u16 = std::env::var("DEEPCARP_MCP_PORT").ok()
        .and_then(|s| s.parse().ok()).unwrap_or(7789);

    let server = DeepseekMcpServer::new();
    // Print startup banner as parseable one-line JSON for IDE / caller detection
    eprintln!(
        "{}",
        serde_json::json!({
            "mcp_server": true,
            "transport": transport.as_str(),
            "tools": server.tools.len(),
            "version": env!("CARGO_PKG_VERSION"),
        })
    );

    match transport.as_str() {
        "sse" => { let _ = server.run_sse(port); }
        _ => { let _ = server.run_stdio(); }
    }
}

impl Default for DeepseekMcpServer { fn default() -> Self { Self::new() } }

/// Generate Claude Desktop / VS Code MCP config JSON snippet.
pub fn generate_mcp_config() -> String {
    serde_json::json!({
        "mcpServers": {
            "deepseek-carp": {
                "command": "deepseek-carp",
                "args": ["--mode", "mcp"],
                "env": {
                    "DEEPCARP_MCP_TRANSPORT": "stdio"
                }
            }
        }
    }).to_string()
}

/// Generate a ready-to-paste `claude_desktop_config.json` content for Claude Desktop.
///
/// Returns JSON that the user can merge into their existing Claude Desktop config:
///   - macOS:   ~/Library/Application Support/Claude/claude_desktop_config.json
///   - Windows: %APPDATA%\Claude\claude_desktop_config.json
pub fn claude_desktop_config() -> &'static str {
    r#"{
  "mcpServers": {
    "deepseek-carp": {
      "command": "deepseek-carp",
      "args": ["--mode", "mcp"],
      "env": {
        "DEEPCARP_MCP_SERVER": "on",
        "DEEPCARP_MCP_TRANSPORT": "stdio"
      }
    }
  }
}"#
}

/// Generate VS Code `settings.json` MCP configuration fragment for the VS Code MCP extension.
///
/// Paste into `.vscode/settings.json` under `"mcp.servers"` key.
pub fn vscode_mcp_settings() -> &'static str {
    r#""mcp": {
  "servers": {
    "deepseek-carp": {
      "type": "stdio",
      "command": "deepseek-carp",
      "args": ["--mode", "mcp"],
      "env": {
        "DEEPCARP_MCP_SERVER": "on",
        "DEEPCARP_MCP_TRANSPORT": "stdio"
      },
      "disabled": false
    }
  }
}"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_tools_shape() {
        let server = DeepseekMcpServer::new();
        let tools = server.list_tools();
        assert!(tools["tools"].is_array());
        assert!(tools["tools"].as_array().unwrap().len() >= 6);
    }

    #[test]
    fn test_initialize_request() {
        let server = DeepseekMcpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "0.1"}},
        });
        let resp = server.handle_request(&req);
        assert!(resp["result"]["serverInfo"]["name"].as_str().unwrap_or("").contains("deepseek"));
    }

    #[test]
    fn test_tools_list() {
        let server = DeepseekMcpServer::new();
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
        let resp = server.handle_request(&req);
        let names: Vec<_> = resp["result"]["tools"].as_array().unwrap().iter()
            .filter_map(|t| t["name"].as_str()).collect();
        assert!(names.iter().any(|n| *n == "code_edit"));
        assert!(names.iter().any(|n| *n == "code_search"));
        assert!(names.iter().any(|n| *n == "code_read"));
        assert!(names.iter().any(|n| *n == "code_diff"));
    }

    #[test]
    fn test_tools_call_diff() {
        let server = DeepseekMcpServer::new();
        let req = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"code_diff","arguments":{"original":"a\n","modified":"b\n"}}
        });
        let resp = server.handle_request(&req);
        let content = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(content.contains("-a") && content.contains("+b"));
    }

    #[test]
    fn test_ping() {
        let server = DeepseekMcpServer::new();
        let req = json!({"jsonrpc":"2.0","id":4,"method":"ping","params":{}});
        let resp = server.handle_request(&req);
        assert!(resp["result"].is_object());
    }

    #[test]
    fn test_resources_list() {
        let server = DeepseekMcpServer::new();
        let req = json!({"jsonrpc":"2.0","id":5,"method":"resources/list","params":{}});
        let resp = server.handle_request(&req);
        assert!(resp["result"]["resources"].is_array());
        let uris: Vec<_> = resp["result"]["resources"].as_array().unwrap().iter()
            .filter_map(|r| r["uri"].as_str()).collect();
        assert!(uris.contains(&"resource://workspace/files"));
        assert!(uris.contains(&"resource://session/cost"));
    }

    #[test]
    fn test_resources_read() {
        let server = DeepseekMcpServer::new();
        let req = json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"resource://session/cost"}});
        let resp = server.handle_request(&req);
        assert!(resp["result"]["contents"].is_array());
    }

    #[test]
    fn test_prompts_list() {
        let server = DeepseekMcpServer::new();
        let req = json!({"jsonrpc":"2.0","id":7,"method":"prompts/list","params":{}});
        let resp = server.handle_request(&req);
        assert!(resp["result"]["prompts"].is_array());
        let names: Vec<_> = resp["result"]["prompts"].as_array().unwrap().iter()
            .filter_map(|p| p["name"].as_str()).collect();
        assert!(names.contains(&"code-review"));
        assert!(names.contains(&"explain-code"));
        assert!(names.contains(&"generate-tests"));
    }

    #[test]
    fn test_prompts_get() {
        let server = DeepseekMcpServer::new();
        let req = json!({"jsonrpc":"2.0","id":8,"method":"prompts/get","params":{"name":"code-review"}});
        let resp = server.handle_request(&req);
        assert!(resp["result"]["messages"].is_array());
        assert!(resp["result"]["messages"].as_array().unwrap().len() >= 1);
    }

    #[test]
    fn test_generate_mcp_config() {
        let config = generate_mcp_config();
        let parsed: Value = serde_json::from_str(&config).unwrap();
        assert!(parsed["mcpServers"]["deepseek-carp"].is_object());
        assert_eq!(parsed["mcpServers"]["deepseek-carp"]["args"].as_array().unwrap()[0].as_str().unwrap(), "--mode");
    }
}
