//! Apply-to-Editor — send AI-generated code changes to the IDE.
//!
//! Ported from Claude Code's IDE integration pattern. Uses a lockfile
//! (~/.deepseek-carp/ide/<port>.lock) to discover the IDE connection.
//! Sends old_string→new_string diffs for the IDE to display and apply.
//!
//! ## Protocol (Claude Code compatible)
//!
//! 1. IDE creates lockfile: ~/.deepseek-carp/ide/{port}.lock
//!    Content: {"workspaceFolders":["..."], "pid":..., "transport":"ws", "authToken":"..."}
//! 2. Carp reads lockfile → connects via WebSocket/SSE
//! 3. Carp sends: {"type":"diff","filePath":"...","old":"...","new":"..."}
//! 4. IDE displays diff → user accepts/rejects

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Lockfile content for IDE discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeLockfile {
    /// Workspace folders the IDE has open.
    pub workspace_folders: Vec<String>,
    /// IDE process ID.
    pub pid: u32,
    /// Transport protocol (ws or sse).
    pub transport: String,
    /// Authentication token for connection.
    pub auth_token: Option<String>,
    /// IDE name (vscode, cursor, windsurf, jetbrains, etc.)
    pub ide_name: Option<String>,
}

/// A code edit to apply in the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeEdit {
    /// File path (relative to workspace).
    pub file_path: String,
    /// Original content (for diff preview).
    pub old_content: String,
    /// New content (after edit).
    pub new_content: String,
    /// Optional title for the diff tab.
    pub tab_name: Option<String>,
}

/// IDE connection discovered from lockfile.
#[derive(Debug, Clone)]
pub struct IdeConnection {
    pub transport: String,
    pub url: String,
    pub auth_token: Option<String>,
    pub ide_name: String,
}

/// IDE Connector — discovers and communicates with editor.
pub struct IdeConnector {
    /// Path to IDE lockfiles: ~/.deepseek-carp/ide/
    lock_dir: PathBuf,
}

impl Default for IdeConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl IdeConnector {
    pub fn new() -> Self {
        let lock_dir = crate::config::paths::config_file()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("ide");
        std::fs::create_dir_all(&lock_dir).ok();
        Self { lock_dir }
    }

    /// Scan for running IDEs by reading lockfiles.
    pub fn discover(&self) -> Vec<IdeConnection> {
        let mut connections = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&self.lock_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "lock") {
                    continue;
                }

                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(lock) = serde_json::from_str::<IdeLockfile>(&content) {
                        let port = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("8734");

                        connections.push(IdeConnection {
                            transport: lock.transport.clone(),
                            url: if lock.transport == "ws" {
                                format!("ws://127.0.0.1:{}", port)
                            } else {
                                format!("http://127.0.0.1:{}", port)
                            },
                            auth_token: lock.auth_token.clone(),
                            ide_name: lock.ide_name.unwrap_or_else(|| "unknown".into()),
                        });
                    }
                }
            }
        }

        tracing::info!(count=connections.len(), "IDE connections discovered");
        connections
    }

    /// Apply an edit to the connected editor.
    /// Sends the diff to all connected IDEs.
    pub async fn apply_edit(&self, edit: &IdeEdit) -> Result<usize, String> {
        let connections = self.discover();
        let mut count = 0;

        for conn in &connections {
            let payload = serde_json::json!({
                "type": "diff",
                "filePath": edit.file_path,
                "old": edit.old_content,
                "new": edit.new_content,
                "tabName": edit.tab_name,
            });

            let client = reqwest::Client::new();
            let url = format!("{}/apply", conn.url);

            let mut builder = client.post(&url).json(&payload);
            if let Some(ref token) = conn.auth_token {
                builder = builder.header("Authorization", format!("Bearer {}", token));
            }

            match builder.send().await {
                Ok(resp) if resp.status().is_success() => {
                    count += 1;
                    tracing::info!(ide=%conn.ide_name, file=%edit.file_path, "Edit applied to IDE");
                }
                Ok(resp) => {
                    tracing::warn!(ide=%conn.ide_name, status=%resp.status(), "IDE rejected edit");
                }
                Err(e) => {
                    tracing::debug!(ide=%conn.ide_name, error=%e, "IDE connection failed (may be offline)");
                }
            }
        }

        Ok(count)
    }

    /// Apply multiple edits in batch.
    pub async fn apply_edits(&self, edits: &[IdeEdit]) -> Result<usize, String> {
        let mut total = 0;
        for edit in edits {
            total += self.apply_edit(edit).await?;
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ide_lockfile_parse() {
        let json = r#"{"workspace_folders":["/home/user/project"],"pid":12345,"transport":"ws","auth_token":"tok123","ide_name":"vscode"}"#;
        let lock: IdeLockfile = serde_json::from_str(json).unwrap();
        assert_eq!(lock.workspace_folders[0], "/home/user/project");
        assert_eq!(lock.ide_name.unwrap(), "vscode");
    }

    #[test]
    fn test_ide_connector_creation() {
        let connector = IdeConnector::new();
        // No IDEs running — should return empty
        let connections = connector.discover();
        assert!(connections.is_empty());
    }
}
