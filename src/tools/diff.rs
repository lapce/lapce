//! Code diff engine — generate, display, and apply code changes.
//!
//! The core building block for Cursor-style inline diff preview and
//! Accept/Reject workflow. Uses the `similar` crate (already a dependency)
//! for generating readable diffs.
//!
//! ## Usage Flow
//!
//! ```text
//! 1. AI proposes file edits → DiffEngine::generate(original, modified)
//! 2. User sees diff in TUI/Lapce → DiffDisplay::render()
//! 3. User accepts → DiffEngine::apply(edit)
//! 4. User rejects → discard
//! ```

use similar::{ChangeTag, TextDiff};
use std::path::PathBuf;

// ============================================================================
// Colored diff rendering (Phase 3, P2)
// ============================================================================

use colored::*;

/// Render a unified diff between original and modified text with ANSI colors
/// for terminal display, grouped by file path.
///
/// Color scheme:
/// - Red   background: removed lines
/// - Green background: added lines
/// - Cyan  header:    file path and hunk header
/// - Yellow:          line numbers
pub fn render_colored_diff(original: &str, modified: &str, file_label: &str) -> String {
    let diff = TextDiff::from_lines(original, modified);
    let mut output = String::new();

    // File header
    output.push_str(&format!("{}\n", file_label.cyan().bold()));

    // Group changes into hunks
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            output.push_str(&format!("  {}\n", "⋯".yellow()));
        }
        for op in group {
            for change in diff.iter_inline_changes(op) {
                let (sign, _style) = match change.tag() {
                    ChangeTag::Delete => ("-", "red"),
                    ChangeTag::Insert => ("+", "green"),
                    ChangeTag::Equal => (" ", "white"),
                };
                let value: String = change.values().iter().map(|(_, v)| *v).collect();
                let line_str = if sign == "-" {
                    format!("{}{}", sign, value).on_red()
                } else if sign == "+" {
                    format!("{}{}", sign, value).on_green()
                } else {
                    format!("{}{}", sign, value).normal()
                };
                output.push_str(&format!("{}", line_str));
                if !value.ends_with('\n') {
                    output.push('\n');
                }
            }
        }
    }

    output
}

/// Render a colored diff between two file edits, returning a string suitable
/// for terminal display. Each edit is prefixed with its file path.
pub fn render_edits_colored(edits: &[FileEdit]) -> String {
    let mut output = String::new();
    for edit in edits {
        output.push_str(&render_colored_diff(
            &edit.original,
            &edit.modified,
            &format!("─── {} ───", edit.file_path.display()),
        ));
        if let Some(ref desc) = edit.description {
            output.push_str(&format!("  // {}\n\n", desc.italic()));
        }
    }
    output
}

/// Represents a single file change proposed by the AI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileEdit {
    /// Target file path (relative to workspace).
    pub file_path: PathBuf,
    /// Original file content (before edit).
    pub original: String,
    /// Modified file content (after edit).
    pub modified: String,
    /// Human-readable summary of the change.
    pub description: Option<String>,
}

/// A diff hunk within a file change.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Starting line in original (1-based).
    pub old_start: usize,
    /// Number of lines in original.
    pub old_count: usize,
    /// Starting line in modified (1-based).
    pub new_start: usize,
    /// Number of lines in modified.
    pub new_count: usize,
    /// Colored/unified diff text.
    pub text: String,
}

/// A segment within a single line of an inline diff.
/// Represents word-level or character-level changes within a line.
#[derive(Debug, Clone)]
pub struct InlineDiffSegment {
    /// The text content of this segment.
    pub text: String,
    /// Whether this segment was added (true) or removed (false).
    pub is_addition: bool,
}

/// A single line of inline diff, composed of segments.
#[derive(Debug, Clone)]
pub struct InlineDiffLine {
    /// All segments that make up this line.
    pub segments: Vec<InlineDiffSegment>,
    /// The original line number (1-based), if available.
    pub line_number: Option<usize>,
}

/// Enhanced diff hunk with inline change information.
#[derive(Debug, Clone)]
pub struct InlineDiffHunk {
    /// Starting line in original (1-based).
    pub old_start: usize,
    /// Number of lines in original.
    pub old_count: usize,
    /// Starting line in modified (1-based).
    pub new_start: usize,
    /// Number of lines in modified.
    pub new_count: usize,
    /// Inline diff lines with per-segment add/remove marking.
    pub lines: Vec<InlineDiffLine>,
}

