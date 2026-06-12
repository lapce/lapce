//! Incremental Context Update - Diff-only context for efficiency.
//!
//! This module provides:
//! - Efficient diff computation
//! - Incremental context updates
//! - Change tracking
//! - Minimal context reconstruction

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A change in the codebase.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub change_type: ChangeType,
    pub old_content: Option<String>,
    pub new_content: Option<String>,
    pub diff: Option<String>,
    pub line_ranges: Vec<LineRange>,
}

/// Type of change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// A range of lines.
#[derive(Debug, Clone)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

/// Incremental context state.
#[derive(Debug, Clone)]
pub struct IncrementalContext {
    pub file_hashes: HashMap<String, u64>,
    pub last_context: Option<String>,
    pub changes_since_last: Vec<FileChange>,
}

/// Incremental context manager.
pub struct IncrementalContextManager {
    state: Arc<RwLock<IncrementalContext>>,
    max_diff_size: usize,
}

impl IncrementalContextManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(IncrementalContext {
                file_hashes: HashMap::new(),
                last_context: None,
                changes_since_last: Vec::new(),
            })),
            max_diff_size: 1000,
        }
    }

    /// Update context with changes.
    pub async fn update(&self, changes: Vec<FileChange>) -> ContextUpdateResult {
        let mut state = self.state.write().await;
        let mut updates = Vec::new();
        let mut removed_files = Vec::new();

        for change in &changes {
            let _old_hash = state.file_hashes.get(&change.path).copied();
            let new_hash = if change.change_type != ChangeType::Deleted {
                Some(simple_hash(change.new_content.as_deref().unwrap_or("")))
            } else {
                None
            };

            // Update hash
            if change.change_type == ChangeType::Deleted {
                state.file_hashes.remove(&change.path);
                removed_files.push(change.path.clone());
            } else if let Some(hash) = new_hash {
                state.file_hashes.insert(change.path.clone(), hash);
            }

            // Generate incremental update
            let update = match change.change_type {
                ChangeType::Added => {
                    IncrementalUpdate {
                        path: change.path.clone(),
                        update_type: UpdateType::Addition,
                        content: change.new_content.clone(),
                        diff: change.diff.clone(),
                        line_ranges: change.line_ranges.clone(),
                    }
                }
                ChangeType::Modified => {
                    // Compute actual diff if not provided
                    let diff = change.diff.clone().or_else(|| {
                        compute_diff(
                            change.old_content.as_deref().unwrap_or(""),
                            change.new_content.as_deref().unwrap_or(""),
                        )
                    });

                    IncrementalUpdate {
                        path: change.path.clone(),
                        update_type: UpdateType::Modification,
                        content: change.new_content.clone(),
                        diff,
                        line_ranges: change.line_ranges.clone(),
                    }
                }
                ChangeType::Deleted => {
                    IncrementalUpdate {
                        path: change.path.clone(),
                        update_type: UpdateType::Deletion,
                        content: None,
                        diff: change.diff.clone(),
                        line_ranges: change.line_ranges.clone(),
                    }
                }
                ChangeType::Renamed => {
                    IncrementalUpdate {
                        path: change.path.clone(),
                        update_type: UpdateType::Rename,
                        content: change.new_content.clone(),
                        diff: None,
                        line_ranges: Vec::new(),
                    }
                }
            };

            updates.push(update);
        }

        let result = ContextUpdateResult {
            updates,
            removed_files,
            context_delta: self.estimate_delta(&state),
        };

        state.changes_since_last.extend(changes);

        result
    }

    /// Estimate the delta size.
    fn estimate_delta(&self, state: &IncrementalContext) -> usize {
        state.changes_since_last.iter()
            .map(|c| {
                match c.change_type {
                    ChangeType::Deleted => c.old_content.as_ref().map(|s| s.len()).unwrap_or(0),
                    _ => c.new_content.as_ref().map(|s| s.len()).unwrap_or(0),
                }
            })
            .sum()
    }

    /// Get diff-only context for a file.
    pub async fn get_diff_context(&self, _path: &str, old_content: &str, new_content: &str) -> String {
        if let Some(diff) = compute_diff(old_content, new_content) {
            if diff.len() <= self.max_diff_size {
                return diff;
            }
        }

        // Fallback to full content if diff is too large
        format!("// Full file content (diff too large):\n{}", new_content)
    }

    /// Reset incremental state.
    pub async fn reset(&self) {
        let mut state = self.state.write().await;
        state.last_context = None;
        state.changes_since_last.clear();
    }

    /// Get changes summary.
    pub async fn get_changes_summary(&self) -> ChangesSummary {
        let state = self.state.read().await;

        let additions = state.changes_since_last.iter()
            .filter(|c| c.change_type == ChangeType::Added)
            .count();
        let modifications = state.changes_since_last.iter()
            .filter(|c| c.change_type == ChangeType::Modified)
            .count();
        let deletions = state.changes_since_last.iter()
            .filter(|c| c.change_type == ChangeType::Deleted)
            .count();

        ChangesSummary {
            total_files: state.file_hashes.len(),
            additions,
            modifications,
            deletions,
            pending_changes: state.changes_since_last.len(),
        }
    }
}

