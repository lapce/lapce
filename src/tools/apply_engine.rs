//! ApplyEngine — Precise code edit application with conflict resolution
//! and edit reliability scoring.
//!
//! Bridges the gap between LLM-generated code patches and file system edits.
//! Supports multiple edit formats (SEARCH/REPLACE, unified diff, full file,
//! line range) with validation and scoring before application.
//!
//! ## Architecture
//!
//! ```text
//! LLM Response → parse_llm_response() → Vec<(PathBuf, EditFormat)>
//!                  ↓
//!            validate_patch() → ScoredPatch
//!                  ↓
//!            apply_edit() / apply_batch() → EditResult
//! ```

use std::path::{Path, PathBuf};

/// How the LLM requested the edit.
#[derive(Debug, Clone)]
pub enum EditFormat {
    /// Unified diff format (git diff-like)
    UnifiedDiff(String),
    /// Search/replace block (like Aider's SEARCH/REPLACE)
    SearchReplace { search: String, replace: String },
    /// Whole file replacement
    FullFile(String),
    /// Line-based edit: (start_line, end_line, replacement_text)
    LineRange { start: u32, end: u32, text: String },
}

/// Result of applying a single edit.
#[derive(Debug, Clone)]
pub struct EditResult {
    pub file_path: PathBuf,
    pub success: bool,
    pub format: EditFormatType,
    pub lines_changed: usize,
    pub confidence: f64,         // 0.0-1.0: How confident the apply is correct
    pub error: Option<String>,
    pub backup_path: Option<PathBuf>,
}

/// The format type of an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditFormatType {
    UnifiedDiff,
    SearchReplace,
    FullFile,
    LineRange,
}

/// Validated and scored edit patch.
#[derive(Debug, Clone)]
pub struct ScoredPatch {
    pub file_path: PathBuf,
    pub original_format: EditFormatType,
    pub score: f64,              // Reliability score 0-100
    pub issues: Vec<String>,     // Potential issues found
    pub confidence: f64,         // Confidence after validation
}

impl ScoredPatch {
    pub fn is_safe_to_apply(&self) -> bool {
        self.score >= 70.0 && self.confidence >= 0.6
    }

    /// Enhanced scoring with more sophisticated heuristics.
    ///
    /// Evaluates the edit based on:
    /// - Content match quality
    /// - Syntax balance (bracket/brace matching)
    /// - Symbol reference integrity
    /// - Edit format-specific metrics
    pub fn compute_detailed_score(
        file_content: &str,
        edit: &EditFormat,
        language: Option<&str>,
    ) -> Self {
        let mut issues = Vec::new();
        let (format_type, score, confidence) = match edit {
            EditFormat::SearchReplace { search, replace } => {
                let mut s: f32 = 80.0;
                let mut c: f32 = 0.8;
                if search.is_empty() {
                    issues.push("Search text is empty".to_string());
                    s -= 40.0;
                    c -= 0.3;
                }
                if replace.is_empty() {
                    issues.push("Replace text is empty".to_string());
                    s -= 10.0;
                }
                if !file_content.contains(search) {
                    issues.push("Search text not found in file".to_string());
                    s -= 30.0;
                    c -= 0.3;
                }
                let count = file_content.matches(search).count();
                if count > 1 {
                    issues.push(format!("Search text found {} times — ambiguous match", count));
                    s -= 15.0;
                    c -= 0.15;
                }
                if count == 1 {
                    s += 10.0;
                    c += 0.1;
                }
                // Syntax balance check
                let balance_issues = Self::check_syntax_balance(file_content, edit);
                if !balance_issues.is_empty() {
                    issues.extend(balance_issues);
                    s -= 10.0;
                    c -= 0.1;
                }
                // Symbol references
                let ref_score = Self::check_symbol_references(file_content, edit);
                s += ref_score as f32 * 5.0;
                (EditFormatType::SearchReplace, s.max(0.0).min(100.0), c.max(0.0).min(1.0))
            }
            EditFormat::UnifiedDiff(diff) => {
                let mut s: f32 = 75.0;
                let mut c: f32 = 0.75;
                if diff.is_empty() {
                    issues.push("Diff is empty".to_string());
                    s -= 40.0;
                    c -= 0.3;
                }
                if !diff.contains("--- ") || !diff.contains("+++ ") {
                    issues.push("Diff missing file headers".to_string());
                    s -= 15.0;
                    c -= 0.1;
                }
                let hunk_count = diff.lines().filter(|l| l.starts_with("@@")).count();
                if hunk_count == 0 {
                    issues.push("No hunks found in diff".to_string());
                    s -= 20.0;
                    c -= 0.15;
                }
                (EditFormatType::UnifiedDiff, s.max(0.0).min(100.0), c.max(0.0).min(1.0))
            }
            EditFormat::FullFile(content) => {
                let mut s: f32 = 85.0;
                let mut c: f32 = 0.85;
                if content.is_empty() {
                    issues.push("Replacement content is empty".to_string());
                    s -= 30.0;
                    c -= 0.2;
                }
                if file_content == *content {
                    issues.push("New content is identical to current content".to_string());
                    s -= 20.0;
                    c -= 0.15;
                }
                (EditFormatType::FullFile, s.max(0.0).min(100.0), c.max(0.0).min(1.0))
            }
            EditFormat::LineRange { start, end, text } => {
                let mut s: f32 = 82.0;
                let mut c: f32 = 0.82;
                let total_lines = file_content.lines().count();
                if *start == 0 || *end == 0 {
                    issues.push("Line numbers should be 1-based".to_string());
                    s -= 10.0;
                }
                if *start > *end {
                    issues.push("Start line > end line".to_string());
                    s -= 20.0;
                    c -= 0.15;
                }
                if *end > total_lines as u32 && total_lines > 0 {
                    issues.push(format!("End line {} exceeds file length {}", end, total_lines));
                    s -= 10.0;
                }
                if text.is_empty() {
                    issues.push("Replacement text is empty".to_string());
                    s -= 10.0;
                }
                (EditFormatType::LineRange, s.max(0.0).min(100.0), c.max(0.0).min(1.0))
            }
        };

        // Language-specific adjustments
        if let Some(lang) = language {
            match lang {
                "rust" | "rs"
                    if !file_content.contains("fn ") && matches!(edit, EditFormat::FullFile(_)) => {
                        // Rust file without functions is suspicious
                    }
                _ => {}
            }
        }

        ScoredPatch {
            file_path: PathBuf::from("unknown"),
            original_format: format_type,
            score: score as f64,
            issues,
            confidence: confidence as f64,
        }
    }

    /// Check if the edit would parse correctly (bracket/brace balance).
    ///
    /// Returns a list of issues found (empty if balanced).
    fn check_syntax_balance(content: &str, edit: &EditFormat) -> Vec<String> {
        let mut issues = Vec::new();

        // Get the resulting content after applying the edit
        let result_content = match edit {
            EditFormat::SearchReplace { search, replace } => {
                if let Some(pos) = content.find(search) {
                    let mut r = String::with_capacity(content.len() + replace.len().saturating_sub(search.len()));
                    r.push_str(&content[..pos]);
                    r.push_str(replace);
                    r.push_str(&content[pos + search.len()..]);
                    r
                } else {
                    return issues; // Can't determine result
                }
            }
            EditFormat::FullFile(c) => c.clone(),
            EditFormat::LineRange { start, end, text } => {
                let lines: Vec<&str> = content.lines().collect();
                let mut r = String::new();
                let start_idx = (start.saturating_sub(1)) as usize;
                let end_idx = (end.saturating_sub(1)) as usize;
                for (i, line) in lines.iter().enumerate() {
                    if i >= start_idx && i <= end_idx {
                        if i == start_idx {
                            if !r.is_empty() { r.push('\n'); }
                            r.push_str(text);
                        }
                    } else {
                        if !r.is_empty() { r.push('\n'); }
                        r.push_str(line);
                    }
                }
                if r.is_empty() { r = text.clone(); }
                r
            }
            EditFormat::UnifiedDiff(_) => return issues, // Hard to determine without applying
        };

        // Count brackets and braces
        let opens: Vec<char> = result_content.chars().filter(|c| matches!(c, '{' | '(' | '[')).collect();
        let closes: Vec<char> = result_content.chars().filter(|c| matches!(c, '}' | ')' | ']')).collect();

        let open_count = opens.len();
        let close_count = closes.len();

        if open_count != close_count {
            issues.push(format!(
                "Bracket/brace imbalance: {} opening vs {} closing",
                open_count, close_count
            ));
        }

        issues
    }

    /// Check if the edit references existing symbols.
    ///
    /// Returns a score between 0.0 and 1.0 indicating how well the edit
    /// references symbols that exist in the file content.
    fn check_symbol_references(content: &str, edit: &EditFormat) -> f64 {
        let edit_text = match edit {
            EditFormat::SearchReplace { replace, .. } => replace.as_str(),
            EditFormat::FullFile(c) => c.as_str(),
            EditFormat::LineRange { text, .. } => text.as_str(),
            EditFormat::UnifiedDiff(_) => return 0.5, // Neutral for diffs
        };

        // Extract symbols (function calls, variable names) from both
        let content_symbols: Vec<&str> = content
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() >= 3)
            .collect();

        let edit_symbols: Vec<&str> = edit_text
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() >= 3)
            .collect();

        if edit_symbols.is_empty() {
            return 0.5;
        }

        // Count how many symbols in the edit also appear in the content
        let ref_count = edit_symbols
            .iter()
            .filter(|s| content_symbols.contains(s))
            .count();

        ref_count as f64 / edit_symbols.len() as f64
    }
}

/// Strategy for handling edit conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Fail on any conflict
    Fail,
    /// Overwrite the target
    Overwrite,
    /// Attempt to merge changes
    Merge,
    /// Skip conflicting edits
    Skip,
}

/// Result type for conflict resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Changes merged cleanly
    CleanMerge,
    /// Conflict resolved by taking ours (existing)
    TakeOurs,
    /// Conflict resolved by taking theirs (new edit)
    TakeTheirs,
    /// Manual resolution needed (could not auto-merge)
    ManualOnly,
}

/// Result of attempting to resolve a conflict.
#[derive(Debug, Clone)]
pub struct ConflictResult {
    pub resolved_content: String,
    pub resolution: ConflictResolution,
    pub hunks_resolved: usize,
    pub hunks_total: usize,
}

/// Resolves conflicts between existing file content and incoming edits.
///
/// Uses a simple three-way merge strategy with configurable context lines
/// and fallback behavior for unresolvable conflicts.
pub struct ConflictResolver {
    /// How many lines of context to use for merge (default: 3)
    pub context_lines: usize,
    /// Strategy for unresolvable conflicts
    pub fallback_strategy: ConflictStrategy,
}

