//! Carp Bridge — bidirectional sync between dscarp-lapce and deepseek-carp.
//!
//! ## Two Communication Channels
//!
//! ### Channel 1: MCP JSON-RPC (primary — real-time, bidirectional)
//!
//! dscarp-lapce connects to a deepseek-carp MCP SSE server over localhost
//! HTTP and invokes tools in real time. This is the preferred channel when
//! deepseek-carp is running as a subprocess.
//!
//! ```text
//! dscarp-lapce ──HTTP POST localhost:7789──▶ deepseek-carp MCP Server
//!                  tools/call {code_apply}
//!                  tools/call {security_scan}
//!                  tools/call {list_skills}
//!                  tools/call {run_test}
//! ```
//!
//! ### Channel 2: File sync (fallback — for debugging / offline)
//!
//! | Direction | Path | Contents |
//! |-----------|------|----------|
//! | deepseek-carp → dscarp-lapce | `.carp/diagnostics/diags.json` | Diagnostics pushed to Problem panel |
//! | dscarp-lapce → deepseek-carp | `.carp/workspace/state.json` | Workspace state read by LoopEngine |
//! | deepseek-carp → dscarp-lapce | `.carp/qa/results.json` | QA test results for QA panel |
//! | deepseek-carp → dscarp-lapce | `.carp/screenshots/` | Screenshots captured during analysis |

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use crossbeam_channel::{Sender, unbounded};
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::workspace::LapceWorkspace;

/// Default MCP SSE endpoint for deepseek-carp.
pub const MCP_DEFAULT_HOST: &str = "127.0.0.1";
pub const MCP_DEFAULT_PORT: u16 = 7789;
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A single diagnostic from deepseek-carp's bridge JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct CarpDiagnostic {
    pub file: String,
    pub line: usize,
    pub severity: u8,
    pub message: String,
    pub source: String,
}

/// Workspace state written by dscarp-lapce for deepseek-carp to read.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LapceSyncState {
    pub root: PathBuf,
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub git_status: Option<String>,
    pub cursor: Option<CursorPosition>,
    pub diagnostics: HashMap<PathBuf, Vec<DiagnosticInfo>>,
}

/// Cursor position within a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPosition {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
}

/// A single diagnostic entry for workspace state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticInfo {
    pub line: u32,
    pub severity: u8,
    pub message: String,
}

/// QA test result summary from deepseek-carp's test runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaResultSummary {
    pub suite: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate_pct: f64,
    pub timestamp: u64,
    pub details: Vec<QaResultDetail>,
}

/// A single QA test result detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaResultDetail {
    pub test_name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub screenshot_ref: Option<String>,
}

/// Bridge that monitors Carp diagnostics and writes workspace state.
pub struct CarpBridge {
    /// The workspace root path.
    workspace: Arc<LapceWorkspace>,
    /// Last observed modification time of the diags.json file.
    last_mtime: Option<SystemTime>,
    /// Last observed modification time of the qa/results.json file.
    last_qa_mtime: Option<SystemTime>,
    /// Last observed QA results (for change detection).
    last_qa_result: Option<QaResultSummary>,
    /// Receiver for workspace state updates from the main thread.
    state_rx: crossbeam_channel::Receiver<LapceSyncState>,
    /// Sender for workspace state updates (cloned by main thread).
    pub state_tx: crossbeam_channel::Sender<LapceSyncState>,
}

impl CarpBridge {
    /// Create a new CarpBridge for the given workspace.
    pub fn new(workspace: Arc<LapceWorkspace>) -> Self {
        let (state_tx, state_rx) = unbounded();
        Self {
            workspace,
            last_mtime: None,
            last_qa_mtime: None,
            last_qa_result: None,
            state_rx,
            state_tx,
        }
    }

    /// The path to the diagnostics JSON file.
    fn diags_path(&self) -> Option<PathBuf> {
        self.workspace
            .path
            .as_ref()
            .map(|p| p.join(".carp").join("diagnostics").join("diags.json"))
    }

    /// The path to the workspace state JSON file.
    fn state_path(&self) -> Option<PathBuf> {
        self.workspace
            .path
            .as_ref()
            .as_ref()
            .map(|p| p.join(".carp").join("workspace").join("state.json"))
    }

    /// The path to the QA results JSON file.
    fn qa_path(&self) -> Option<PathBuf> {
        self.workspace
            .path
            .as_ref()
            .as_ref()
            .map(|p| p.join(".carp").join("qa").join("results.json"))
    }

