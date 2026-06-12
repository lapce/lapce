//! IDE Bidirectional Sync — inspired by mattpocock/skills .claude-plugin pattern.
//!
//! Provides a lightweight sync protocol between deepseek-carp and dscarp-lapce:
//! - Watches `.carp/sync/` directory for changes from either side
//! - Syncs skill definitions, config, and state between processes
//! - Uses file-based events (no external dependencies)
//!
//! ## Protocol
//!
//! ```text
//! .carp/sync/
//!   skills/          ← SKILL.md files synced from IDE
//!   config.json      ← shared configuration
//!   state.json       ← current agent state
//!   events/          ← event log for cross-process communication
//! ```
//!
//! ## Architecture
//!
//! ```text
//! deepseek-carp (src/skills/ide_sync.rs)
//!       ↕  file-based sync via .carp/sync/
//! dscarp-lapce (editor plugin)
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Sync configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeSyncConfig {
    /// Sync directory (default: `.carp/sync/`).
    pub sync_dir: PathBuf,
    /// Poll interval in milliseconds (default: 2000).
    pub poll_ms: u64,
    /// Whether to auto-install skills from IDE (default: true).
    pub auto_install_skills: bool,
    /// Whether to emit events for IDE consumption (default: true).
    pub emit_events: bool,
}

impl Default for IdeSyncConfig {
    fn default() -> Self {
        Self {
            sync_dir: PathBuf::from(".carp/sync"),
            poll_ms: 2000,
            auto_install_skills: true,
            emit_events: true,
        }
    }
}

/// A sync event — represents a state change that the other side should process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEvent {
    pub id: String,
    pub kind: SyncEventKind,
    pub source: String,       // "deepseek-carp" or "dscarp-lapce"
    pub timestamp: u64,       // unix millis
    pub payload: String,
}

/// Types of sync events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SyncEventKind {
    /// A skill was installed/updated.
    SkillUpdated,
    /// A skill was removed.
    SkillRemoved,
    /// Configuration changed.
    ConfigChanged,
    /// Agent state changed (current task, context).
    StateChanged,
    /// IDE requested a specific action.
    ActionRequested,
}

/// Shared state synced between deepseek-carp and IDE.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// Current task being worked on.
    pub current_task: Option<String>,
    /// Last review result summary.
    pub last_review: Option<String>,
    /// Active skills list (names only, for lightweight sync).
    pub active_skills: Vec<String>,
    /// Arbitrary key-value state for extensibility.
    pub extras: HashMap<String, String>,
}

/// The IDE sync engine.
pub struct IdeSync {
    config: IdeSyncConfig,
    /// Last known event ID processed.
    last_event_id: Option<String>,
    /// Current state.
    state: SyncState,
}

impl IdeSync {
    /// Create a new IDE sync engine.
    pub fn new(config: IdeSyncConfig) -> Result<Self> {
        let sync_dir = &config.sync_dir;
        if !sync_dir.exists() {
            std::fs::create_dir_all(sync_dir)
                .with_context(|| format!("Failed to create sync dir: {}", sync_dir.display()))?;
            std::fs::create_dir_all(sync_dir.join("skills"))
                .ok();
            std::fs::create_dir_all(sync_dir.join("events"))
                .ok();
        }

        Ok(Self {
            config,
            last_event_id: None,
            state: SyncState::default(),
        })
    }