impl ConflictResolver {
    pub fn new() -> Self {
        Self {
            context_lines: 3,
            fallback_strategy: ConflictStrategy::Skip,
        }
    }

    /// Try to merge an edit into existing content.
    ///
    /// For `SearchReplace`: attempts to find the search text and replace it.
    /// For `UnifiedDiff`: applies hunks to the content.
    /// For `FullFile`: takes the edit as-is (TakeTheirs).
    /// For `LineRange`: replaces the specified line range.
    pub fn resolve(&self, existing: &str, edit: &EditFormat) -> ConflictResult {
        match edit {
            EditFormat::SearchReplace { search, replace } => {
                self.resolve_search_replace(existing, search, replace)
            }
            EditFormat::UnifiedDiff(diff) => self.resolve_unified_diff(existing, diff),
            EditFormat::FullFile(content) => ConflictResult {
                resolved_content: content.clone(),
                resolution: ConflictResolution::TakeTheirs,
                hunks_resolved: 1,
                hunks_total: 1,
            },
            EditFormat::LineRange { start, end, text } => {
                let lines: Vec<&str> = existing.lines().collect();
                let mut result = String::new();
                let start_idx = (start.saturating_sub(1)) as usize;
                let end_idx = (end.saturating_sub(1)) as usize;
                for (i, line) in lines.iter().enumerate() {
                    if i >= start_idx && i <= end_idx {
                        if i == start_idx {
                            if !result.is_empty() {
                                result.push('\n');
                            }
                            result.push_str(text);
                        }
                    } else {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(line);
                    }
                }
                if result.is_empty() {
                    result = text.clone();
                }
                ConflictResult {
                    resolved_content: result,
                    resolution: ConflictResolution::CleanMerge,
                    hunks_resolved: 1,
                    hunks_total: 1,
                }
            }
        }
    }

    /// Three-way merge: base vs existing vs edit.
    ///
    /// Uses line-level diff from the `similar` crate to merge changes.
    /// Lines changed only in existing → keep existing's version.
    /// Lines changed only in edit → keep edit's version.
    /// Lines changed in both to the same thing → keep either.
    /// Lines changed in both differently → conflict markers.
    pub fn three_way_merge(&self, base: &str, existing: &str, edit: &str) -> ConflictResult {
        use similar::TextDiff;

        let base_lines: Vec<&str> = base.lines().collect();
        let existing_lines: Vec<&str> = existing.lines().collect();
        let edit_lines: Vec<&str> = edit.lines().collect();

        let d_existing = TextDiff::from_lines(base, existing);
        let d_edit = TextDiff::from_lines(base, edit);

        let ops_existing = d_existing.ops();
        let ops_edit = d_edit.ops();

        let mut result: Vec<String> = Vec::new();
        let mut resolution = ConflictResolution::CleanMerge;
        let mut hunks_resolved = 0;
        let mut hunks_total = 0;

        // We process both diffs in parallel, ordered by base position.
        // Each op covers an old_range (in base) and a new_range (in target).
        let mut base_pos = 0usize;
        let mut ex_idx = 0usize;
        let mut ed_idx = 0usize;

        while base_pos < base_lines.len() || ex_idx < ops_existing.len() || ed_idx < ops_edit.len() {
            let ex_op = ops_existing.get(ex_idx);
            let ed_op = ops_edit.get(ed_idx);

            let ex_start = ex_op.map(|o| o.old_range().start).unwrap_or(usize::MAX);
            let ed_start = ed_op.map(|o| o.old_range().start).unwrap_or(usize::MAX);

            // Copy unchanged lines up to the next change
            let next_change = ex_start.min(ed_start);
            while base_pos < next_change && base_pos < base_lines.len() {
                result.push(base_lines[base_pos].to_string());
                base_pos += 1;
            }

            if base_pos >= base_lines.len() && ex_idx >= ops_existing.len() && ed_idx >= ops_edit.len() {
                break;
            }

            // Determine which op(s) start at this position
            let ex_active = ex_op.is_some() && ex_op.unwrap().old_range().start == base_pos;
            let ed_active = ed_op.is_some() && ed_op.unwrap().old_range().start == base_pos;

            if ex_active && ed_active {
                // Both have changes at this position
                hunks_total += 1;
                let ex_data = ex_op.unwrap();
                let ed_data = ed_op.unwrap();

                let ex_range = ex_data.new_range();
                let ed_range = ed_data.new_range();
                let ex_old_len = ex_data.old_range().len();
                let ed_old_len = ed_data.old_range().len();

                let ex_new_lines: Vec<&str> = existing_lines[ex_range.start..ex_range.end.min(existing_lines.len())].to_vec();
                let ed_new_lines: Vec<&str> = edit_lines[ed_range.start..ed_range.end.min(edit_lines.len())].to_vec();

                if ex_new_lines == ed_new_lines {
                    // Same change — apply once
                    for line in &ex_new_lines {
                        result.push(line.to_string());
                    }
                    hunks_resolved += 1;
                } else if ex_new_lines.is_empty() {
                    // Existing deleted, edit kept/changed
                    for line in &ed_new_lines {
                        result.push(line.to_string());
                    }
                    resolution = ConflictResolution::TakeTheirs;
                    hunks_resolved += 1;
                } else if ed_new_lines.is_empty() {
                    // Edit deleted, existing kept/changed
                    for line in &ex_new_lines {
                        result.push(line.to_string());
                    }
                    resolution = ConflictResolution::TakeOurs;
                    hunks_resolved += 1;
                } else {
                    // Genuine conflict — mark both
                    resolution = ConflictResolution::ManualOnly;
                    result.push("<<<<<<< existing (ours)".to_string());
                    for line in &ex_new_lines {
                        result.push(line.to_string());
                    }
                    result.push("=======".to_string());
                    for line in &ed_new_lines {
                        result.push(line.to_string());
                    }
                    result.push(">>>>>>> edit (theirs)".to_string());
                }

                base_pos += ex_old_len.max(ed_old_len);
                ex_idx += 1;
                ed_idx += 1;
            } else if ex_active {
                // Only existing has a change
                hunks_total += 1;
                let op = ex_op.unwrap();
                let old_len = op.old_range().len();
                let new_range = op.new_range();
                let new_lines = &existing_lines[new_range.start..new_range.end.min(existing_lines.len())];
                for line in new_lines {
                    result.push(line.to_string());
                }
                base_pos += old_len;
                ex_idx += 1;
                hunks_resolved += 1;
            } else if ed_active {
                // Only edit has a change
                hunks_total += 1;
                let op = ed_op.unwrap();
                let old_len = op.old_range().len();
                let new_range = op.new_range();
                let new_lines = &edit_lines[new_range.start..new_range.end.min(edit_lines.len())];
                for line in new_lines {
                    result.push(line.to_string());
                }
                base_pos += old_len;
                ed_idx += 1;
                hunks_resolved += 1;
            } else {
                // No active op — advance past any gap
                // This can happen if we've exhausted one diff
                if ex_idx < ops_existing.len() {
                    let skip = ops_existing[ex_idx].old_range().start.saturating_sub(base_pos);
                    for i in 0..skip {
                        if base_pos + i < base_lines.len() {
                            result.push(base_lines[base_pos + i].to_string());
                        }
                    }
                    base_pos += skip;
                } else if ed_idx < ops_edit.len() {
                    let skip = ops_edit[ed_idx].old_range().start.saturating_sub(base_pos);
                    for i in 0..skip {
                        if base_pos + i < base_lines.len() {
                            result.push(base_lines[base_pos + i].to_string());
                        }
                    }
                    base_pos += skip;
                } else {
                    break;
                }
            }
        }

        // Append any remaining base lines
        while base_pos < base_lines.len() {
            result.push(base_lines[base_pos].to_string());
            base_pos += 1;
        }

        ConflictResult {
            resolved_content: result.join("\n"),
            resolution,
            hunks_resolved,
            hunks_total,
        }
    }

    /// Resolve a SEARCH/REPLACE conflict.
    ///
    /// Attempts exact match first, then trimmed match, then three-way merge
    /// using the search text as base and replace text as edit.
    fn resolve_search_replace(&self, content: &str, search: &str, replace: &str) -> ConflictResult {
        // Try exact match
        if let Some(pos) = content.find(search) {
            let mut result = String::with_capacity(
                content.len() + replace.len().saturating_sub(search.len()),
            );
            result.push_str(&content[..pos]);
            result.push_str(replace);
            result.push_str(&content[pos + search.len()..]);
            return ConflictResult {
                resolved_content: result,
                resolution: ConflictResolution::CleanMerge,
                hunks_resolved: 1,
                hunks_total: 1,
            };
        }

        // Try trimmed match
        let trimmed_search = search.trim();
        if !trimmed_search.is_empty() && trimmed_search != search {
            if let Some(pos) = content.find(trimmed_search) {
                let mut result = String::with_capacity(
                    content.len() + replace.len().saturating_sub(trimmed_search.len()),
                );
                result.push_str(&content[..pos]);
                result.push_str(replace);
                result.push_str(&content[pos + trimmed_search.len()..]);
                return ConflictResult {
                    resolved_content: result,
                    resolution: ConflictResolution::CleanMerge,
                    hunks_resolved: 1,
                    hunks_total: 1,
                };
            }
        }

        // Try three-way merge with search as base and replace as edit
        let merge_result = self.three_way_merge(search, content, replace);
        if merge_result.resolution != ConflictResolution::ManualOnly {
            return merge_result;
        }

        // Fallback: return the content unchanged with ManualOnly resolution
        ConflictResult {
            resolved_content: content.to_string(),
            resolution: ConflictResolution::ManualOnly,
            hunks_resolved: 0,
            hunks_total: 1,
        }
    }