    /// The path to the screenshots directory.
    fn screenshots_path(&self) -> Option<PathBuf> {
        self.workspace
            .path
            .as_ref()
            .as_ref()
            .map(|p| p.join(".carp").join("screenshots"))
    }

    /// Poll the diagnostics file. Returns parsed diagnostics grouped by file path
    /// if the file has changed since the last poll.
    pub fn poll(&mut self) -> Option<im::HashMap<PathBuf, Vec<Diagnostic>>> {
        let path = match self.diags_path() {
            Some(p) => p,
            None => return None,
        };

        if !path.exists() {
            return None;
        }

        let mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(m) => m,
            Err(_) => return None,
        };

        if self.last_mtime == Some(mtime) {
            return None;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return None,
        };

        let carp_diags: Vec<CarpDiagnostic> = match serde_json::from_str(&content) {
            Ok(d) => d,
            Err(_) => return None,
        };

        self.last_mtime = Some(mtime);

        let workspace_path = self.workspace.path.as_deref().unwrap_or(Path::new(""));
        let mut by_path: im::HashMap<PathBuf, Vec<Diagnostic>> = im::HashMap::new();
        for cd in carp_diags {
            let full_path = workspace_path.join(&cd.file);
            by_path.entry(full_path).or_default().push(carp_to_lsp(cd));
        }

        Some(by_path)
    }

    /// Poll the QA results file. Returns the QA summary if the file has changed.
    pub fn poll_qa(&mut self) -> Option<QaResultSummary> {
        let path = match self.qa_path() {
            Some(p) => p,
            None => return None,
        };

        if !path.exists() {
            return None;
        }

        let mtime = match std::fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(m) => m,
            Err(_) => return None,
        };

        if self.last_qa_mtime == Some(mtime) {
            return None;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return None,
        };

        let result: QaResultSummary = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(_) => return None,
        };

        // Avoid re-sending the same result
        if self.last_qa_result.as_ref().map(|r| r.timestamp) == Some(result.timestamp) {
            return None;
        }

        self.last_qa_mtime = Some(mtime);
        self.last_qa_result = Some(result.clone());
        Some(result)
    }

    /// Get the latest screenshots directory listing, if it exists.
    pub fn list_screenshots(&self) -> Vec<PathBuf> {
        let path = match self.screenshots_path() {
            Some(p) => p,
            None => return Vec::new(),
        };

        if !path.exists() {
            return Vec::new();
        }

        let mut screenshots: Vec<PathBuf> = match std::fs::read_dir(&path) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "png" || ext == "jpg")
                        .unwrap_or(false)
                })
                .map(|e| e.path())
                .collect(),
            Err(_) => Vec::new(),
        };

        screenshots.sort_by_key(|p| std::fs::metadata(p).ok().and_then(|m| m.modified()));
        screenshots.reverse(); // newest first
        screenshots
    }

    /// Write latest workspace state received from the main thread.
    fn handle_state_update(&self, state: LapceSyncState) {
        let path = match self.state_path() {
            Some(p) => p,
            None => return,
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(content) = serde_json::to_string_pretty(&state) {
            let _ = std::fs::write(&path, content);
        }
    }

    /// Write AI diagnostics to `.carp/diagnostics/lsp_diags.json` for the
    /// editor event loop to pick up.
    pub fn push_diagnostics_to_lsp(&self, diags: Vec<Diagnostic>, path: &Path) {
        let workspace_root = match self.workspace.path.as_ref() {
            Some(p) => p,
            None => return,
        };
        write_lsp_diags_file(&diags, path, workspace_root);
    }

    /// Read the last AI diagnostics from `.carp/diagnostics/lsp_diags.json`.
    pub fn get_lsp_diagnostics(&self) -> Option<Vec<(String, Vec<Diagnostic>)>> {
        let workspace_root = self.workspace.path.as_ref()?;
        read_lsp_diags_file(workspace_root)
    }

    /// Start a background polling thread.
    pub fn start_poller(
        mut self,
        tx: Sender<im::HashMap<PathBuf, Vec<Diagnostic>>>,
        qa_tx: Option<Sender<QaResultSummary>>,
    ) {
        std::thread::Builder::new()
            .name("carp-bridge-poller".to_owned())
            .spawn(move || loop {
                // Check for workspace state updates from main thread
                while let Ok(state) = self.state_rx.try_recv() {
                    self.handle_state_update(state);
                }

                // Poll diagnostic file
                if let Some(diags) = self.poll() {
                    if tx.send(diags).is_err() {
                        break;
                    }
                }

                // Poll QA results file (for IDE QA panel)
                if let Some(ref qa_tx) = qa_tx {
                    if let Some(qa_result) = self.poll_qa() {
                        if qa_tx.send(qa_result).is_err() {
                            break;
                        }
                    }
                }

                std::thread::sleep(POLL_INTERVAL);
            })
            .expect("Failed to spawn Carp bridge poller thread");
    }
}