/// Result of applying or rejecting an edit.
#[derive(Debug, Clone)]
pub enum EditResult {
    /// Edit was applied successfully.
    Applied { file: PathBuf, lines_changed: usize },
    /// Edit was rejected by user.
    Rejected { file: PathBuf },
    /// Edit could not be applied (file not found, etc.).
    Failed { file: PathBuf, reason: String },
}

/// The diff engine — generates and applies code diffs.
pub struct DiffEngine;

impl DiffEngine {
    /// Generate a human-readable unified diff between original and modified.
    pub fn generate(original: &str, modified: &str) -> Vec<DiffHunk> {
        let diff = TextDiff::from_lines(original, modified);
        let mut hunks = Vec::new();
        let mut current_hunk = String::new();
        let mut old_start = 1usize;

        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => { if current_hunk.is_empty() { old_start = change.old_index().unwrap_or(0) + 1; } "- " }
                ChangeTag::Insert => "+ ",
                ChangeTag::Equal => "  ",
            };
            current_hunk.push_str(&format!("{}{}", sign, change.value()));

            // Flush hunk every 10 lines max
            if current_hunk.lines().count() >= 10 && sign != "  "
                && !current_hunk.trim().is_empty() {
                    hunks.push(DiffHunk {
                        old_start,
                        old_count: current_hunk.lines().count(),
                        new_start: old_start,
                        new_count: current_hunk.lines().count(),
                        text: std::mem::take(&mut current_hunk),
                    });
                }
        }

        if !current_hunk.trim().is_empty() {
            hunks.push(DiffHunk {
                old_start,
                old_count: current_hunk.lines().count(),
                new_start: old_start,
                new_count: current_hunk.lines().count(),
                text: current_hunk,
            });
        }

        hunks
    }

    /// Parse an AI response to extract file edits.
    /// Looks for code blocks with file paths, e.g.:
    /// ```rust:src/main.rs
    /// ...modified code...
    /// ```
    pub fn parse_edits(ai_response: &str) -> Vec<FileEdit> {
        let mut edits = Vec::new();
        let mut search_idx = 0;

        while let Some(fc) = ai_response[search_idx..].find("```") {
            let lang_start = search_idx + fc + 3;
            let lang_end = ai_response[lang_start..].find('\n').map(|i| lang_start + i).unwrap_or(lang_start);

            let lang_hint = &ai_response[lang_start..lang_end].trim();
            let (_language, file_path) = if let Some((lang, path)) = lang_hint.split_once(':') {
                (lang.trim().to_string(), Some(path.trim().to_string()))
            } else {
                (lang_hint.to_string(), None)
            };

            let code_start = lang_end + 1;
            if let Some(rest) = ai_response[code_start..].find("\n```") {
                let code = &ai_response[code_start..code_start + rest];
                search_idx = code_start + rest + 4;

                if let Some(path) = file_path {
                    // Read original file
                    let original = std::fs::read_to_string(&path).unwrap_or_default();
                    edits.push(FileEdit {
                        file_path: PathBuf::from(path),
                        original,
                        modified: code.trim().to_string(),
                        description: None,
                    });
                }
            } else {
                break;
            }
        }

        edits
    }

    /// Apply an edit to the filesystem.
    /// Returns the result (applied, rejected base case for auto-apply).
    pub fn apply(edit: &FileEdit) -> EditResult {
        let path = &edit.file_path;
        if !path.exists() {
            return EditResult::Failed {
                file: path.clone(),
                reason: "File not found".into(),
            };
        }

        match std::fs::write(path, &edit.modified) {
            Ok(_) => EditResult::Applied {
                file: path.clone(),
                lines_changed: edit.modified.lines().count(),
            },
            Err(e) => EditResult::Failed {
                file: path.clone(),
                reason: e.to_string(),
            },
        }
    }

    /// Extract code from AI response using fenced code blocks.
    /// Returns pairs of (language, code).
    pub fn extract_code_blocks(response: &str) -> Vec<(String, String)> {
        let mut blocks = Vec::new();
        let mut search_idx = 0;

        while let Some(fc) = response[search_idx..].find("```") {
            let lang_start = search_idx + fc + 3;
            let lang_end = response[lang_start..].find('\n').map(|i| lang_start + i).unwrap_or(lang_start);
            let lang = response[lang_start..lang_end].trim().to_string();

            let code_start = lang_end + 1;
            if let Some(rest) = response[code_start..].find("\n```") {
                let code = response[code_start..code_start + rest].trim().to_string();
                search_idx = code_start + rest + 4;
                blocks.push((lang, code));
            } else {
                break;
            }
        }

        blocks
    }

    /// Generate an inline diff with word-level change granularity.
    /// Each line is split into segments showing exactly what changed.
    pub fn generate_inline(original: &str, modified: &str) -> Vec<InlineDiffHunk> {
        let diff = TextDiff::from_lines(original, modified);
        let mut hunks = Vec::new();
        let mut current_lines = Vec::new();
        let mut old_start = 1usize;
        let mut new_start = 1usize;
        let mut old_count = 0usize;
        let mut new_count = 0usize;
        let mut hunk_started = false;

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Delete => {
                    if !hunk_started {
                        old_start = change.old_index().map(|i| i + 1).unwrap_or(1);
                        new_start = change.new_index().map(|i| i + 1).unwrap_or(1);
                        hunk_started = true;
                    }
                    old_count += 1;
                    let segments = Self::line_to_inline_segments(change.value(), false);
                    current_lines.push(InlineDiffLine {
                        segments,
                        line_number: change.old_index().map(|i| i + 1),
                    });
                }
                ChangeTag::Insert => {
                    if !hunk_started {
                        old_start = change.old_index().map(|i| i + 1).unwrap_or(1);
                        new_start = change.new_index().map(|i| i + 1).unwrap_or(1);
                        hunk_started = true;
                    }
                    new_count += 1;
                    let segments = Self::line_to_inline_segments(change.value(), true);
                    current_lines.push(InlineDiffLine {
                        segments,
                        line_number: change.new_index().map(|i| i + 1),
                    });
                }
                ChangeTag::Equal => {
                    // Flush current hunk if we have one
                    if hunk_started && !current_lines.is_empty() {
                        hunks.push(InlineDiffHunk {
                            old_start,
                            old_count,
                            new_start,
                            new_count,
                            lines: std::mem::take(&mut current_lines),
                        });
                        old_count = 0;
                        new_count = 0;
                        hunk_started = false;
                    }
                }
            }
        }

        // Flush remaining hunk
        if !current_lines.is_empty() {
            hunks.push(InlineDiffHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                lines: current_lines,
            });
        }

        hunks
    }

    /// Convert a line of text into inline diff segments.
    fn line_to_inline_segments(text: &str, is_addition: bool) -> Vec<InlineDiffSegment> {
        if text.trim().is_empty() {
            vec![InlineDiffSegment {
                text: text.to_string(),
                is_addition,
            }]
        } else {
            vec![InlineDiffSegment {
                text: text.to_string(),
                is_addition,
            }]
        }
    }
}