impl Default for IncrementalContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct IncrementalUpdate {
    pub path: String,
    pub update_type: UpdateType,
    pub content: Option<String>,
    pub diff: Option<String>,
    pub line_ranges: Vec<LineRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateType {
    Addition,
    Modification,
    Deletion,
    Rename,
}

#[derive(Debug, Clone)]
pub struct ContextUpdateResult {
    pub updates: Vec<IncrementalUpdate>,
    pub removed_files: Vec<String>,
    pub context_delta: usize,
}

#[derive(Debug, Clone)]
pub struct ChangesSummary {
    pub total_files: usize,
    pub additions: usize,
    pub modifications: usize,
    pub deletions: usize,
    pub pending_changes: usize,
}

/// Simple hash function.
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    hash
}

/// Compute diff between two strings.
fn compute_diff(old: &str, new: &str) -> Option<String> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff_lines = Vec::new();
    let mut i = 0;
    let mut j = 0;

    // Simple LCS-based diff
    while i < old_lines.len() || j < new_lines.len() {
        if i < old_lines.len() && j < new_lines.len() {
            if old_lines[i] == new_lines[j] {
                diff_lines.push(format!("  {}", new_lines[j]));
                i += 1;
                j += 1;
            } else if i + 1 < old_lines.len() && old_lines[i + 1] == new_lines[j] {
                diff_lines.push(format!("- {}", old_lines[i]));
                i += 1;
            } else if j + 1 < new_lines.len() && old_lines[i] == new_lines[j + 1] {
                diff_lines.push(format!("+ {}", new_lines[j]));
                j += 1;
            } else {
                diff_lines.push(format!("- {}", old_lines[i]));
                diff_lines.push(format!("+ {}", new_lines[j]));
                i += 1;
                j += 1;
            }
        } else if i < old_lines.len() {
            diff_lines.push(format!("- {}", old_lines[i]));
            i += 1;
        } else if j < new_lines.len() {
            diff_lines.push(format!("+ {}", new_lines[j]));
            j += 1;
        }
    }

    let diff = diff_lines.join("\n");
    if diff.is_empty() {
        None
    } else {
        Some(diff)
    }
}

/// Diff parser to reconstruct from diff.
pub struct DiffParser;

impl DiffParser {
    /// Parse a diff and apply it.
    pub fn apply_diff(old: &str, diff: &str) -> String {
        let old_lines: Vec<&str> = old.lines().collect();
        let diff_lines: Vec<&str> = diff.lines().collect();

        let mut result = Vec::new();
        let mut old_idx = 0;

        for line in diff_lines {
            if line.starts_with("  ") {
                // Context line
                result.push(&line[2..]);
                old_idx += 1;
            } else if line.starts_with("- ") {
                // Removed line - skip from old
                old_idx += 1;
            } else if line.starts_with("+ ") {
                // Added line
                result.push(&line[2..]);
            }
        }

        // Add remaining old lines
        while old_idx < old_lines.len() {
            result.push(old_lines[old_idx]);
            old_idx += 1;
        }

        result.join("\n")
    }

    /// Extract changed line ranges from diff.
    pub fn extract_ranges(diff: &str) -> Vec<LineRange> {
        let diff_lines: Vec<&str> = diff.lines().collect();
        let mut ranges = Vec::new();
        let mut start = 0;
        let mut in_range = false;

        for (i, line) in diff_lines.iter().enumerate() {
            if line.starts_with("+ ") || line.starts_with("- ") {
                if !in_range {
                    start = i;
                    in_range = true;
                }
            } else if in_range {
                ranges.push(LineRange { start, end: i });
                in_range = false;
            }
        }

        if in_range {
            ranges.push(LineRange { start, end: diff_lines.len() });
        }

        ranges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_update_added_file() {
        let manager = IncrementalContextManager::new();

        let changes = vec![FileChange {
            path: "new_file.rs".to_string(),
            change_type: ChangeType::Added,
            old_content: None,
            new_content: Some("fn main() {}".to_string()),
            diff: None,
            line_ranges: vec![LineRange { start: 1, end: 1 }],
        }];

        let result = manager.update(changes).await;
        assert_eq!(result.updates.len(), 1);
        assert_eq!(result.updates[0].update_type, UpdateType::Addition);
    }

    #[test]
    fn test_compute_diff() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3";

        let diff = compute_diff(old, new);
        assert!(diff.is_some());
        let diff_str = diff.unwrap();
        assert!(diff_str.contains("- line2"));
        assert!(diff_str.contains("+ modified"));
    }

    #[test]
    fn test_diff_parser() {
        let old = "line1\nline2\nline3";
        let diff = "- line2\n+ modified";

        let result = DiffParser::apply_diff(old, diff);
        assert!(result.contains("modified"));
        assert!(!result.contains("line2"));
    }
}
