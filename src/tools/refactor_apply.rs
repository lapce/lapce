//! One-Click Refactoring Apply - Apply refactoring suggestions with single command.
//!
//! This module provides one-click refactoring functionality:
//! - Preview refactoring changes before applying
//! - Apply refactoring with automatic backup
//! - Rollback if something goes wrong
//! - Track refactoring history

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::tools::semantic_refactor::{RefactorType, RiskLevel, Location};

/// A refactoring suggestion that can be applied.
#[derive(Debug, Clone)]
pub struct RefactorSuggestion {
    /// Unique ID.
    pub id: String,
    /// Type of refactoring.
    pub refactor_type: RefactorType,
    /// Target name.
    pub target_name: String,
    /// Description.
    pub description: String,
    /// Location info.
    pub location: Location,
    /// Risk assessment.
    pub risk_level: RiskLevel,
    /// The suggested new code.
    pub suggested_code: String,
    /// Original code.
    pub original_code: String,
    /// Files that will be affected.
    pub affected_files: Vec<PathBuf>,
    /// Estimated change size (lines).
    pub change_size: ChangeSize,
}

/// Size of the change.
#[derive(Debug, Clone, Copy)]
pub struct ChangeSize {
    pub lines_added: usize,
    pub lines_removed: usize,
    pub files_changed: usize,
}

/// Result of applying a refactoring.
#[derive(Debug)]
pub struct ApplyResult {
    /// Whether the refactoring was successful.
    pub success: bool,
    /// Files that were modified.
    pub modified_files: Vec<PathBuf>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Backup path if created.
    pub backup_path: Option<PathBuf>,
}

/// Refactoring applier with backup and rollback support.
pub struct RefactorApplier {
    /// Active refactorings.
    active: Arc<RwLock<Vec<ActiveRefactor>>>,
    /// Backup directory.
    backup_dir: PathBuf,
    /// Enable auto-backup.
    auto_backup: bool,
}

/// An active refactoring that can be rolled back.
#[derive(Debug)]
struct ActiveRefactor {
    id: String,
    timestamp: u64,
    original_files: HashMap<PathBuf, String>,
    modified_files: Vec<PathBuf>,
}

impl RefactorApplier {
    pub fn new(backup_dir: PathBuf) -> Self {
        Self {
            active: Arc::new(RwLock::new(Vec::new())),
            backup_dir,
            auto_backup: true,
        }
    }

    /// Apply a refactoring suggestion.
    pub async fn apply(&self, suggestion: &RefactorSuggestion) -> ApplyResult {
        if suggestion.risk_level == RiskLevel::Risky || suggestion.risk_level == RiskLevel::High {
            return ApplyResult {
                success: false,
                modified_files: vec![],
                error: Some(format!("Refactoring {:?} has high risk level. Please review manually.", suggestion.refactor_type)),
                backup_path: None,
            };
        }

        let mut modified_files = Vec::new();
        let mut original_files = HashMap::new();
        let mut backup_path = None;

        // Read original file
        let original_content = match tokio::fs::read_to_string(&suggestion.location.file).await {
            Ok(c) => c,
            Err(e) => return ApplyResult {
                success: false,
                modified_files: vec![],
                error: Some(format!("Failed to read file: {}", e)),
                backup_path: None,
            },
        };

        // Create backup if enabled
        if self.auto_backup {
            if let Some(path) = self.create_backup(&suggestion.location.file, &original_content).await {
                backup_path = Some(path);
            }
        }

        // Store original
        original_files.insert(suggestion.location.file.clone(), original_content.clone());

        // Apply the refactoring
        let lines: Vec<&str> = original_content.lines().collect();
        let mut new_lines = lines.clone();

        // Replace lines
        let start = suggestion.location.start_line.saturating_sub(1);
        let end = suggestion.location.end_line.saturating_sub(1);

        if start <= end && end < new_lines.len() {
            new_lines.splice(start..=end, suggestion.suggested_code.lines());
        }

        let new_content = new_lines.join("\n");

        // Write back
        match tokio::fs::write(&suggestion.location.file, &new_content).await {
            Ok(_) => {
                modified_files.push(suggestion.location.file.clone());

                // Track this refactoring for potential rollback
                let active = ActiveRefactor {
                    id: suggestion.id.clone(),
                    timestamp: current_timestamp(),
                    original_files,
                    modified_files: modified_files.clone(),
                };

                self.active.write().await.push(active);

                ApplyResult {
                    success: true,
                    modified_files,
                    error: None,
                    backup_path,
                }
            }
            Err(e) => ApplyResult {
                success: false,
                modified_files: vec![],
                error: Some(format!("Failed to write file: {}", e)),
                backup_path,
            },
        }
    }