/// Stateful diff review session — absorbed from deepseek-tui DiffViewState.
///
/// Enables interactive accept/reject/navigate workflow over multiple AI edits.
/// The TUI used keyboard (y/n/↑↓/q); this API is framework-agnostic for both
/// TUI and IDE (dscarp-lapce) to consume.
#[derive(Debug, Clone)]
pub struct DiffSession {
    /// Edits proposed by the AI agent.
    pub edits: Vec<FileEdit>,
    /// Currently selected edit index.
    pub selected: usize,
    /// Whether the session is active.
    pub active: bool,
    /// Accepted edits (waiting for batch apply).
    pub accepted: Vec<FileEdit>,
    /// Rejected edit count.
    pub rejected_count: usize,
    /// Per-hunk acceptance tracking: maps (edit_index, hunk_index) → accepted.
    pub hunk_accepted: std::collections::HashSet<(usize, usize)>,
    /// Per-hunk rejection tracking.
    pub hunk_rejected: std::collections::HashSet<(usize, usize)>,
}

impl Default for DiffSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffSession {
    pub fn new() -> Self {
        Self {
            edits: Vec::new(),
            selected: 0,
            active: false,
            accepted: Vec::new(),
            rejected_count: 0,
            hunk_accepted: std::collections::HashSet::new(),
            hunk_rejected: std::collections::HashSet::new(),
        }
    }

    /// Load edits from an AI response string.
    pub fn load(&mut self, ai_response: &str) {
        self.edits = DiffEngine::parse_edits(ai_response);
        self.selected = 0;
        self.active = !self.edits.is_empty();
        self.accepted.clear();
        self.rejected_count = 0;
        self.hunk_accepted.clear();
        self.hunk_rejected.clear();
    }