/// Convert a Carp diagnostic to an LSP Diagnostic for the Problem panel.
fn carp_to_lsp(cd: CarpDiagnostic) -> Diagnostic {
    let line = if cd.line > 0 { cd.line - 1 } else { 0 };
    let severity = match cd.severity {
        0 => Some(DiagnosticSeverity::ERROR),
        1 => Some(DiagnosticSeverity::WARNING),
        2 => Some(DiagnosticSeverity::INFORMATION),
        3 => Some(DiagnosticSeverity::HINT),
        _ => Some(DiagnosticSeverity::WARNING),
    };

    Diagnostic {
        range: Range {
            start: Position {
                line: line as u32,
                character: 0,
            },
            end: Position {
                line: line as u32,
                character: 0,
            },
        },
        severity,
        message: cd.message,
        source: Some(cd.source),
        ..Default::default()
    }
}

/// Write LSP diagnostics to `.carp/diagnostics/lsp_diags.json` for the
/// editor event loop to pick up.  Stores as a JSON array of
/// `{uri, diagnostics}` entries.
pub fn write_lsp_diags_file(
    diags: &[Diagnostic],
    file_path: &Path,
    workspace_root: &Path,
) {
    let dir = workspace_root.join(".carp").join("diagnostics");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("lsp_diags.json");

    let uri = Url::from_file_path(file_path)
        .map(|u| u.to_string())
        .unwrap_or_default();

    let entry = serde_json::json!({
        "uri": uri,
        "diagnostics": diags,
    });

    let _ = std::fs::write(&path, serde_json::to_string(&entry).unwrap_or_default());
}