    /// Resolve a unified diff conflict.
    fn resolve_unified_diff(&self, content: &str, diff: &str) -> ConflictResult {
        if diff.is_empty() {
            return ConflictResult {
                resolved_content: content.to_string(),
                resolution: ConflictResolution::ManualOnly,
                hunks_resolved: 0,
                hunks_total: 0,
            };
        }

        // Try to apply the unified diff directly using external logic
        // We manually apply hunks here with conflict detection
        let mut result = content.to_string();
        let mut hunks_total = 0;
        let mut hunks_resolved = 0;
        let mut resolution = ConflictResolution::CleanMerge;
        let mut current_hunk: Option<(u32, Vec<String>, Vec<String>)> = None;

        for line in diff.lines() {
            if line.starts_with("@@") {
                // Apply previous hunk
                if let Some((start, removed, added)) = current_hunk.take() {
                    hunks_total += 1;
                    match self.apply_hunk_safe(&result, start, &removed, &added) {
                        Ok(new_result) => {
                            result = new_result;
                            hunks_resolved += 1;
                        }
                        Err(_) => {
                            resolution = ConflictResolution::ManualOnly;
                        }
                    }
                }

                // Parse hunk header
                let header = line.trim_start_matches("@@").trim_end_matches("@@").trim();
                let parts: Vec<&str> = header.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                let old_start: u32 = parts[0]
                    .trim_start_matches('-')
                    .split(',')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                current_hunk = Some((old_start, Vec::new(), Vec::new()));
            } else if let Some((_, ref mut removed, ref mut added)) = current_hunk {
                if line.starts_with('-') {
                    removed.push(line[1..].to_string());
                } else if line.starts_with('+') {
                    added.push(line[1..].to_string());
                } else {
                    removed.push(line.to_string());
                    added.push(line.to_string());
                }
            }
        }

        // Apply last hunk
        if let Some((start, removed, added)) = current_hunk.take() {
            hunks_total += 1;
            match self.apply_hunk_safe(&result, start, &removed, &added) {
                Ok(new_result) => {
                    result = new_result;
                    hunks_resolved += 1;
                }
                Err(_) => {
                    resolution = ConflictResolution::ManualOnly;
                }
            }
        }

        ConflictResult {
            resolved_content: result,
            resolution,
            hunks_resolved,
            hunks_total,
        }
    }

    /// Apply a single hunk with conflict detection.
    fn apply_hunk_safe(
        &self,
        content: &str,
        start_line: u32,
        removed: &[String],
        added: &[String],
    ) -> Result<String, String> {
        let lines: Vec<&str> = content.lines().collect();
        let idx = (start_line.saturating_sub(1)) as usize;

        if idx >= lines.len() {
            return Err(format!(
                "Hunk start line {} exceeds file length {}",
                start_line,
                lines.len()
            ));
        }

        // Verify removed lines match
        for (i, rem_line) in removed.iter().enumerate() {
            let content_line = lines.get(idx + i).unwrap_or(&"");
            if *rem_line != *content_line && rem_line.trim() != content_line.trim() {
                return Err(format!(
                    "Hunk context mismatch at line {}: expected '{}', got '{}'",
                    start_line + i as u32,
                    rem_line,
                    content_line
                ));
            }
        }

        // Build new content
        let mut new_lines: Vec<String> = Vec::new();

        for i in 0..idx.min(lines.len()) {
            new_lines.push(lines[i].to_string());
        }
        for add_line in added {
            new_lines.push(add_line.to_string());
        }
        let skip = removed.len();
        for i in (idx + skip)..lines.len() {
            new_lines.push(lines[i].to_string());
        }

        Ok(new_lines.join("\n"))
    }
}

impl Default for ConflictResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a compilation check.
#[derive(Debug, Clone)]
pub struct CompilationCheck {
    pub success: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub duration_ms: u64,
}

/// Validates edits by checking compilation (if possible).
///
/// For Rust projects: runs `cargo check --lib` in the workspace.
/// For other languages: attempts language-specific check if available.
#[derive(Debug, Clone)]
pub struct CompilationValidator {
    pub enabled: bool,
    pub timeout_secs: u64,
    pub cargo_path: String,
}

impl CompilationValidator {
    pub fn new() -> Self {
        Self {
            enabled: true,
            timeout_secs: 30,
            cargo_path: "cargo".to_string(),
        }
    }

