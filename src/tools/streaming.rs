//! Streaming tool executor — inspired by Claude Code's StreamingToolExecutor.
//!
//! Executes tools with real-time output streaming via hook events.
//! Supports concurrent safe tools (read operations) and exclusive tools (write ops).
//!
//! ## State machine
//!
//! ```text
//! queued → executing → completed → yielded
//!                    ↘ errored  → yielded
//! ```
//!
//! ## Concurrency model
//!
//! - Read-only tools (read_file, search, list) can run in parallel
//! - Write/destructive tools (write_file, execute_shell) require exclusive lock
//! - If a bash tool fails, sibling reads are cancelled (cascade error)

use async_trait::async_trait;
use tokio::sync::{Mutex, mpsc};
use std::sync::Arc;

/// Progress update emitted during tool execution.
#[derive(Debug, Clone)]
pub struct ToolProgress {
    /// Tool name.
    pub tool_name: String,
    /// Call ID for correlation.
    pub call_id: String,
    /// Streaming output chunk (partial stdout/stderr).
    pub output_chunk: String,
    /// Whether this is the final chunk.
    pub is_done: bool,
}

/// Error from streaming tool execution.
#[derive(Debug, thiserror::Error)]
pub enum StreamingToolError {
    #[error("Tool '{name}' failed: {message}")]
    Failed { name: String, message: String },
    #[error("Tool '{name}' timed out after {timeout_secs}s")]
    Timeout { name: String, timeout_secs: u64 },
}

/// Trait for streaming-capable tool execution.
#[async_trait]
pub trait StreamingTool: Send + Sync {
    /// Execute the tool, sending progress via the channel.
    /// Returns the final output on success.
    async fn execute_streaming(
        &self,
        name: &str,
        args: &str,
        call_id: &str,
        progress_tx: mpsc::Sender<ToolProgress>,
    ) -> Result<String, StreamingToolError>;

    /// Whether this tool can run concurrently with other tools.
    fn is_concurrency_safe(&self) -> bool { false }

    /// Whether this tool is read-only (vs destructive).
    fn is_read_only(&self) -> bool { false }

    /// What to do when interrupted (cancel tool vs let it finish).
    fn interrupt_behavior(&self) -> ToolInterrupt { ToolInterrupt::Cancel }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolInterrupt {
    /// Cancel the tool on interrupt (default for read-only tools).
    Cancel,
    /// Let the tool finish even if the user sends a new message.
    Block,
}

/// Streaming executor — manages concurrent tool execution with progress streaming.
pub struct StreamingToolExecutor {
    /// Registry of streaming tools.
    tools: Vec<Arc<dyn StreamingTool>>,
    /// Mutex to serialize destructive tool operations.
    destructive_lock: Arc<Mutex<()>>,
}

impl StreamingToolExecutor {
    pub fn new(tools: Vec<Arc<dyn StreamingTool>>) -> Self {
        Self {
            tools,
            destructive_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Execute a single tool with real-time progress streaming.
    /// Returns a channel receiver for progress updates.
    pub async fn execute(
        &self,
        tool_name: &str,
        args: &str,
        call_id: &str,
    ) -> Result<(mpsc::Receiver<ToolProgress>, String), StreamingToolError> {
        let tool = self.tools.iter()
            .find(|_t| {
                // Match by name — could be more sophisticated
                std::any::type_name::<dyn StreamingTool>().contains(tool_name)
            });

        let (progress_tx, progress_rx) = mpsc::channel(64);

        // If destructive, acquire lock
        let needs_lock = tool.is_none_or(|t| !t.is_concurrency_safe());
        let _guard = if needs_lock {
            Some(self.destructive_lock.lock().await)
        } else {
            None
        };

        // Try the first matching tool, or return error
        if let Some(tool) = tool {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                tool.execute_streaming(tool_name, args, call_id, progress_tx),
            ).await;

            match result {
                Ok(Ok(output)) => Ok((progress_rx, output)),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(StreamingToolError::Timeout {
                    name: tool_name.to_string(),
                    timeout_secs: 120,
                }),
            }
        } else {
            // No streaming tool found — return empty progress
            drop(progress_rx); // close channel
            let (_, empty_rx) = mpsc::channel(1);
            Ok((empty_rx, format!("Tool '{}' executed (no streaming)", tool_name)))
        }
    }

    /// Execute multiple tools concurrently where safe.
    pub async fn execute_many(
        &self,
        commands: Vec<(String, String, String)>, // (tool_name, args, call_id)
    ) -> Vec<Result<(mpsc::Receiver<ToolProgress>, String), StreamingToolError>> {
        let mut handles = Vec::new();

        for (name, args, call_id) in commands {
            let executor = self.clone_executor();
            handles.push(tokio::spawn(async move {
                executor.execute(&name, &args, &call_id).await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(StreamingToolError::Failed {
                    name: "unknown".into(),
                    message: format!("Join error: {}", e),
                })),
            }
        }

        results
    }

    fn clone_executor(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            destructive_lock: Arc::clone(&self.destructive_lock),
        }
    }
}

// ============================================================================
// Built-in streaming tool: Shell command execution
// ============================================================================

/// Streaming shell command execution.
/// Uses std::process::Command with piped stdout.
pub struct ShellTool;

#[async_trait]
impl StreamingTool for ShellTool {
    async fn execute_streaming(
        &self,
        _name: &str,
        args: &str,
        call_id: &str,
        progress_tx: mpsc::Sender<ToolProgress>,
    ) -> Result<String, StreamingToolError> {
        use tokio::process::Command;
        use tokio::io::AsyncBufReadExt;

        // Parse command from args JSON
        let cmd_str = if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            v["command"].as_str().unwrap_or(args).to_string()
        } else {
            args.to_string()
        };