    /// Rollback a refactoring by ID.
    pub async fn rollback(&self, id: &str) -> ApplyResult {
        let mut active = self.active.write().await;

        if let Some(pos) = active.iter().position(|r| r.id == id) {
            let refactor = active.remove(pos);
            let mut modified = Vec::new();
            let mut errors = Vec::new();

            for (file, content) in refactor.original_files {
                match tokio::fs::write(&file, content).await {
                    Ok(_) => modified.push(file),
                    Err(e) => errors.push(format!("{}: {}", file.display(), e)),
                }
            }

            ApplyResult {
                success: errors.is_empty(),
                modified_files: modified,
                error: if errors.is_empty() { None } else { Some(errors.join("; ")) },
                backup_path: None,
            }
        } else {
            ApplyResult {
                success: false,
                modified_files: vec![],
                error: Some(format!("Refactoring {} not found in active history", id)),
                backup_path: None,
            }
        }
    }

    /// Create a backup of a file.
    async fn create_backup(&self, path: &PathBuf, content: &str) -> Option<PathBuf> {
        let backup_name = format!(
            "{}_{}.backup",
            path.file_name()?.to_str()?,
            current_timestamp()
        );
        let backup_path = self.backup_dir.join(&backup_name);

        tokio::fs::write(&backup_path, content).await.ok()?;
        Some(backup_path)
    }

    /// Get active refactoring history.
    pub async fn history(&self) -> Vec<RefactorHistoryEntry> {
        let active = self.active.read().await;
        active.iter().map(|r| RefactorHistoryEntry {
            id: r.id.clone(),
            timestamp: r.timestamp,
            modified_files: r.modified_files.clone(),
        }).collect()
    }
}

/// Entry in refactoring history.
#[derive(Debug, Clone)]
pub struct RefactorHistoryEntry {
    pub id: String,
    pub timestamp: u64,
    pub modified_files: Vec<PathBuf>,
}

/// Quick refactoring menu for one-click operations.
pub struct QuickRefactorMenu {
    suggestions: Arc<RwLock<Vec<RefactorSuggestion>>>,
}

impl QuickRefactorMenu {
    pub fn new() -> Self {
        Self {
            suggestions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a suggestion to the menu.
    pub async fn add(&self, suggestion: RefactorSuggestion) {
        self.suggestions.write().await.push(suggestion);
    }

    /// Get all suggestions.
    pub async fn get_all(&self) -> Vec<RefactorSuggestion> {
        self.suggestions.read().await.clone()
    }

    /// Get suggestion by ID.
    pub async fn get(&self, id: &str) -> Option<RefactorSuggestion> {
        self.suggestions.read().await.iter()
            .find(|s| s.id == id)
            .cloned()
    }

    /// Clear all suggestions.
    pub async fn clear(&self) {
        self.suggestions.write().await.clear();
    }

    /// Get suggestions by risk level.
    pub async fn by_risk(&self, max_risk: RiskLevel) -> Vec<RefactorSuggestion> {
        let risk_order = |r: &RiskLevel| match r {
            RiskLevel::Safe => 0,
            RiskLevel::Low => 1,
            RiskLevel::Medium => 2,
            RiskLevel::High => 3,
            RiskLevel::Risky => 4,
        };

        self.suggestions.read().await.iter()
            .filter(|s| risk_order(&s.risk_level) <= risk_order(&max_risk))
            .cloned()
            .collect()
    }

    /// Format suggestions as a selectable menu.
    pub async fn format_menu(&self) -> String {
        let suggestions = self.suggestions.read().await;
        let mut output = String::from("\n=== Quick Refactor Menu ===\n\n");

        for (i, s) in suggestions.iter().enumerate() {
            output.push_str(&format!(
                "[{}] {} - {} (Risk: {:?})\n    {}\n\n",
                i + 1,
                s.refactor_type.name(),
                s.target_name,
                s.risk_level,
                s.description
            ));
        }

        output
    }
}

impl Default for QuickRefactorMenu {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: refactor_apply.rs:316")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quick_refactor_menu() {
        let menu = QuickRefactorMenu::new();

        let suggestion = RefactorSuggestion {
            id: "test_1".to_string(),
            refactor_type: RefactorType::ExtractMethod,
            target_name: "process_data".to_string(),
            description: "Extract processing logic".to_string(),
            location: Location {
                file: PathBuf::from("test.rs"),
                start_line: 10,
                end_line: 20,
                start_column: 0,
                end_column: 0,
            },
            risk_level: RiskLevel::Low,
            suggested_code: "fn process_data() {}".to_string(),
            original_code: "// old code".to_string(),
            affected_files: vec![PathBuf::from("test.rs")],
            change_size: ChangeSize {
                lines_added: 5,
                lines_removed: 10,
                files_changed: 1,
            },
        };

        menu.add(suggestion).await;
        let all = menu.get_all().await;
        assert_eq!(all.len(), 1);

        let by_risk = menu.by_risk(RiskLevel::Medium).await;
        assert_eq!(by_risk.len(), 1);
    }

    #[tokio::test]
    async fn test_menu_format() {
        let menu = QuickRefactorMenu::new();
        let formatted = menu.format_menu().await;
        assert!(formatted.contains("Quick Refactor Menu"));
    }
}