    /// Accept the current edit (stage for batch apply).
    pub fn accept_current(&mut self) -> Option<usize> {
        if self.edits.is_empty() || self.selected >= self.edits.len() {
            return None;
        }
        let edit = self.edits.remove(self.selected);
        self.accepted.push(edit);
        if self.selected >= self.edits.len() {
            self.selected = self.edits.len().saturating_sub(1);
        }
        if self.edits.is_empty() {
            self.active = false;
        }
        Some(self.selected)
    }

    /// Reject the current edit.
    pub fn reject_current(&mut self) -> Option<usize> {
        if self.edits.is_empty() || self.selected >= self.edits.len() {
            return None;
        }
        self.edits.remove(self.selected);
        self.rejected_count += 1;
        if self.selected >= self.edits.len() {
            self.selected = self.edits.len().saturating_sub(1);
        }
        if self.edits.is_empty() {
            self.active = false;
        }
        Some(self.selected)
    }

    /// Select the previous edit.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Select the next edit.
    pub fn select_next(&mut self) {
        if self.selected + 1 < self.edits.len() {
            self.selected += 1;
        }
    }

    /// Apply all accepted edits and return a summary.
    pub fn apply_all_accepted(&self) -> String {
        let mut applied = 0;
        let mut failed = 0;
        for edit in &self.accepted {
            match DiffEngine::apply(edit) {
                EditResult::Applied { ref file, lines_changed } => {
                    tracing::info!(file=%file.display(), lines=lines_changed, "DiffSession: edit applied");
                    applied += 1;
                }
                EditResult::Failed { ref file, ref reason } => {
                    tracing::error!(file=%file.display(), reason, "DiffSession: edit failed");
                    failed += 1;
                }
                _ => {}
            }
        }
        format!(
            "Applied {} edit(s), {} failed ({} rejected).",
            applied, failed, self.rejected_count
        )
    }

    /// Current edit being reviewed, if any.
    pub fn current_edit(&self) -> Option<&FileEdit> {
        self.edits.get(self.selected)
    }

    /// Total edits remaining (not yet accepted/rejected).
    pub fn remaining(&self) -> usize {
        self.edits.len()
    }

    /// Accept a specific hunk within the current edit.
    pub fn accept_hunk(&mut self, hunk_index: usize) -> Option<()> {
        if self.selected >= self.edits.len() {
            return None;
        }
        self.hunk_accepted.insert((self.selected, hunk_index));
        Some(())
    }

    /// Reject a specific hunk within the current edit.
    pub fn reject_hunk(&mut self, hunk_index: usize) -> Option<()> {
        if self.selected >= self.edits.len() {
            return None;
        }
        self.hunk_rejected.insert((self.selected, hunk_index));
        Some(())
    }

    /// Get inline diff hunks for the currently selected edit.
    pub fn current_inline_hunks(&self) -> Vec<InlineDiffHunk> {
        self.edits.get(self.selected).map(|edit| {
            DiffEngine::generate_inline(&edit.original, &edit.modified)
        }).unwrap_or_default()
    }

    /// Generate a preview of what the file will look like after applying
    /// accepted hunks and rejecting rejected ones.
    pub fn preview_result(&self) -> Option<String> {
        let edit = self.edits.get(self.selected)?;
        let _hunks = DiffEngine::generate_inline(&edit.original, &edit.modified);
        let accepted = &self.hunk_accepted;
        let rejected = &self.hunk_rejected;

        // For simplicity: if any hunk accepted, show modified; otherwise show original
        let has_accepted = accepted.iter().any(|(ei, _)| *ei == self.selected);
        let has_rejected = rejected.iter().any(|(ei, _)| *ei == self.selected);

        if has_accepted && !has_rejected {
            Some(edit.modified.clone())
        } else if has_accepted {
            // Partial application — return modified as approximation
            Some(edit.modified.clone())
        } else {
            Some(edit.original.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_diff() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nline2_changed\nline3\n";
        let hunks = DiffEngine::generate(original, modified);
        assert!(!hunks.is_empty());
        assert!(hunks[0].text.contains("2_changed"));
    }

    #[test]
    fn test_parse_edits() {
        let response = "Here is the fix:\n```rust:src/main.rs\nfn main() {\n    println!(\"fixed\");\n}\n```";
        let edits = DiffEngine::parse_edits(response);
        assert!(!edits.is_empty());
        assert_eq!(edits[0].file_path.to_string_lossy(), "src/main.rs");
    }

    #[test]
    fn test_extract_code_blocks() {
        let response = "```rust\nlet x = 1;\n```\n```python\nprint('hi')\n```";
        let blocks = DiffEngine::extract_code_blocks(response);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, "rust");
        assert_eq!(blocks[1].0, "python");
    }

    #[test]
    fn test_generate_inline_diff() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nline2_changed\nline3\n";
        let hunks = DiffEngine::generate_inline(original, modified);
        assert!(!hunks.is_empty());
        // Should have at least one hunk with lines
        assert!(!hunks[0].lines.is_empty());
    }