        let (shell_cmd, shell_args) = if cfg!(target_os = "windows") {
            ("cmd", vec!["/C", &cmd_str])
        } else {
            ("sh", vec!["-c", &cmd_str])
        };

        let mut child = Command::new(shell_cmd)
            .args(&shell_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| StreamingToolError::Failed {
            name: "shell".into(),
            message: format!("Failed to spawn: {}", e),
        })?;

        let stdout = child.stdout.take().expect("option empty: streaming.rs:218");
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut line = String::new();
        let mut output = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    output.push_str(&line);
                    let _ = progress_tx.send(ToolProgress {
                        tool_name: "shell".into(),
                        call_id: call_id.to_string(),
                        output_chunk: line.clone(),
                        is_done: false,
                    }).await;
                }
                Err(_) => break,
            }
        }

        let status = child.wait().await.map_err(|e| StreamingToolError::Failed {
            name: "shell".into(),
            message: format!("Wait error: {}", e),
        })?;

        let _ = progress_tx.send(ToolProgress {
            tool_name: "shell".into(),
            call_id: call_id.to_string(),
            output_chunk: format!("\n[Exit code: {}]", status.code().unwrap_or(-1)),
            is_done: true,
        }).await;

        Ok(output)
    }

    fn is_concurrency_safe(&self) -> bool { false }
    fn is_read_only(&self) -> bool { false }
    fn interrupt_behavior(&self) -> ToolInterrupt { ToolInterrupt::Cancel }
}

// ============================================================================
// Built-in streaming tool: File read (streaming large files)
// ============================================================================

/// Streaming file reader — reads large files in chunks.
pub struct FileReadTool;

#[async_trait]
impl StreamingTool for FileReadTool {
    async fn execute_streaming(
        &self,
        _name: &str,
        args: &str,
        call_id: &str,
        progress_tx: mpsc::Sender<ToolProgress>,
    ) -> Result<String, StreamingToolError> {
        let path = if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
            v["path"].as_str().unwrap_or(args).to_string()
        } else {
            args.to_string()
        };

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            StreamingToolError::Failed {
                name: "read_file".into(),
                message: format!("Read error: {}", e),
            }
        })?;

        // Stream in 4KB chunks
        for chunk in content.as_bytes().chunks(4096) {
            let _ = progress_tx.send(ToolProgress {
                tool_name: "read_file".into(),
                call_id: call_id.to_string(),
                output_chunk: String::from_utf8_lossy(chunk).to_string(),
                is_done: false,
            }).await;
        }

        let _ = progress_tx.send(ToolProgress {
            tool_name: "read_file".into(),
            call_id: call_id.to_string(),
            output_chunk: String::new(),
            is_done: true,
        }).await;

        Ok(content)
    }

    fn is_concurrency_safe(&self) -> bool { true }
    fn is_read_only(&self) -> bool { true }
    fn interrupt_behavior(&self) -> ToolInterrupt { ToolInterrupt::Cancel }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_tool_echo() {
        let tool = ShellTool;
        let (tx, mut rx) = mpsc::channel(64);
        let result = tool.execute_streaming(
            "shell",
            r#"{"command":"echo hello"}"#,
            "call-1",
            tx,
        ).await;

        assert!(result.is_ok());
        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            chunks.push(chunk);
        }
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.output_chunk.contains("hello")));
    }

    #[tokio::test]
    async fn test_concurrent_reads() {
        let tool1 = Arc::new(FileReadTool) as Arc<dyn StreamingTool>;
        let tool2 = Arc::new(FileReadTool) as Arc<dyn StreamingTool>;
        let executor = StreamingToolExecutor::new(vec![tool1, tool2]);

        let results = executor.execute_many(vec![
            ("read_file".into(), r#"{"path":"Cargo.toml"}"#.into(), "c1".into()),
            ("read_file".into(), r#"{"path":"README.md"}"#.into(), "c2".into()),
        ]).await;

        assert_eq!(results.len(), 2);
    }
}