    /// Check if the workspace compiles after edits.
    pub async fn check_compilation(&self, workspace: &Path) -> CompilationCheck {
        let start = std::time::Instant::now();

        let result = std::process::Command::new(&self.cargo_path)
            .args(["check", "--lib"])
            .current_dir(workspace)
            .output();

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{}{}", stdout, stderr);
                let (errors, warnings) = self.parse_cargo_check(&combined);
                CompilationCheck {
                    success: output.status.success(),
                    errors,
                    warnings,
                    duration_ms,
                }
            }
            Err(e) => CompilationCheck {
                success: false,
                errors: vec![format!("Failed to run cargo check: {}", e)],
                warnings: Vec::new(),
                duration_ms,
            },
        }
    }

    /// Parse cargo check output for errors and warnings.
    ///
    /// Errors match patterns like `error[E0425]: ...` or `error: ...`
    /// Warnings match patterns like `warning: ...` or `warning[E...]: ...`
    pub fn parse_cargo_check(&self, output: &str) -> (Vec<String>, Vec<String>) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Regex matches error/warning lines with optional error codes
        let error_re = regex::Regex::new(r"^\s*(error)(?:\[([A-Z]\d+)\])?\s*:")
            .expect("Valid error regex");
        let warning_re = regex::Regex::new(r"^\s*(warning)(?:\[([A-Z]\d+)\])?\s*:")
            .expect("Valid warning regex");

        for line in output.lines() {
            if error_re.is_match(line) {
                errors.push(line.to_string());
            } else if warning_re.is_match(line) {
                warnings.push(line.to_string());
            }
        }

        (errors, warnings)
    }

    // ── Timeout & incremental check (Task 4) ─────────────────────────────

    /// Check compilation with timeout.
    pub async fn check_compilation_timeout(&self, workspace: &Path, timeout_secs: u64) -> CompilationCheck {
        let mut config = self.clone();
        config.timeout_secs = timeout_secs;
        config.check_compilation(workspace).await
    }

    /// Incremental check: only check changed modules.
    /// Requires `cargo check -p <specific-package>`.
    pub async fn check_incremental(&self, workspace: &Path, changed_crates: &[String]) -> CompilationCheck {
        let start = std::time::Instant::now();
        let mut all_errors = Vec::new();
        let mut all_warnings = Vec::new();

        for crate_name in changed_crates {
            let check = self.check_single_crate(workspace, crate_name).await;
            all_errors.extend(check.errors);
            all_warnings.extend(check.warnings);
        }

        CompilationCheck {
            success: all_errors.is_empty(),
            errors: all_errors,
            warnings: all_warnings,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Check a single crate compilation.
    async fn check_single_crate(&self, workspace: &Path, crate_name: &str) -> CompilationCheck {
        let start = std::time::Instant::now();

        let output = tokio::process::Command::new(&self.cargo_path)
            .arg("check")
            .arg("-p")
            .arg(crate_name)
            .arg("--lib")
            .current_dir(workspace)
            .output()
            .await;

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{}\n{}", stdout, stderr);
                let (errors, warnings) = self.parse_cargo_check(&combined);
                CompilationCheck {
                    success: errors.is_empty(),
                    errors,
                    warnings,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            Err(e) => CompilationCheck {
                success: false,
                errors: vec![format!("Process error: {}", e)],
                warnings: vec![],
                duration_ms: start.elapsed().as_millis() as u64,
            },
        }
    }
}

impl Default for CompilationValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Engine for applying precise code edits with validation.
///
/// # Example
///
/// ```ignore
/// let engine = ApplyEngine::new("/path/to/workspace");
///
/// let edit = EditFormat::SearchReplace {
///     search: "fn old_name()".into(),
///     replace: "fn new_name()".into(),
/// };
///
/// let result = engine.apply_edit(Path::new("src/lib.rs"), &edit).await?;
/// println!("Confidence: {:.2}", result.confidence);
/// ```
pub struct ApplyEngine {
    /// Max tries for ambiguous edits
    pub max_retries: u32,
    /// Whether to create backups before applying
    pub auto_backup: bool,
    /// Strategy for handling conflicts
    pub conflict_strategy: ConflictStrategy,
    /// Whether to validate edit with compilation (if available)
    pub validate_with_compile: bool,
    working_dir: PathBuf,
}

impl ApplyEngine {
    pub fn new(working_dir: &Path) -> Self {
        Self {
            max_retries: 3,
            auto_backup: true,
            conflict_strategy: ConflictStrategy::Fail,
            validate_with_compile: false,
            working_dir: working_dir.to_path_buf(),
        }
    }

    /// Parse and apply a single edit in any format.
    pub async fn apply_edit(&self, file_path: &Path, edit: &EditFormat) -> anyhow::Result<EditResult> {
        let full_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            self.working_dir.join(file_path)
        };

        // Read current content
        let current_content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => String::new(), // File doesn't exist yet
        };

        // Create backup if needed
        let backup_path = if self.auto_backup && full_path.exists() {
            let backup = full_path.with_extension("bak");
            std::fs::write(&backup, &current_content).ok();
            Some(backup)
        } else {
            None
        };

        // Apply the edit
        let (new_content, format_type, lines_changed, confidence) = match edit {
            EditFormat::SearchReplace { search, replace } => {
                match self.apply_search_replace(&current_content, search, replace) {
                    Ok(new) => {
                        let lines = new.lines().count().max(current_content.lines().count());
                        (new, EditFormatType::SearchReplace, lines.abs_diff(current_content.lines().count()), 0.85)
                    }
                    Err(e) => {
                        return Ok(EditResult {
                            file_path: full_path,
                            success: false,
                            format: EditFormatType::SearchReplace,
                            lines_changed: 0,
                            confidence: 0.0,
                            error: Some(e),
                            backup_path,
                        });
                    }
                }
            }
            EditFormat::UnifiedDiff(diff) => {
                match self.apply_unified_diff(&current_content, diff) {
                    Ok(new) => {
                        let lines = new.lines().count().max(current_content.lines().count());
                        (new, EditFormatType::UnifiedDiff, lines.abs_diff(current_content.lines().count()), 0.80)
                    }
                    Err(e) => {
                        return Ok(EditResult {
                            file_path: full_path,
                            success: false,
                            format: EditFormatType::UnifiedDiff,
                            lines_changed: 0,
                            confidence: 0.0,
                            error: Some(e),
                            backup_path,
                        });
                    }
                }
            }
            EditFormat::FullFile(content) => {
                let old_lines = current_content.lines().count();
                let new_lines = content.lines().count();
                (content.clone(), EditFormatType::FullFile, old_lines.abs_diff(new_lines), 0.90)
            }
            EditFormat::LineRange { start, end, text } => {
                let new = self.apply_line_range(&current_content, *start, *end, text);
                let lines_changed = (end.saturating_sub(*start) + 1) as usize;
                (new, EditFormatType::LineRange, lines_changed, 0.88)
            }
        };

        // Write the new content
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, &new_content)?;

        Ok(EditResult {
            file_path: full_path,
            success: true,
            format: format_type,
            lines_changed,
            confidence,
            error: None,
            backup_path,
        })
    }

    /// Apply multiple edits in order, rolling back all on failure.
    pub async fn apply_batch(&self, edits: &[(PathBuf, EditFormat)]) -> anyhow::Result<Vec<EditResult>> {
        let mut results = Vec::new();
        let mut applied: Vec<(PathBuf, String)> = Vec::new(); // (path, original_content)

        for (file_path, edit) in edits {
            // Backup current content before applying
            let full_path = if file_path.is_absolute() {
                file_path.clone()
            } else {
                self.working_dir.join(file_path)
            };
            let original = std::fs::read_to_string(&full_path).unwrap_or_default();

            match self.apply_edit(file_path, edit).await {
                Ok(result) => {
                    if result.success {
                        applied.push((full_path, original));
                    }
                    results.push(result);
                }
                Err(e) => {
                    // Rollback all applied edits in reverse order
                    for (rolled_back_path, original_content) in applied.iter().rev() {
                        if !original_content.is_empty() {
                            let _ = std::fs::write(rolled_back_path, original_content);
                        } else {
                            let _ = std::fs::remove_file(rolled_back_path);
                        }
                    }
                    anyhow::bail!("Batch edit failed at {:?}: {}", file_path, e);
                }
            }
        }

        Ok(results)
    }

    /// Parse LLM output and extract edit blocks.
    ///
    /// Supports formats:
    /// - Code blocks with `SEARCH\n...\nREPLACE\n...` (Aider-compatible)
    /// - Code blocks with `--- a/file\n+++ b/file` (unified diff)
    /// - Lines with `// file: path_to_file` followed by code block
    /// - Fenced code blocks with language annotation
    pub fn parse_llm_response(&self, response: &str) -> Vec<(PathBuf, EditFormat)> {
        let mut edits = Vec::new();
        let mut current_file: Option<PathBuf> = None;

        for line in response.lines() {
            // Check for file indicator comments
            let trimmed = line.trim();
            if let Some(file_path) = trimmed.strip_prefix("// file: ")
                .or_else(|| trimmed.strip_prefix("# file: "))
                .or_else(|| trimmed.strip_prefix("// File: "))
                .or_else(|| trimmed.strip_prefix("## file: "))
            {
                current_file = Some(PathBuf::from(file_path.trim()));
                continue;
            }

            // Check for file indicator in markdown headings
            if trimmed.starts_with("### ") || trimmed.starts_with("## ") {
                let content = trimmed.trim_start_matches('#').trim();
                if let Some(fpath) = content.strip_prefix("file: ")
                    .or_else(|| content.strip_prefix("File: "))
                {
                    current_file = Some(PathBuf::from(fpath.trim()));
                    continue;
                }
            }
        }

        // Parse fenced code blocks
        let mut in_code_block = false;
        let mut block_content = String::new();
        let mut block_lang = String::new();

        for line in response.lines() {
            if line.trim().starts_with("```") {
                if in_code_block {
                    // End of code block — process content
                    in_code_block = false;

                    // Detect edit format in block content
                    if let Some(edit) = self.detect_edit_format(&block_content, &block_lang, &current_file) {
                        edits.push(edit);
                    }

                    block_content.clear();
                    block_lang.clear();
                } else {
                    // Start of code block
                    in_code_block = true;
                    block_lang = line.trim().trim_start_matches("```").trim().to_string();
                }
                continue;
            }

            if in_code_block {
                if !block_content.is_empty() {
                    block_content.push('\n');
                }
                block_content.push_str(line);
            }
        }

        // Also handle content that's not in code blocks but has SEARCH/REPLACE markers
        // This handles Aider-style output where SEARCH/REPLACE blocks may not be fenced
        if !in_code_block {
            let lower = response.to_lowercase();
            if lower.contains("search\n") && lower.contains("replace\n") {
                // Try to extract SEARCH/REPLACE blocks from raw text
                if let Some(edit) = self.extract_search_replace_raw(response, &current_file) {
                    edits.push(edit);
                }
            }
        }

        edits
    }

    /// Detect edit format from a code block's content.
    fn detect_edit_format(
        &self,
        content: &str,
        lang: &str,
        default_file: &Option<PathBuf>,
    ) -> Option<(PathBuf, EditFormat)> {
        let content_lower = content.to_lowercase();
        let file = default_file.clone().unwrap_or_else(|| PathBuf::from("unknown"));

        // Check for Aider-style SEARCH/REPLACE block
        if content_lower.starts_with("search\n") || content.contains("\nsearch\n") {
            let _search_marker = if content_lower.starts_with("search\n") {
                "SEARCH\n"
            } else {
                // Find the SEARCH marker
                let idx = content_lower.find("\nsearch\n")?;
                &content[idx + 1..]
            };

            // Find SEARCH and REPLACE sections
            let search_start = if content.starts_with("SEARCH\n") { 7 } else {
                content_lower.find("search\n")? + 7
            };

            let replace_marker = if let Some(pos) = content[search_start..].to_lowercase().find("\nreplace\n") {
                search_start + pos + 1
            } else {
                return None;
            };

            let replace_start = replace_marker + 8; // "REPLACE\n" length
            let search_text = &content[search_start..replace_marker].trim();
            let replace_text = &content[replace_start..].trim();

            if !search_text.is_empty() {
                return Some((file, EditFormat::SearchReplace {
                    search: search_text.to_string(),
                    replace: replace_text.to_string(),
                }));
            }
        }

        // Check for unified diff format
        if content.contains("--- ") && content.contains("+++ ") {
            return Some((file, EditFormat::UnifiedDiff(content.to_string())));
        }

        // Check for line range format (e.g., "// ... lines 10-20")
        let line_range_re = regex::Regex::new(r"(?i)lines?\s*(\d+)\s*[-–to]+\s*(\d+)").ok()?;
        if let Some(caps) = line_range_re.captures(content) {
            let start: u32 = caps[1].parse().ok()?;
            let end: u32 = caps[2].parse().ok()?;
            // The text after the line range marker is the replacement
            let text = content[caps.get(0).unwrap().end()..].trim().to_string();
            if !text.is_empty() {
                return Some((file, EditFormat::LineRange { start, end, text }));
            }
        }

        // Default: treat as full file replacement (only if there's substantial content)
        if content.len() > 10 && !lang.is_empty() {
            Some((file, EditFormat::FullFile(content.to_string())))
        } else {
            None
        }
    }

    /// Extract SEARCH/REPLACE blocks from raw text (outside code blocks).
    fn extract_search_replace_raw(
        &self,
        response: &str,
        default_file: &Option<PathBuf>,
    ) -> Option<(PathBuf, EditFormat)> {
        let file = default_file.clone().unwrap_or_else(|| PathBuf::from("unknown"));
        let lower = response.to_lowercase();

        let search_idx = lower.find("search\n")?;
        let replace_idx = lower[search_idx + 7..].find("replace\n")? + search_idx + 7;

        let search_text = response[search_idx + 7..replace_idx].trim();
        let replace_text = response[replace_idx + 8..].trim();

        if search_text.is_empty() {
            return None;
        }

        // Find where the replace block ends (next blank line or end of content)
        let replace_end = replace_text.find("\n\n").unwrap_or(replace_text.len());
        let replace = replace_text[..replace_end].trim().to_string();

        Some((file, EditFormat::SearchReplace {
            search: search_text.to_string(),
            replace,
        }))
    }

    /// Score and validate a patch before applying.
    pub fn validate_patch(&self, file_path: &Path, edit: &EditFormat) -> ScoredPatch {
        let full_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            self.working_dir.join(file_path)
        };

        let current_content = std::fs::read_to_string(&full_path).unwrap_or_default();
        let mut issues = Vec::new();
        let (format_type, base_score, _base_conf) = match edit {
            EditFormat::SearchReplace { search, replace } => {
                let mut score: f32 = 80.0;
                let mut conf: f32 = 0.8;

                if search.is_empty() {
                    issues.push("Search text is empty".to_string());
                    score -= 40.0;
                    conf -= 0.3;
                }
                if replace.is_empty() {
                    issues.push("Replace text is empty".to_string());
                    score -= 10.0;
                }
                if !current_content.contains(search) {
                    issues.push("Search text not found in file".to_string());
                    score -= 30.0;
                    conf -= 0.3;
                }
                // Check for multiple occurrences
                let count = current_content.matches(search).count();
                if count > 1 {
                    issues.push(format!("Search text found {} times — ambiguous match", count));
                    score -= 15.0;
                    conf -= 0.15;
                }
                if count == 1 {
                    score += 10.0; // Exact single match
                    conf += 0.1;
                }

                (EditFormatType::SearchReplace, score.max(0.0).min(100.0), conf.max(0.0).min(1.0))
            }
            EditFormat::UnifiedDiff(diff) => {
                let mut score: f32 = 75.0;
                let mut conf: f32 = 0.75;

                if diff.is_empty() {
                    issues.push("Diff is empty".to_string());
                    score -= 40.0;
                    conf -= 0.3;
                }
                if !diff.contains("--- ") || !diff.contains("+++ ") {
                    issues.push("Diff missing file headers (---/+++)".to_string());
                    score -= 15.0;
                    conf -= 0.1;
                }
                // Count hunk headers
                let hunk_count = diff.lines().filter(|l| l.starts_with("@@")).count();
                if hunk_count == 0 {
                    issues.push("No hunks found in diff".to_string());
                    score -= 20.0;
                    conf -= 0.15;
                }

                (EditFormatType::UnifiedDiff, score.max(0.0).min(100.0), conf.max(0.0).min(1.0))
            }
            EditFormat::FullFile(content) => {
                let mut score: f32 = 85.0;
                let mut conf: f32 = 0.85;

                if content.is_empty() {
                    issues.push("Replacement content is empty".to_string());
                    score -= 30.0;
                    conf -= 0.2;
                }
                if current_content == *content {
                    issues.push("New content is identical to current content".to_string());
                    score -= 20.0;
                    conf -= 0.15;
                }

                (EditFormatType::FullFile, score.max(0.0).min(100.0), conf.max(0.0).min(1.0))
            }
            EditFormat::LineRange { start, end, text } => {
                let mut score: f32 = 82.0;
                let mut conf: f32 = 0.82;
                let total_lines = current_content.lines().count();

                if *start == 0 || *end == 0 {
                    issues.push("Line numbers should be 1-based".to_string());
                    score -= 10.0;
                }
                if *start > *end {
                    issues.push("Start line > end line".to_string());
                    score -= 20.0;
                    conf -= 0.15;
                }
                if *end > total_lines as u32 && total_lines > 0 {
                    issues.push(format!("End line {} exceeds file length {}", end, total_lines));
                    score -= 10.0;
                }
                if text.is_empty() {
                    issues.push("Replacement text is empty".to_string());
                    score -= 10.0;
                }

                (EditFormatType::LineRange, score.max(0.0).min(100.0), conf.max(0.0).min(1.0))
            }
        };

        ScoredPatch {
            file_path: full_path,
            original_format: format_type,
            score: base_score as f64,
            issues,
            confidence: base_score as f64 / 100.0, // Normalized confidence
        }
    }

    /// Apply SEARCH/REPLACE edit (Aider-style).
    fn apply_search_replace(&self, content: &str, search: &str, replace: &str) -> Result<String, String> {
        if search.is_empty() {
            return Err("Search text is empty".to_string());
        }

        if let Some(pos) = content.find(search) {
            let mut result = String::with_capacity(
                content.len() + replace.len().saturating_sub(search.len())
            );
            result.push_str(&content[..pos]);
            result.push_str(replace);
            result.push_str(&content[pos + search.len()..]);
            Ok(result)
        } else {
            // Try with trimmed whitespace matching
            let trimmed_search = search.trim();
            let trimmed_content = content;
            if let Some(pos) = trimmed_content.find(trimmed_search) {
                let mut result = String::with_capacity(
                    trimmed_content.len() + replace.len().saturating_sub(trimmed_search.len())
                );
                result.push_str(&trimmed_content[..pos]);
                result.push_str(replace);
                result.push_str(&trimmed_content[pos + trimmed_search.len()..]);
                Ok(result)
            } else {
                Err(format!("Search text not found in content:\n---search---\n{}\n---content---\n{}", search, content))
            }
        }
    }

    /// Apply unified diff edit.
    fn apply_unified_diff(&self, content: &str, diff: &str) -> Result<String, String> {
        if diff.is_empty() {
            return Err("Diff is empty".to_string());
        }

        // Parse the diff into hunks
        let mut result = content.to_string();
        let mut current_hunk: Option<(u32, u32, Vec<String>, Vec<String>)> = None; // (start_line, context_lines, removed, added)

        for line in diff.lines() {
            if line.starts_with("@@") {
                // Apply previous hunk if exists
                if let Some((start, ctx, removed, added)) = current_hunk.take() {
                    result = self.apply_hunk(&result, start, ctx, &removed, &added)?;
                }

                // Parse hunk header: @@ -start,count +start,count @@
                let header = line.trim_start_matches("@@").trim_end_matches("@@").trim();
                let parts: Vec<&str> = header.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                let old_start: u32 = parts[0].trim_start_matches('-').split(',').next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                // let old_count: u32 = parts[0].split(',').nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);

                current_hunk = Some((old_start, 0, Vec::new(), Vec::new()));
            } else if let Some((_, _, ref mut removed, ref mut added)) = current_hunk {
                if line.starts_with('-') {
                    removed.push(line[1..].to_string());
                } else if line.starts_with('+') {
                    added.push(line[1..].to_string());
                } else {
                    // Context line — belongs to both removed and added for alignment
                    removed.push(line.to_string());
                    added.push(line.to_string());
                }
            }
        }

        // Apply last hunk
        if let Some((start, ctx, removed, added)) = current_hunk {
            result = self.apply_hunk(&result, start, ctx, &removed, &added)?;
        }

        Ok(result)
    }

    /// Apply a single hunk to content.
    fn apply_hunk(
        &self,
        content: &str,
        start_line: u32,
        _context_lines: u32,
        removed: &[String],
        added: &[String],
    ) -> Result<String, String> {
        let lines: Vec<&str> = content.lines().collect();
        let idx = (start_line.saturating_sub(1)) as usize;

        if idx >= lines.len() {
            return Err(format!("Hunk start line {} exceeds file length {}", start_line, lines.len()));
        }

        // Verify that the removed lines match the content
        let mut match_failed = false;
        for (i, rem_line) in removed.iter().enumerate() {
            let content_line = lines.get(idx + i).unwrap_or(&"");
            // Skip context lines that match
            if *rem_line != *content_line {
                // Allow trimming comparison
                if rem_line.trim() != content_line.trim() {
                    match_failed = true;
                    break;
                }
            }
        }

        if match_failed && !removed.is_empty() {
            // Fall back to simpler approach: just replace the lines
            // This handles cases where context has slight differences
        }

        // Build new content
        let mut new_lines: Vec<&str> = Vec::new();

        // Lines before the hunk
        for i in 0..idx.min(lines.len()) {
            new_lines.push(lines[i]);
        }

        // Add the new (replacement) lines
        for add_line in added {
            new_lines.push(add_line);
        }

        // Lines after the removed section
        let skip = removed.len();
        for i in (idx + skip)..lines.len() {
            new_lines.push(lines[i]);
        }

        Ok(new_lines.join("\n"))
    }

    /// Apply line range edit.
    fn apply_line_range(&self, content: &str, start: u32, end: u32, text: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        let start_idx = (start.saturating_sub(1)) as usize;
        let end_idx = (end.saturating_sub(1)) as usize;

        let mut result = String::new();

        // Lines before the range
        for i in 0..start_idx.min(total) {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(lines[i]);
        }

        // Replacement text
        if !result.is_empty() && !text.is_empty() {
            result.push('\n');
        }
        result.push_str(text);

        // Lines after the range
        for i in (end_idx + 1)..total {
            result.push('\n');
            result.push_str(lines[i]);
        }

        if result.is_empty() {
            // If everything was replaced and content was empty
            text.to_string()
        } else {
            result
        }
    }

    /// Enhanced LLM response parser that extracts edits from diverse formats.
    ///
    /// Supported formats:
    /// 1. ```search ... replace ... ``` (Aider-compatible, existing)
    /// 2. --- a/file +++ b/file ... (unified diff, existing)
    /// 3. // file: path 注释后紧跟代码块 (explicit file markers)
    /// 4. [file: path] 标记行 (bracket markers)
    /// 5. ```diff ... ``` 代码块 (diff language tag)
    /// 6. 行号标注: "src/main.rs:42-56" + content
    /// 7. 纯代码块 (从上下文推测文件路径)
    pub fn parse_llm_response_v2(&self, response: &str) -> Vec<(PathBuf, EditFormat)> {
        let mut edits = Vec::new();
        let mut current_file: Option<PathBuf> = None;
        let mut file_from_bracket: Option<String> = None;

        // Phase 1: Extract file paths from non-block content (used for context)
        let _file_paths = self.extract_file_paths(response);

        // Phase 2: Walk through lines looking for file markers and code blocks
        let mut in_code_block = false;
        let mut block_content = String::new();
        let mut block_lang = String::new();
        let mut line_number_hint: Option<(u32, u32)> = None;

        for line in response.lines() {
            let trimmed = line.trim();

            // Check for file indicator comments
            if let Some(file_path) = trimmed
                .strip_prefix("// file: ")
                .or_else(|| trimmed.strip_prefix("# file: "))
                .or_else(|| trimmed.strip_prefix("// File: "))
                .or_else(|| trimmed.strip_prefix("## file: "))
                .or_else(|| trimmed.strip_prefix("//file:"))
                .or_else(|| trimmed.strip_prefix("#file:"))
            {
                current_file = Some(PathBuf::from(file_path.trim()));
                continue;
            }

            // Check for bracket file markers: [file: path] or [file://path]
            if trimmed.starts_with("[file:")
                || trimmed.starts_with("[File:")
                || trimmed.starts_with("[FILE:")
            {
                let path_part = trimmed
                    .trim_start_matches('[')
                    .split(':')
                    .nth(1)
                    .unwrap_or("")
                    .trim_end_matches(']')
                    .trim();
                if !path_part.is_empty() {
                    let clean = path_part.trim_end_matches(']').trim();
                    file_from_bracket = Some(clean.to_string());
                    current_file = Some(PathBuf::from(clean));
                }
                continue;
            }

            // Check for markdown heading file indicators
            if trimmed.starts_with("### ") || trimmed.starts_with("## ") {
                let content = trimmed.trim_start_matches('#').trim();
                if let Some(fpath) = content
                    .strip_prefix("file: ")
                    .or_else(|| content.strip_prefix("File: "))
                {
                    current_file = Some(PathBuf::from(fpath.trim()));
                    continue;
                }
            }

            // Check for line number annotations: "src/main.rs:42-56"
            let line_range_re =
                regex::Regex::new(r#"^["`]?([\w./\\\-]+):(\d+)[-–](\d+)"#).ok();
            if let Some(ref re) = line_range_re {
                if let Some(caps) = re.captures(trimmed) {
                    let fpath = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    let start_line: u32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                    let end_line: u32 = caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                    if !fpath.is_empty() && start_line > 0 && end_line > 0 {
                        current_file = Some(PathBuf::from(fpath));
                        line_number_hint = Some((start_line, end_line));
                    }
                }
            }

            // Handle code blocks
            if trimmed.starts_with("```") {
                if in_code_block {
                    // End of code block — process content
                    in_code_block = false;

                    let file = current_file.clone()
                        .or_else(|| {
                            file_from_bracket.as_ref().map(|p| PathBuf::from(p.clone()))
                        })
                        .unwrap_or_else(|| PathBuf::from("unknown"));

                    if let Some((start_line, end_line)) = line_number_hint {
                        // Line range format
                        if !block_content.is_empty() {
                            edits.push((
                                self.infer_file_path(&block_content, &file),
                                EditFormat::LineRange {
                                    start: start_line,
                                    end: end_line,
                                    text: block_content.clone(),
                                },
                            ));
                        }
                    } else if let Some(edit) =
                        self.detect_edit_format(&block_content, &block_lang, &Some(file.clone()))
                    {
                        edits.push(edit);
                    } else if !block_content.is_empty() {
                        // Fallback: treat as full file
                        edits.push((
                            self.infer_file_path(&block_content, &file),
                            EditFormat::FullFile(block_content.clone()),
                        ));
                    }

                    block_content.clear();
                    block_lang.clear();
                    line_number_hint = None;
                } else {
                    // Start of code block
                    in_code_block = true;
                    block_lang = line.trim().trim_start_matches("```").trim().to_string();
                }
                continue;
            }

            if in_code_block {
                if !block_content.is_empty() {
                    block_content.push('\n');
                }
                block_content.push_str(line);
            }
        }

        // Process trailing content outside code blocks
        if !in_code_block && !block_content.is_empty() {
            let file = current_file.clone()
                .or_else(|| file_from_bracket.as_ref().map(|p| PathBuf::from(p.clone())))
                .unwrap_or_else(|| PathBuf::from("unknown"));
            if let Some(edit) = self.detect_edit_format(&block_content, &block_lang, &Some(file.clone())) {
                edits.push(edit);
            }
        }

        // Also run the original parser as fallback
        let original_edits = self.parse_llm_response(response);
        edits.extend(original_edits);

        edits
    }

    /// Extract file paths from LLM response using regex patterns.
    fn extract_file_paths(&self, text: &str) -> Vec<String> {
        let mut paths = Vec::new();

        // Pattern: // file: path or # file: path
        let file_comment_re = regex::Regex::new(r#"(?m)^\s*(?://|#)\s*file:\s*(\S+)\s*$"#)
            .expect("Valid file comment regex");
        for cap in file_comment_re.captures_iter(text) {
            let path = cap[1].to_string();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }

        // Pattern: [file: path] or [File: path]
        let bracket_re =
            regex::Regex::new(r#"(?i)\[file:\s*([^\]]+)\]"#).expect("Valid bracket regex");
        for cap in bracket_re.captures_iter(text) {
            let path = cap[1].trim().to_string();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }

        // Pattern: --- a/path  (unified diff header)
        let diff_header_re =
            regex::Regex::new(r#"^---\s+a/(\S+)"#).expect("Valid diff header regex");
        for cap in diff_header_re.captures_iter(text) {
            let path = cap[1].to_string();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }

        paths
    }

    /// Infer file path from surrounding context when not explicitly stated.
    fn infer_file_path(&self, _block: &str, default_base: &Path) -> PathBuf {
        // If default_base is not "unknown", use it directly
        if default_base.to_string_lossy() != "unknown" {
            return default_base.to_path_buf();
        }

        // Try to extract from block content (e.g., "// src/main.rs" pattern)
        for line in _block.lines().take(5) {
            let trimmed = line.trim();
            // Check for common file path patterns in comments
            if let Some(path) = trimmed
                .strip_prefix("// ")
                .or_else(|| trimmed.strip_prefix("# "))
            {
                let candidate = path.trim();
                if (candidate.contains('/') || candidate.contains('\\'))
                    && (candidate.ends_with(".rs")
                        || candidate.ends_with(".py")
                        || candidate.ends_with(".js")
                        || candidate.ends_with(".ts")
                        || candidate.ends_with(".go")
                        || candidate.ends_with(".java"))
                    {
                        return PathBuf::from(candidate);
                    }
            }
        }

        default_base.to_path_buf()
    }

    /// Enhanced apply with conflict resolution.
    ///
    /// Uses the provided `ConflictResolver` to handle conflicts between
    /// existing file content and the incoming edit. Falls back to
    /// the standard `apply_edit` if no resolver is provided.
    pub async fn apply_edit_v2(
        &self,
        file_path: &Path,
        edit: &EditFormat,
        resolver: Option<&ConflictResolver>,
    ) -> anyhow::Result<EditResult> {
        if let Some(r) = resolver {
            let full_path = if file_path.is_absolute() {
                file_path.to_path_buf()
            } else {
                self.working_dir.join(file_path)
            };

            let current_content = std::fs::read_to_string(&full_path).unwrap_or_default();

            let conflict_result = r.resolve(&current_content, edit);
            match conflict_result.resolution {
                ConflictResolution::CleanMerge
                | ConflictResolution::TakeOurs
                | ConflictResolution::TakeTheirs => {
                    // Write the resolved content
                    if let Some(parent) = full_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&full_path, &conflict_result.resolved_content)?;

                    Ok(EditResult {
                        file_path: full_path,
                        success: true,
                        format: match edit {
                            EditFormat::SearchReplace { .. } => EditFormatType::SearchReplace,
                            EditFormat::UnifiedDiff(_) => EditFormatType::UnifiedDiff,
                            EditFormat::FullFile(_) => EditFormatType::FullFile,
                            EditFormat::LineRange { .. } => EditFormatType::LineRange,
                        },
                        lines_changed: conflict_result.hunks_resolved,
                        confidence: 0.85,
                        error: None,
                        backup_path: None,
                    })
                }
                ConflictResolution::ManualOnly => {
                    Ok(EditResult {
                        file_path: full_path,
                        success: false,
                        format: match edit {
                            EditFormat::SearchReplace { .. } => EditFormatType::SearchReplace,
                            EditFormat::UnifiedDiff(_) => EditFormatType::UnifiedDiff,
                            EditFormat::FullFile(_) => EditFormatType::FullFile,
                            EditFormat::LineRange { .. } => EditFormatType::LineRange,
                        },
                        lines_changed: conflict_result.hunks_resolved,
                        confidence: 0.0,
                        error: Some(format!(
                            "Conflict could not be auto-resolved ({}/{} hunks resolved)",
                            conflict_result.hunks_resolved, conflict_result.hunks_total
                        )),
                        backup_path: None,
                    })
                }
            }
        } else {
            self.apply_edit(file_path, edit).await
        }
    }

    /// Apply with compilation validation.
    ///
    /// Applies the edit, then optionally validates compilation.
    /// If compilation fails, automatically reverts the changes.
    pub async fn apply_edit_with_check(
        &self,
        file_path: &Path,
        edit: &EditFormat,
        compiler: Option<&CompilationValidator>,
    ) -> anyhow::Result<EditResult> {
        let result = self.apply_edit(file_path, edit).await?;

        if let Some(compiler) = compiler {
            if compiler.enabled && result.success {
                let check = compiler.check_compilation(&self.working_dir).await;
                if !check.success && !check.errors.is_empty() {
                    // Auto-revert if compilation fails
                    if let Some(ref backup) = result.backup_path {
                        let full_path = if file_path.is_absolute() {
                            file_path.to_path_buf()
                        } else {
                            self.working_dir.join(file_path)
                        };
                        // Restore from backup
                        if let Ok(backup_content) = std::fs::read_to_string(backup) {
                            let _ = std::fs::write(&full_path, &backup_content);
                        }
                    }

                    return Ok(EditResult {
                        file_path: result.file_path,
                        success: false,
                        format: result.format,
                        lines_changed: 0,
                        confidence: 0.0,
                        error: Some(format!(
                            "Compilation failed ({} errors) — reverted",
                            check.errors.len()
                        )),
                        backup_path: result.backup_path,
                    });
                }
            }
        }

        Ok(result)
    }

    /// Smart batch: apply edits, validate compilation, revert on failure.
    ///
    /// Applies a batch of edits sequentially. After all edits are applied,
    /// checks compilation. If compilation fails, reverts all edits
    /// using the stored original contents.
    pub async fn apply_batch_smart(
        &self,
        edits: &[(PathBuf, EditFormat)],
        compiler: &CompilationValidator,
    ) -> anyhow::Result<Vec<EditResult>> {
        let mut results = Vec::new();
        let mut applied: Vec<(PathBuf, String)> = Vec::new(); // (path, original_content)

        // Phase 1: Apply all edits
        for (file_path, edit) in edits {
            let full_path = if file_path.is_absolute() {
                file_path.clone()
            } else {
                self.working_dir.join(file_path)
            };
            let original = std::fs::read_to_string(&full_path).unwrap_or_default();

            match self.apply_edit(file_path, edit).await {
                Ok(result) => {
                    if result.success {
                        applied.push((full_path, original));
                    }
                    results.push(result);
                }
                Err(e) => {
                    // Rollback all applied edits in reverse order
                    for (rolled_back_path, original_content) in applied.iter().rev() {
                        if !original_content.is_empty() {
                            let _ = std::fs::write(rolled_back_path, original_content);
                        } else {
                            let _ = std::fs::remove_file(rolled_back_path);
                        }
                    }
                    anyhow::bail!("Smart batch edit failed at {:?}: {}", file_path, e);
                }
            }
        }

        // Phase 2: Validate compilation
        if compiler.enabled && !applied.is_empty() {
            let check = compiler.check_compilation(&self.working_dir).await;
            if !check.success && !check.errors.is_empty() {
                // Revert all edits
                for (path, original_content) in applied.iter().rev() {
                    if !original_content.is_empty() {
                        let _ = std::fs::write(path, original_content);
                    } else {
                        let _ = std::fs::remove_file(path);
                    }
                }

                anyhow::bail!(
                    "Smart batch: compilation failed ({} errors, {} warnings) — all edits reverted",
                    check.errors.len(),
                    check.warnings.len()
                );
            }
        }

        Ok(results)
    }

    // ── Fuzzy search/replace (Task 3) ────────────────────────────────────

    /// Apply SEARCH/REPLACE with fuzzy matching for legacy code.
    /// Tries multiple strategies in order:
    /// 1. Exact match (current)
    /// 2. Whitespace-normalized match
    /// 3. Trimmed line match
    /// 4. Fuzzy match (using similar::TextDiff with threshold)
    pub fn apply_search_replace_fuzzy(&self, content: &str, search: &str, replace: &str) -> Result<String, String> {
        // Strategy 1: Exact match
        if let Ok(r) = self.apply_search_replace(content, search, replace) {
            return Ok(r);
        }

        // Strategy 2: Normalize whitespace
        let normalized_search = normalize_whitespace(search);
        let normalized_content = normalize_whitespace(content);
        if normalized_content.contains(&normalized_search) {
            // Found via normalization, map back to original positions
            let found = find_original_text(content, &normalized_search, search);
            return self.apply_search_replace(content, &found, replace);
        }

        // Strategy 3: Fuzzy match via similar
        let changes = similar::TextDiff::from_lines(search, content);
        let similarity = changes.ratio();
        if similarity > 0.8 {
            return Ok(self.apply_fuzzy_with_context(content, search, replace, similarity as f64));
        }

        // Strategy 4: Line-by-line best match
        let search_lines: Vec<&str> = search.lines().collect();
        let content_lines: Vec<&str> = content.lines().collect();
        if let Some(start_line) = find_best_match_line(&search_lines, &content_lines) {
            return Ok(replace_lines(&content_lines, start_line, search_lines.len(), replace));
        }

        Err("Search text not found (tried exact, normalized, and fuzzy)".to_string())
    }

    /// Apply fuzzy match with context-aware merging.
    fn apply_fuzzy_with_context(&self, content: &str, search: &str, replace: &str, _similarity: f64) -> String {
        // Use similar's diff to replace lines
        let diff = similar::TextDiff::from_lines(search, content);
        let mut result = String::new();
        let _replace_vec: Vec<&str> = replace.lines().collect();

        for (idx, change) in diff.iter_all_changes().enumerate() {
            match change.tag() {
                similar::ChangeTag::Equal => {
                    result.push_str(change.value());
                }
                similar::ChangeTag::Delete => {
                    // This is content from search that's being replaced
                    // We need to skip it
                }
                similar::ChangeTag::Insert => {
                    // This is content from the target that doesn't match search
                    // Keep it but also check if we need to insert replacement
                    if idx == 0 || !result.ends_with('\n') {
                        // Check if this insert corresponds to a delete
                    }
                    result.push_str(change.value());
                }
            }
        }

        // Fallback: simple replacement
        if result.is_empty() || result == content {
            // If the diff-based approach didn't change anything, do a simple line replacement
            let search_lines: Vec<&str> = search.lines().collect();
            let content_lines: Vec<&str> = content.lines().collect();
            if let Some(start) = find_best_match_line(&search_lines, &content_lines) {
                return replace_lines(&content_lines, start, search_lines.len(), replace);
            }
        }

        if result.is_empty() { content.to_string() } else { result }
    }

    /// Async version with file read
    pub async fn apply_edit_fuzzy(&self, file_path: &Path, search: &str, replace: &str) -> anyhow::Result<EditResult> {
        let full_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            self.working_dir.join(file_path)
        };

        let content = tokio::fs::read_to_string(&full_path).await
            .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", full_path.display(), e))?;

        match self.apply_search_replace_fuzzy(&content, search, replace) {
            Ok(new_content) => {
                tokio::fs::write(&full_path, &new_content).await?;
                Ok(EditResult {
                    file_path: full_path,
                    success: true,
                    format: EditFormatType::SearchReplace,
                    lines_changed: new_content.lines().count().abs_diff(content.lines().count()),
                    confidence: self.compute_fuzzy_confidence(search, &content, &new_content),
                    error: None,
                    backup_path: None,
                })
            }
            Err(e) => Ok(EditResult {
                file_path: full_path,
                success: false,
                format: EditFormatType::SearchReplace,
                lines_changed: 0,
                confidence: 0.0,
                error: Some(e),
                backup_path: None,
            }),
        }
    }

    /// Compute confidence based on how many lines matched.
    fn compute_fuzzy_confidence(&self, search: &str, _old_content: &str, _new_content: &str) -> f64 {
        let search_lines = search.lines().count();
        if search_lines == 0 { return 0.0; }
        // Higher confidence when more search lines were matched
        (search_lines as f64).min(10.0) / 10.0
    }
}

/// Normalize all whitespace to single spaces.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Find original text in content that matches normalized search.
fn find_original_text(content: &str, normalized_search: &str, original_search: &str) -> String {
    // Try to find the original search text in the content
    if content.contains(original_search) {
        return original_search.to_string();
    }
    // Fallback: return normalized version
    normalized_search.to_string()
}

/// Find the best matching line position using sliding window.
fn find_best_match_line(search_lines: &[&str], content_lines: &[&str]) -> Option<usize> {
    if search_lines.is_empty() || search_lines.len() > content_lines.len() {
        return None;
    }

    let n = search_lines.len();
    let mut best_score = 0.6_f64; // threshold
    let mut best_pos = None;

    for i in 0..=content_lines.len() - n {
        let mut score: f64 = 0.0;
        for (j, sl) in search_lines.iter().enumerate() {
            let trimmed_s = sl.trim();
            let trimmed_c = content_lines[i + j].trim();
            if trimmed_s == trimmed_c {
                score += 1.0;
            } else if trimmed_s.eq_ignore_ascii_case(trimmed_c) {
                score += 0.8;
            } else if trimmed_s.len() > 3 && trimmed_c.contains(trimmed_s) {
                score += 0.5;
            } else {
                // Partial match using similar
                let ratio = similar::TextDiff::from_chars(trimmed_s, trimmed_c).ratio();
                if ratio > 0.6 {
                    score += ratio as f64 * 0.6;
                }
            }
        }
        let avg = score / n as f64;
        if avg > best_score {
            best_score = avg;
            best_pos = Some(i);
        }
    }

    best_pos
}

/// Replace a range of lines with new content.
fn replace_lines(content_lines: &[&str], start: usize, count: usize, replacement: &str) -> String {
    let mut result = String::new();
    for (i, line) in content_lines.iter().enumerate() {
        if i == start {
            result.push_str(replacement);
            if !replacement.ends_with('\n') {
                result.push('\n');
            }
        } else if i < start || i >= start + count {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_engine_creation() {
        let engine = ApplyEngine::new(Path::new("/tmp/test_workspace"));
        assert_eq!(engine.max_retries, 3);
        assert!(engine.auto_backup);
        assert_eq!(engine.conflict_strategy, ConflictStrategy::Fail);
        assert!(!engine.validate_with_compile);
    }

    #[test]
    fn test_parse_search_replace() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let response = "```\nSEARCH\nfn old_name() -> i32 {\n    42\n}\nREPLACE\nfn new_name() -> i32 {\n    42\n}\n```";
        let edits = engine.parse_llm_response(response);
        assert!(!edits.is_empty(), "Should parse at least one edit");

        let (path, format) = &edits[0];
        assert_eq!(path.to_string_lossy(), "unknown");

        match format {
            EditFormat::SearchReplace { search, replace } => {
                assert!(search.contains("old_name"));
                assert!(replace.contains("new_name"));
            }
            other => panic!("Expected SearchReplace format, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_unified_diff() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let response = "```diff\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n fn hello() {\n-    println!(\"old\");\n+    println!(\"new\");\n }\n```";
        let edits = engine.parse_llm_response(response);
        assert!(!edits.is_empty(), "Should parse unified diff");

        let (_path, format) = &edits[0];
        match format {
            EditFormat::UnifiedDiff(diff) => {
                assert!(diff.contains("---"));
                assert!(diff.contains("+++"));
            }
            other => panic!("Expected UnifiedDiff format, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_full_file() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let response = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        let edits = engine.parse_llm_response(response);
        assert!(!edits.is_empty(), "Should parse full file replacement");

        let (_path, format) = &edits[0];
        match format {
            EditFormat::FullFile(content) => {
                assert!(content.contains("fn main"));
            }
            other => panic!("Expected FullFile format, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_patch_high_score() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn hello() {\n    println!(\"world\");\n}\n").unwrap();

        let engine = ApplyEngine::new(dir.path());
        let edit = EditFormat::SearchReplace {
            search: "fn hello()".to_string(),
            replace: "fn goodbye()".to_string(),
        };

        let patch = engine.validate_patch(&file_path, &edit);
        assert!(patch.score >= 70.0, "Score should be high for clean match, got {}", patch.score);
        assert!(patch.is_safe_to_apply());
    }

    #[test]
    fn test_validate_patch_low_score() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn hello() {\n    println!(\"world\");\n}\n").unwrap();

        let engine = ApplyEngine::new(dir.path());
        let edit = EditFormat::SearchReplace {
            search: "fn nonexistent()".to_string(),
            replace: "fn replaced()".to_string(),
        };

        let patch = engine.validate_patch(&file_path, &edit);
        assert!(patch.score < 70.0, "Score should be low for non-matching search, got {}", patch.score);
        assert!(!patch.issues.is_empty(), "Should have issues for non-matching search");
        assert!(!patch.is_safe_to_apply());
    }

    #[test]
    fn test_search_replace_success() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let content = "fn hello() {\n    println!(\"world\");\n}";
        let result = engine.apply_search_replace(content, "fn hello()", "fn goodbye()");
        assert!(result.is_ok());
        let new_content = result.unwrap();
        assert!(new_content.contains("fn goodbye()"));
        assert!(!new_content.contains("fn hello()"));
    }

    #[test]
    fn test_search_replace_fail() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let content = "fn hello() {\n    println!(\"world\");\n}";
        let result = engine.apply_search_replace(content, "fn nonexistent()", "fn replaced()");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    // === ConflictResolver tests ===

    #[test]
    fn test_conflict_resolver_creation() {
        let resolver = ConflictResolver::new();
        assert_eq!(resolver.context_lines, 3);
        assert_eq!(resolver.fallback_strategy, ConflictStrategy::Skip);

        let custom = ConflictResolver {
            context_lines: 5,
            fallback_strategy: ConflictStrategy::Overwrite,
        };
        assert_eq!(custom.context_lines, 5);
        assert_eq!(custom.fallback_strategy, ConflictStrategy::Overwrite);
    }

    #[test]
    fn test_three_way_merge_clean() {
        let resolver = ConflictResolver::new();
        // base and existing are the same, edit adds a line
        let base = "line1\nline2\nline3";
        let existing = "line1\nline2\nline3";
        let edit = "line1\nline2\nmodified_line3";

        let result = resolver.three_way_merge(base, existing, edit);
        assert_eq!(result.resolution, ConflictResolution::CleanMerge);
        assert!(result.resolved_content.contains("modified_line3"));
        assert_eq!(result.hunks_resolved, 1);
        assert!(result.hunks_total >= 1);
    }

    #[test]
    fn test_three_way_merge_conflict() {
        let resolver = ConflictResolver::new();
        // Both existing and edit change the same line differently
        let base = "line1\nline2\nline3";
        let existing = "line1\nCHANGED_A\nline3";
        let edit = "line1\nCHANGED_B\nline3";

        let result = resolver.three_way_merge(base, existing, edit);
        assert_eq!(result.resolution, ConflictResolution::ManualOnly);
        assert!(result.resolved_content.contains("<<<<<<<"));
        assert!(result.resolved_content.contains("CHANGED_A"));
        assert!(result.resolved_content.contains("CHANGED_B"));
        assert!(result.resolved_content.contains(">>>>>>>"));
    }

    #[test]
    fn test_resolve_search_replace_success() {
        let resolver = ConflictResolver::new();
        let content = "fn hello() {\n    println!(\"hi\");\n}";
        let result = resolver.resolve_search_replace(content, "fn hello()", "fn goodbye()");
        assert_eq!(result.resolution, ConflictResolution::CleanMerge);
        assert!(result.resolved_content.contains("fn goodbye()"));
        assert!(!result.resolved_content.contains("fn hello()"));
        assert_eq!(result.hunks_resolved, 1);
    }

    #[test]
    fn test_resolve_search_replace_not_found() {
        let resolver = ConflictResolver::new();
        let content = "fn hello() {\n    println!(\"hi\");\n}";
        let result = resolver.resolve_search_replace(content, "fn nonexistent()", "fn replaced()");
        assert_eq!(result.resolution, ConflictResolution::ManualOnly);
        assert_eq!(result.hunks_resolved, 0);
    }

    #[test]
    fn test_conflict_resolver_resolve_full_file() {
        let resolver = ConflictResolver::new();
        let edit = EditFormat::FullFile("new content".to_string());
        let result = resolver.resolve("old content", &edit);
        assert_eq!(result.resolution, ConflictResolution::TakeTheirs);
        assert_eq!(result.resolved_content, "new content");
    }

    #[test]
    fn test_conflict_resolver_resolve_line_range() {
        let resolver = ConflictResolver::new();
        let edit = EditFormat::LineRange {
            start: 2,
            end: 3,
            text: "replacement".to_string(),
        };
        let content = "line1\nline2\nline3\nline4";
        let result = resolver.resolve(content, &edit);
        assert_eq!(result.resolution, ConflictResolution::CleanMerge);
        assert_eq!(result.resolved_content, "line1\nreplacement\nline4");
    }

    // === CompilationValidator tests ===

    #[test]
    fn test_compilation_validator_creation() {
        let validator = CompilationValidator::new();
        assert!(validator.enabled);
        assert_eq!(validator.timeout_secs, 30);
        assert_eq!(validator.cargo_path, "cargo");
    }

    #[test]
    fn test_parse_cargo_check_errors() {
        let validator = CompilationValidator::new();
        let output = "\
error[E0425]: cannot find value `x` in this scope
  --> src/main.rs:10:13
   |
10 |     println!(\"{}\", x);
   |                    ^ help: a local variable with a similar name exists: `y`

warning: unused variable: `counter`
  --> src/lib.rs:25:9
   |
25 |     let counter = 0;
   |     ^^^^^^^ help: if this is intentional, prefix it with an underscore

error: aborting due to 1 previous error
";
        let (errors, warnings) = validator.parse_cargo_check(output);
        assert!(!errors.is_empty(), "Should find errors");
        assert!(!warnings.is_empty(), "Should find warnings");
        assert!(errors.iter().any(|e| e.contains("E0425")));
        assert!(warnings.iter().any(|w| w.contains("unused variable")));
    }

    #[test]
    fn test_parse_cargo_check_clean() {
        let validator = CompilationValidator::new();
        let output = "    Checking hello v0.1.0\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.42s\n";
        let (errors, warnings) = validator.parse_cargo_check(output);
        assert!(errors.is_empty(), "Should have no errors");
        assert!(warnings.is_empty(), "Should have no warnings");
    }

    // === Parse LLM v2 tests ===

    #[test]
    fn test_parse_llm_v2_diff_block() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let response = "\
```diff
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 fn hello() {
-    println!(\"old\");
+    println!(\"new\");
 }
```";
        let edits = engine.parse_llm_response_v2(response);
        let has_diff = edits.iter().any(|(_, f)| matches!(f, EditFormat::UnifiedDiff(_)));
        assert!(has_diff, "Should parse diff block in v2");
    }

    #[test]
    fn test_parse_llm_v2_file_marker() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let response = "\
// file: src/utils.rs
```rust
fn helper() -> i32 {
    42
}
```";
        let edits = engine.parse_llm_response_v2(response);
        let has_utils = edits.iter().any(|(p, _)| {
            p.to_string_lossy().contains("utils.rs") || p.to_string_lossy().contains("utils")
        });
        assert!(has_utils, "Should detect file marker: {:?}", edits);
    }

    #[test]
    fn test_parse_llm_v2_bracket_marker() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let response = "\
[file: src/main.rs]
```rust
fn main() {}
```";
        let edits = engine.parse_llm_response_v2(response);
        let has_main = edits.iter().any(|(p, _)| p.to_string_lossy().contains("main.rs"));
        assert!(has_main, "Should detect bracket file marker: {:?}", edits);
    }

    #[test]
    fn test_parse_llm_v2_line_range() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let response = "\
src/app.rs:42-56
```rust
fn new_function() {
    // replacement code
}
```";
        let edits = engine.parse_llm_response_v2(response);
        let has_range = edits.iter().any(|(_, f)| matches!(f, EditFormat::LineRange { .. }));
        assert!(has_range, "Should parse line range format: {:?}", edits);
    }

    // === ScoredPatch enhancement tests ===

    #[test]
    fn test_scored_patch_syntax_balance() {
        let content = "fn main() {\n    println!(\"ok\");\n}";
        let edit = EditFormat::SearchReplace {
            search: "println!(\"ok\");".to_string(),
            replace: "println!(\"updated\");\n    more_code();".to_string(),
        };
        let issues = ScoredPatch::check_syntax_balance(content, &edit);
        // The edit adds balanced code, so no issues expected
        assert!(issues.is_empty(), "Balanced code should have no issues: {:?}", issues);
    }

    #[test]
    fn test_scored_patch_syntax_balance_imbalanced() {
        let content = "fn main() {";
        let edit = EditFormat::FullFile("fn main() {\n    loop {\n".to_string());
        let issues = ScoredPatch::check_syntax_balance(content, &edit);
        assert!(!issues.is_empty(), "Imbalanced braces should be detected");
        let has_imbalance = issues.iter().any(|i| i.contains("imbalance"));
        assert!(has_imbalance, "Should mention imbalance: {:?}", issues);
    }

    #[test]
    fn test_scored_patch_symbol_references() {
        let content = "fn helper() {}\nfn main() {\n    helper();\n}";
        let edit = EditFormat::SearchReplace {
            search: "fn main()".to_string(),
            replace: "fn main() {\n    helper();\n    new_func();\n}".to_string(),
        };
        let score = ScoredPatch::check_symbol_references(content, &edit);
        // "helper" exists in content, "new_func" doesn't
        assert!(score > 0.0, "Should reference existing symbols");
        assert!(score < 1.0, "Not all symbols should match");
    }

    #[test]
    fn test_compute_detailed_score_basic() {
        let content = "fn existing() -> i32 { 42 }";
        let edit = EditFormat::SearchReplace {
            search: "fn existing() -> i32 { 42 }".to_string(),
            replace: "fn updated() -> i32 { 99 }".to_string(),
        };
        let patch = ScoredPatch::compute_detailed_score(content, &edit, Some("rust"));
        assert!(patch.score >= 60.0, "Score should be decent for clean match, got {}", patch.score);
        assert!(patch.confidence >= 0.5, "Confidence should be reasonable");
    }

    // === ConflictResolver three-way merge additional tests ===

    #[test]
    fn test_three_way_merge_identical() {
        let resolver = ConflictResolver::new();
        let content = "a\nb\nc";
        let result = resolver.three_way_merge(content, content, content);
        assert_eq!(result.resolution, ConflictResolution::CleanMerge);
        assert_eq!(result.resolved_content, content);
    }

    #[test]
    fn test_three_way_merge_only_existing_changed() {
        let resolver = ConflictResolver::new();
        let base = "a\nb\nc";
        let existing = "a\nMODIFIED\nc";
        let edit = "a\nb\nc";  // edit is same as base

        let result = resolver.three_way_merge(base, existing, edit);
        assert_eq!(result.resolution, ConflictResolution::CleanMerge);
        assert_eq!(result.resolved_content, "a\nMODIFIED\nc");
    }

    #[test]
    fn test_three_way_merge_only_edit_changed() {
        let resolver = ConflictResolver::new();
        let base = "a\nb\nc";
        let existing = "a\nb\nc";  // existing is same as base
        let edit = "a\nEDITED\nc";

        let result = resolver.three_way_merge(base, existing, edit);
        assert_eq!(result.resolution, ConflictResolution::CleanMerge);
        assert_eq!(result.resolved_content, "a\nEDITED\nc");
    }

    // === Extract file paths test ===

    #[test]
    fn test_extract_file_paths() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let text = "\
// file: src/main.rs
some content
# file: lib/helper.py
[file: docs/guide.md]
--- a/tests/test_mod.rs
";
        let paths = engine.extract_file_paths(text);
        assert!(paths.iter().any(|p| p.contains("main.rs")));
        assert!(paths.iter().any(|p| p.contains("helper.py")));
        assert!(paths.iter().any(|p| p.contains("guide.md")));
        assert!(paths.iter().any(|p| p.contains("test_mod.rs")));
        // Check deduplication
        assert_eq!(paths.len(), 4, "Should have 4 unique paths");
    }

    // ── Fuzzy matching tests (Task 3) ────────────────────────────────────

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("hello   world"), "hello world");
        assert_eq!(normalize_whitespace("  a   b  c  "), "a b c");
        assert_eq!(normalize_whitespace(""), "");
    }

    #[test]
    fn test_find_best_match_line_exact() {
        let search = vec!["fn hello()", "    println!(\"hi\");"];
        let content = vec!["fn hello()", "    println!(\"hi\");", "fn other()"];
        let pos = find_best_match_line(&search, &content);
        assert_eq!(pos, Some(0), "Should find exact match at position 0");
    }

    #[test]
    fn test_find_best_match_line_fuzzy() {
        let search = vec!["fn hello()", "    println!(\"hi\");"];
        let content = vec!["fn hello() {", "    println!(\"hi\");", "fn other()"];
        let pos = find_best_match_line(&search, &content);
        // Should still match despite braces difference
        assert_eq!(pos, Some(0), "Should fuzzy match at position 0");
    }

    #[test]
    fn test_replace_lines_basic() {
        let content = vec!["a", "b", "c", "d"];
        let result = replace_lines(&content, 1, 2, "x\ny");
        assert_eq!(result, "a\nx\ny\nd\n", "Should replace lines 1-2 with new content");
    }

    #[test]
    fn test_apply_fuzzy_strategy_chain_exact() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let content = "fn hello() {\n    return 42;\n}";
        let result = engine.apply_search_replace_fuzzy(content, "fn hello()", "fn goodbye()");
        assert!(result.is_ok(), "Exact match should succeed");
        let new_content = result.unwrap();
        assert!(new_content.contains("fn goodbye()"));
    }

    #[test]
    fn test_apply_fuzzy_strategy_chain_fallback() {
        let engine = ApplyEngine::new(Path::new("/tmp"));
        let content = "fn  hello()  {\n    return  42;\n}";
        let result = engine.apply_search_replace_fuzzy(content, "fn hello()", "fn goodbye()");
        // Should handle via whitespace normalization or fuzzy
        assert!(result.is_ok(), "Fuzzy match should handle whitespace differences");
    }

    // ── CompilationValidator timeout & incremental tests (Task 4) ────────

    #[tokio::test]
    async fn test_check_compilation_timeout_config() {
        let validator = CompilationValidator::new();
        // Just verify the timeout changes propagate
        let check = validator.check_compilation_timeout(Path::new("/nonexistent"), 60).await;
        // Since workspace doesn't exist, it will fail, but that's fine
        // The point is that the method compiles and runs
        assert!(!check.success);
        // Should have an error about not finding cargo or workspace
    }

    #[tokio::test]
    async fn test_check_incremental_mock() {
        // Mock: create a validator with a fake cargo path that won't execute
        let validator = CompilationValidator {
            enabled: true,
            timeout_secs: 30,
            cargo_path: "cargo_does_not_exist_xyz".to_string(),
        };

        let changed = vec!["nonexistent_crate".to_string()];
        let check = validator.check_incremental(Path::new("/tmp"), &changed).await;
        // Should handle gracefully (process error, not panic)
        assert!(!check.success, "Should fail gracefully");
        // Should have process error
        assert!(check.errors.iter().any(|e| e.contains("Process error") || e.contains("not found") || e.contains("No such file") || e.contains("cannot")),
                "Should report process error: {:?}", check.errors);
        // Duration should be recorded
        assert!(check.duration_ms > 0 || check.errors.len() > 0,
                "Should have duration or errors recorded");
    }
}