/// Read the last AI diagnostics from `.carp/diagnostics/lsp_diags.json`.
/// Returns a list of `(uri_string, diagnostics)` pairs.
pub fn read_lsp_diags_file(workspace_root: &Path) -> Option<Vec<(String, Vec<Diagnostic>)>> {
    let path = workspace_root
        .join(".carp")
        .join("diagnostics")
        .join("lsp_diags.json");
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    let uri = val.get("uri")?.as_str()?.to_string();
    let diags: Vec<Diagnostic> = serde_json::from_value(val.get("diagnostics")?.clone()).ok()?;
    Some(vec![(uri, diags)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_carp_to_lsp_error() {
        let cd = CarpDiagnostic {
            file: "src/main.rs".into(),
            line: 42,
            severity: 0,
            message: "test error".into(),
            source: "deepseek-carp".into(),
        };
        let lsp = carp_to_lsp(cd);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp.range.start.line, 41);
        assert_eq!(lsp.message, "test error");
    }

    #[test]
    fn test_carp_to_lsp_warning() {
        let cd = CarpDiagnostic {
            file: "src/lib.rs".into(),
            line: 10,
            severity: 1,
            message: "test warning".into(),
            source: "deepseek-carp".into(),
        };
        let lsp = carp_to_lsp(cd);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn test_lapce_sync_state_default() {
        let state = LapceSyncState::default();
        assert!(state.root.as_os_str().is_empty());
        assert!(state.open_files.is_empty());
    }
}

// ============================================================================
// Channel 1: MCP Client — real-time bridge to deepseek-carp MCP Server
// ============================================================================

/// Status of the MCP connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpStatus {
    Disconnected,
    Connecting,
    Connected,
}

/// Lightweight JSON-RPC 2.0 MCP client over raw TCP (no external deps).
pub struct McpClient {
    host: String,
    port: u16,
    status: Arc<std::sync::Mutex<McpStatus>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl McpClient {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            status: Arc::new(std::sync::Mutex::new(McpStatus::Disconnected)),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    pub fn default() -> Self {
        Self::new(MCP_DEFAULT_HOST, MCP_DEFAULT_PORT)
    }

    pub fn status(&self) -> McpStatus {
        self.status.lock().cloned().unwrap_or(McpStatus::Disconnected)
    }

    /// Try connecting — on success runs initialize handshake.
    pub fn connect(&self) -> bool {
        let _ = self.set_status(McpStatus::Connecting);
        match TcpStream::connect_timeout(
            &format!("{}:{}", self.host, self.port).parse().unwrap(),
            Duration::from_millis(500),
        ) {
            Ok(_) => {
                let _ = self.set_status(McpStatus::Connected);
                true
            }
            Err(_) => {
                let _ = self.set_status(McpStatus::Disconnected);
                false
            }
        }
    }

    fn set_status(&self, s: McpStatus) -> std::sync::LockResult<std::sync::MutexGuard<'_, McpStatus>> {
        *self.status.lock().unwrap() = s;
        Ok(())
    }

    fn request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let mut stream = TcpStream::connect_timeout(
            &format!("{}:{}", self.host, self.port).parse().unwrap(),
            Duration::from_millis(1000),
        ).map_err(|e| format!("connect: {}", e))?;

        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        let http = format!(
            "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.host, body.len(), body
        );

        stream.write_all(http.as_bytes()).map_err(|e| e.to_string())?;
        stream.flush().map_err(|e| e.to_string())?;

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;

        let text = String::from_utf8_lossy(&buf);
        let json_start = match text.find("\r\n\r\n") {
            Some(p) => p + 4,
            None => 0,
        };
        let json_text = &text[json_start..];
        let val: serde_json::Value = serde_json::from_str(json_text).map_err(|e| format!("parse: {}", e))?;

        Ok(val.get("result").cloned().unwrap_or(val))
    }

    pub fn list_tools(&self) -> Result<Vec<String>, String> {
        let resp = self.request("tools/list", serde_json::json!({}))?;
        let tools = resp.get("tools").and_then(|t| t.as_array()).ok_or("no tools array")?;
        Ok(tools.iter().filter_map(|t| t.get("name").and_then(|v| v.as_str()).map(String::from)).collect())
    }

    pub fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<String, String> {
        let resp = self.request("tools/call", serde_json::json!({
            "name": name,
            "arguments": arguments,
        }))?;
        let content = resp.get("content").and_then(|c| c.as_array()).ok_or("no content")?;
        let first = content.first().ok_or("empty content")?;
        Ok(first.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string())
    }

    pub fn list_skills(&self) -> Result<String, String> {
        self.call_tool("list_skills", serde_json::json!({}))
    }

    pub fn security_scan(&self, target: &str) -> Result<String, String> {
        self.call_tool("security_scan", serde_json::json!({ "target": target }))
    }

    pub fn diff(&self, original: &str, modified: &str, path: &str) -> Result<String, String> {
        self.call_tool("code_diff", serde_json::json!({
            "original": original,
            "modified": modified,
            "path": path,
        }))
    }

    pub fn run_tests(&self) -> Result<String, String> {
        self.call_tool("run_test", serde_json::json!({}))
    }

    pub fn health_ping(&self) -> bool {
        self.connect()
    }
}

impl Default for McpClient { fn default() -> Self { Self::new(MCP_DEFAULT_HOST, MCP_DEFAULT_PORT) } }

/// A unified bridge: MCP primary + file fallback.
pub struct UnifiedBridge {
    pub mcp: McpClient,
    pub file: CarpBridge,
}

impl UnifiedBridge {
    pub fn new(workspace: Arc<LapceWorkspace>, mcp_host: impl Into<String>, mcp_port: u16) -> Self {
        Self {
            mcp: McpClient::new(mcp_host, mcp_port),
            file: CarpBridge::new(workspace),
        }
    }

    pub fn with_mcp_defaults(workspace: Arc<LapceWorkspace>) -> Self {
        Self::new(workspace, MCP_DEFAULT_HOST, MCP_DEFAULT_PORT)
    }

    pub fn available(&self) -> bool {
        self.mcp.health_ping()
    }

    pub fn list_skills(&self) -> Option<String> {
        if self.available() { self.mcp.list_skills().ok() } else { None }
    }

    pub fn security_scan(&self, target: &str) -> Option<String> {
        if self.available() { self.mcp.security_scan(target).ok() } else { None }
    }

    pub fn run_tests(&self) -> Option<String> {
        if self.available() { self.mcp.run_tests().ok() } else { None }
    }
}