    /// Read the current sync state.
    pub fn read_state(&self) -> Result<SyncState> {
        let state_path = self.config.sync_dir.join("state.json");
        if !state_path.exists() {
            return Ok(SyncState::default());
        }
        let content = std::fs::read_to_string(&state_path)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Write the current sync state (deepseek-carp → IDE).
    pub fn write_state(&self, state: &SyncState) -> Result<()> {
        let state_path = self.config.sync_dir.join("state.json");
        let content = serde_json::to_string_pretty(state)?;
        std::fs::write(&state_path, &content)?;
        Ok(())
    }

    /// Emit a sync event.
    pub fn emit_event(&self, kind: SyncEventKind, payload: &str) -> Result<()> {
        if !self.config.emit_events {
            return Ok(());
        }

        let event = SyncEvent {
            id: format!("evt_{}", chrono_millis()),
            kind,
            source: "deepseek-carp".to_string(),
            timestamp: chrono_millis(),
            payload: payload.to_string(),
        };

        let event_path = self.config.sync_dir
            .join("events")
            .join(format!("{}.json", event.id));

        let content = serde_json::to_string_pretty(&event)?;
        std::fs::write(&event_path, &content)?;

        // Cleanup old events (keep last 50)
        cleanup_old_events(&self.config.sync_dir.join("events"), 50);

        Ok(())
    }

    /// Poll for new events from IDE (dscarp-lapce → deepseek-carp).
    pub fn poll_events(&mut self) -> Result<Vec<SyncEvent>> {
        let events_dir = self.config.sync_dir.join("events");
        if !events_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<_> = std::fs::read_dir(&events_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "json").unwrap_or(false))
            .collect();

        // Sort by name (= timestamp) ascending
        entries.sort_by_key(|e| e.file_name());

        let mut new_events = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let event_id = name.trim_end_matches(".json").to_string();

            // Skip already-processed events
            if let Some(ref last) = self.last_event_id {
                if event_id <= *last {
                    continue;
                }
            }

            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(event) = serde_json::from_str::<SyncEvent>(&content) {
                    new_events.push(event);
                }
            }
        }

        if let Some(last) = new_events.last() {
            self.last_event_id = Some(last.id.clone());
        }

        Ok(new_events)
    }

    /// Sync a SKILL.md file from IDE sync dir into deepseek-carp's skill store.
    pub fn install_skill_from_ide(&self, skill_name: &str, store_dir: &Path) -> Result<bool> {
        let src = self.config.sync_dir.join("skills").join(format!("{}.md", skill_name));
        if !src.exists() {
            return Ok(false);
        }

        let dest = store_dir.join(format!("{}.md", skill_name));
        std::fs::copy(&src, &dest)?;
        info!("Installed skill '{}' from IDE sync", skill_name);
        Ok(true)
    }

    /// Sync a SKILL.md file from deepseek-carp to the IDE sync dir.
    pub fn export_skill_to_ide(&self, skill_name: &str, store_dir: &Path) -> Result<bool> {
        let src = store_dir.join(format!("{}.md", skill_name));
        if !src.exists() {
            return Ok(false);
        }

        let dest = self.config.sync_dir.join("skills").join(format!("{}.md", skill_name));
        std::fs::copy(&src, &dest)?;

        self.emit_event(SyncEventKind::SkillUpdated, skill_name)?;
        info!("Exported skill '{}' to IDE sync", skill_name);
        Ok(true)
    }

    /// Get all skills available in the IDE sync directory.
    pub fn list_ide_skills(&self) -> Result<Vec<String>> {
        let skills_dir = self.config.sync_dir.join("skills");
        if !skills_dir.exists() {
            return Ok(Vec::new());
        }

        let mut skills = Vec::new();
        for entry in std::fs::read_dir(&skills_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                    skills.push(name.to_string());
                }
            }
        }

        Ok(skills)
    }
}

/// Get current time in milliseconds since epoch.
fn chrono_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Keep only the N most recent event files.
fn cleanup_old_events(events_dir: &Path, keep: usize) {
    let mut entries: Vec<_> = match std::fs::read_dir(events_dir) {
        Ok(e) => e.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };

    if entries.len() <= keep {
        return;
    }

    entries.sort_by_key(|e| e.file_name());
    let to_remove = entries.len() - keep;
    for entry in entries.iter().take(to_remove) {
        std::fs::remove_file(entry.path()).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_sync() -> (tempfile::TempDir, IdeSync) {
        let tmp = tempfile::tempdir().unwrap();
        let sync_dir = tmp.path().join(".carp").join("sync");
        let config = IdeSyncConfig {
            sync_dir: sync_dir.clone(),
            poll_ms: 100,
            auto_install_skills: true,
            emit_events: true,
        };
        let sync = IdeSync::new(config).unwrap();
        (tmp, sync)
    }

    #[test]
    fn test_ide_sync_create_dir() {
        let (tmp, _sync) = setup_test_sync();
        assert!(tmp.path().join(".carp").join("sync").exists());
        assert!(tmp.path().join(".carp").join("sync").join("skills").exists());
        assert!(tmp.path().join(".carp").join("sync").join("events").exists());
    }

    #[test]
    fn test_ide_sync_state_roundtrip() {
        let (_tmp, sync) = setup_test_sync();

        let state = SyncState {
            current_task: Some("test-task".to_string()),
            active_skills: vec!["tdd".to_string(), "handoff".to_string()],
            ..Default::default()
        };

        sync.write_state(&state).unwrap();
        let read_back = sync.read_state().unwrap();
        assert_eq!(read_back.current_task, Some("test-task".to_string()));
        assert_eq!(read_back.active_skills.len(), 2);
    }

    #[test]
    fn test_ide_sync_emit_and_poll_events() {
        let (_tmp, mut sync) = setup_test_sync();

        sync.emit_event(SyncEventKind::SkillUpdated, "my-skill").unwrap();

        let events = sync.poll_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, SyncEventKind::SkillUpdated);
        assert_eq!(events[0].payload, "my-skill");
        assert_eq!(events[0].source, "deepseek-carp");
    }

    #[test]
    fn test_ide_sync_skill_export_import() {
        let (_tmp, sync) = setup_test_sync();

        // Create a temp skill store
        let store = PathBuf::from(std::env::temp_dir()).join(format!("ide_test_store_{}", std::process::id()));
        fs::create_dir_all(&store).ok();

        // Write a skill to the store
        let skill_content = "---\nname: ide-test\ndescription: Test\n---\n\n## Instructions\n\nTest.\n";
        fs::write(store.join("ide-test.md"), skill_content).unwrap();

        // Export to IDE
        sync.export_skill_to_ide("ide-test", &store).unwrap();
        assert!(sync.list_ide_skills().unwrap().contains(&"ide-test".to_string()));

        // Remove from store, import back
        fs::remove_file(store.join("ide-test.md")).ok();
        sync.install_skill_from_ide("ide-test", &store).unwrap();
        assert!(store.join("ide-test.md").exists());

        // Cleanup
        fs::remove_dir_all(&store).ok();
    }
}