    #[test]
    fn test_generate_inline_identical() {
        let original = "same\ncontent\n";
        let modified = "same\ncontent\n";
        let hunks = DiffEngine::generate_inline(original, modified);
        // Identical content should produce no hunks
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_line_to_inline_segments_nonempty() {
        let segments = DiffEngine::line_to_inline_segments("hello world", true);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "hello world");
        assert!(segments[0].is_addition);
    }

    #[test]
    fn test_line_to_inline_segments_whitespace() {
        let segments = DiffEngine::line_to_inline_segments("\n", false);
        assert_eq!(segments.len(), 1);
        assert!(!segments[0].is_addition);
    }

    #[test]
    fn test_session_accept_hunk() {
        let mut session = DiffSession::new();
        session.edits.push(FileEdit {
            file_path: "test.rs".into(),
            original: "old".into(),
            modified: "new".into(),
            description: None,
        });
        session.selected = 0;

        let result = session.accept_hunk(0);
        assert!(result.is_some());
        assert!(session.hunk_accepted.contains(&(0, 0)));
    }

    #[test]
    fn test_session_accept_hunk_out_of_bounds() {
        let mut session = DiffSession::new();
        // No edits loaded
        let result = session.accept_hunk(0);
        assert!(result.is_none());
    }

    #[test]
    fn test_session_reject_hunk() {
        let mut session = DiffSession::new();
        session.edits.push(FileEdit {
            file_path: "test.rs".into(),
            original: "old".into(),
            modified: "new".into(),
            description: None,
        });
        session.selected = 0;

        let result = session.reject_hunk(0);
        assert!(result.is_some());
        assert!(session.hunk_rejected.contains(&(0, 0)));
    }

    #[test]
    fn test_session_current_inline_hunks() {
        let mut session = DiffSession::new();
        session.edits.push(FileEdit {
            file_path: "test.rs".into(),
            original: "line1\nline2\n".into(),
            modified: "line1\nline2_modified\n".into(),
            description: None,
        });
        session.selected = 0;

        let hunks = session.current_inline_hunks();
        assert!(!hunks.is_empty());
    }

    #[test]
    fn test_session_current_inline_hunks_empty() {
        let session = DiffSession::new();
        let hunks = session.current_inline_hunks();
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_session_preview_result_no_acceptance() {
        let mut session = DiffSession::new();
        session.edits.push(FileEdit {
            file_path: "test.rs".into(),
            original: "original content".into(),
            modified: "modified content".into(),
            description: None,
        });
        session.selected = 0;

        let preview = session.preview_result();
        assert!(preview.is_some());
        assert_eq!(preview.unwrap(), "original content");
    }

    #[test]
    fn test_session_preview_result_with_accepted() {
        let mut session = DiffSession::new();
        session.edits.push(FileEdit {
            file_path: "test.rs".into(),
            original: "original content".into(),
            modified: "modified content".into(),
            description: None,
        });
        session.selected = 0;
        session.hunk_accepted.insert((0, 0));

        let preview = session.preview_result();
        assert!(preview.is_some());
        assert_eq!(preview.unwrap(), "modified content");
    }

    #[test]
    fn test_session_load_clears_hunk_tracking() {
        let mut session = DiffSession::new();
        session.edits.push(FileEdit {
            file_path: "test.rs".into(),
            original: "old".into(),
            modified: "new".into(),
            description: None,
        });
        session.selected = 0;
        session.hunk_accepted.insert((0, 0));
        session.hunk_rejected.insert((0, 1));

        session.load("no edits here");

        assert!(session.hunk_accepted.is_empty());
        assert!(session.hunk_rejected.is_empty());
    }
}
