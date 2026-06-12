//! JSON output utilities — machine-readable structured output (CLI-Anything pattern).
//!
//! Every CLI command can produce both human-readable text AND structured JSON.
//! When `--json` is passed globally, output switches to JSON format.
//!
//! ## Usage
//!
//! ```rust
//! use crate::cli::json_output::{JsonOutput, CommandResult};
//!
//! let result = CommandResult {
//!     command: "archive list".into(),
//!     success: true,
//!     data: serde_json::json!({"runs": 5}),
//!     ..Default::default()
//! };
//! println!("{}", result.to_json());
//! ```

use serde::{Deserialize, Serialize};

/// Universal JSON output envelope for all CLI commands.
///
/// Follows CLI-Anything's convention: every command has the same
/// top-level structure with `command`, `success`, `data`, and `metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    /// The command that was executed.
    pub command: String,
    /// Whether the command succeeded.
    pub success: bool,
    /// Command-specific payload.
    pub data: serde_json::Value,
    /// Human-readable message (shown in non-JSON mode too).
    pub message: Option<String>,
    /// Error details if success == false.
    pub error: Option<String>,
    /// Execution metadata.
    pub metadata: CommandMetadata,
}

/// Metadata about command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMetadata {
    /// ISO 8601 timestamp of when this was generated.
    pub timestamp: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// deepseek-carp version.
    pub version: String,
}

impl Default for CommandMetadata {
    fn default() -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            duration_ms: 0,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl Default for CommandResult {
    fn default() -> Self {
        Self {
            command: "unknown".into(),
            success: false,
            data: serde_json::Value::Null,
            message: None,
            error: None,
            metadata: CommandMetadata::default(),
        }
    }
}

impl CommandResult {
    /// Create a successful result.
    pub fn ok(command: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            command: command.into(),
            success: true,
            data,
            message: None,
            error: None,
            metadata: CommandMetadata::default(),
        }
    }

    /// Create a successful result with a human-readable message.
    pub fn ok_with_msg(command: impl Into<String>, data: serde_json::Value, msg: impl Into<String>) -> Self {
        let mut r = Self::ok(command, data);
        r.message = Some(msg.into());
        r
    }

    /// Create an error result.
    pub fn err(command: impl Into<String>, err: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            success: false,
            data: serde_json::Value::Null,
            message: None,
            error: Some(err.into()),
            metadata: CommandMetadata::default(),
        }
    }

    /// Set the duration.
    pub fn with_duration(mut self, ms: u64) -> Self {
        self.metadata.duration_ms = ms;
        self
    }

    /// Serialize to pretty-printed JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| r#"{"error": "JSON serialization failed"}"#.into())
    }

    /// Print output based on --json flag.
    ///
    /// If `json_mode` is true, prints JSON. Otherwise prints the text message.
    pub fn print(&self, json_mode: bool) {
        if json_mode {
            println!("{}", self.to_json());
        } else if let Some(ref msg) = self.message {
            println!("{}", msg);
        } else if !self.success {
            if let Some(ref err) = self.error {
                eprintln!("Error: {}", err);
            }
        } else {
            // Fallback: print data as-is for non-JSON mode
            if self.data != serde_json::Value::Null {
                println!("{}", self.data);
            }
        }
    }
}

/// Helper to convert archive list to JSON-compatible format.
pub fn archive_list_to_json(
    metas: &[crate::storage::archive::ArchiveMeta],
) -> serde_json::Value {
    let runs: Vec<serde_json::Value> = metas.iter().map(|m| {
        serde_json::json!({
            "run_id": m.run_id,
            "target": m.target,
            "mode": m.mode,
            "passed": m.passed,
            "rounds": m.total_rounds,
            "created_at": m.created_at,
            "duration_ms": m.total_time_ms,
        })
    }).collect();

    serde_json::json!({
        "total": metas.len(),
        "runs": runs,
    })
}

/// Helper to convert RetroReport to JSON-compatible format.
pub fn retro_report_to_json(
    report: &crate::storage::archive::RetroReport,
) -> serde_json::Value {
    let sprints: Vec<serde_json::Value> = report.hotspots.iter().map(|h| {
        serde_json::json!({
            "target": h.target,
            "run_count": h.run_count,
            "pass_count": h.pass_count,
        })
    }).collect();

    serde_json::json!({
        "generated_at": report.generated_at,
        "total_runs": report.total_runs,
        "passed_runs": report.passed_runs,
        "pass_rate_pct": report.pass_rate_pct,
        "avg_rounds": report.avg_rounds,
        "avg_time_secs": report.avg_time_ms / 1000.0,
        "hotspots": sprints,
        "summary": report.summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_result_ok() {
        let r = CommandResult::ok("test", serde_json::json!({"key": "value"}));
        assert!(r.success);
        assert_eq!(r.command, "test");
        let json = r.to_json();
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn test_command_result_err() {
        let r = CommandResult::err("test", "something went wrong");
        assert!(!r.success);
        assert_eq!(r.error.as_deref(), Some("something went wrong"));
    }

    #[test]
    fn test_print_json_mode() {
        let r = CommandResult::ok_with_msg("test", serde_json::json!({}), "hello");
        // Just verify it doesn't panic
        r.print(true);
        r.print(false);
    }

    #[test]
    fn test_archive_list_to_json() {
        // Verify function exists and returns valid JSON
        let val = archive_list_to_json(&[]);
        assert_eq!(val["total"], 0);
    }

    #[test]
    fn test_metadata_timestamp() {
        let meta = CommandMetadata::default();
        assert!(!meta.timestamp.is_empty());
        assert!(!meta.version.is_empty());
    }
}