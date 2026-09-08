//! Context window optimizer — smart summary, attachment trimming, priority scoring.
//!
//! ## Components
//!
//! 1. **SmartSummarizer**: Extracts key information from tool outputs using
//!    pattern matching (file paths, error messages, line counts, etc.).
//!    Much faster than embedding-based summarization — no API calls needed.
//!
//! 2. **AttachmentTrimmer**: For large files (>1000 lines), keeps only the
//!    most relevant code blocks. Uses heuristic scoring based on:
//!    - Function/class definitions
//!    - Import statements
//!    - Lines near the user's cursor position
//!    - Lines containing key terms from the user's query
//!
//! 3. **ContextScorer**: Ranks context items by relevance using:
//!    - Edit distance to user query terms
//!    - Recency (newer = higher score)
//!    - Reference frequency (how often a file is @mentioned)
//!    - Token budget allocation

use std::collections::HashSet;
use std::time::Instant;

/// Configuration for smart summarization.
#[derive(Debug, Clone)]
pub struct SmartSummarizerConfig {
    /// Maximum output length for a summary (chars).
    pub max_summary_chars: usize,
    /// Whether to include file paths in summaries.
    pub include_file_paths: bool,
    /// Whether to include error messages in summaries.
    pub include_errors: bool,
    /// Whether to strip ANSI escape codes.
    pub strip_ansi: bool,
}

impl Default for SmartSummarizerConfig {
    fn default() -> Self {
        Self {
            max_summary_chars: 500,
            include_file_paths: true,
            include_errors: true,
            strip_ansi: true,
        }
    }
}

/// A smart summary of a tool output.
#[derive(Debug, Clone)]
pub struct SmartSummary {
    /// Condensed summary text.
    pub summary: String,
    /// What percentage of the original content was retained.
    pub compression_ratio: f64,
    /// Key file paths found in the output.
    pub file_paths: Vec<String>,
    /// Key error messages found in the output.
    pub errors: Vec<String>,
    /// Line count of original output.
    pub original_lines: usize,
    /// Line count of summary.
    pub summary_lines: usize,
}

/// Smart summarizer that extracts key information from tool outputs.
pub struct SmartSummarizer {
    config: SmartSummarizerConfig,
}

impl SmartSummarizer {
    pub fn new(config: SmartSummarizerConfig) -> Self {
        Self { config }
    }

    /// Summarize a tool output by extracting the most important information.
    pub fn summarize(&self, output: &str, tool_name: &str) -> SmartSummary {
        let original_lines = output.lines().count();
        let cleaned = if self.config.strip_ansi {
            Self::strip_ansi_codes(output)
        } else {
            output.to_string()
        };

        let mut parts: Vec<String> = Vec::new();
        let mut file_paths = Vec::new();
        let mut errors = Vec::new();

        match tool_name {
            "read_file" | "write_file" | "apply_edit" => {
                Self::summarize_file_output(&cleaned, &mut parts, &mut file_paths, &self.config);
            }
            "execute_command" | "bash" => {
                Self::summarize_command_output(&cleaned, &mut parts, &mut errors, &self.config);
            }
            "search_code" | "grep" | "glob" => {
                Self::summarize_search_output(&cleaned, &mut parts, &mut file_paths, &self.config);
            }
            "list_directory" | "ls" => {
                Self::summarize_list_output(&cleaned, &mut parts, &self.config);
            }
            _ => {
                Self::summarize_generic_output(&cleaned, &mut parts, &self.config);
            }
        }

        let summary = parts.join("\n");
        let summary_lines = summary.lines().count();
        let compression_ratio = if original_lines > 0 {
            summary_lines as f64 / original_lines as f64
        } else {
            1.0
        };

        SmartSummary {
            summary,
            compression_ratio,
            file_paths,
            errors,
            original_lines,
            summary_lines,
        }
    }

    /// Summarize a file read/write output.
    fn summarize_file_output(
        cleaned: &str,
        parts: &mut Vec<String>,
        file_paths: &mut Vec<String>,
        config: &SmartSummarizerConfig,
    ) {
        ///// Extract file path from the first line (common format)
        if let Some(_first_line) = cleaned.lines().next() {
            if config.include_file_paths {
                // Try to find file paths in the output
                for line in cleaned.lines().take(5) {
                    for path in extract_file_paths(line) {
                        if !file_paths.contains(&path) {
                            file_paths.push(path);
                        }
                    }
                }
            }
        }

        let lines: Vec<&str> = cleaned.lines().collect();
        let total = lines.len();

        if total <= 20 {
            // Small file — keep all
            parts.push(cleaned.to_string());
            return;
        }

        // Large file — extract key parts
        let mut kept = Vec::new();

        // Keep first 5 lines (imports, package declaration)
        kept.extend(lines.iter().take(5).copied());

        // Keep function/class definition lines
        let important_lines = extract_important_lines(&lines);
        kept.extend(important_lines);

        // Keep last 5 lines
        kept.extend(lines.iter().rev().take(5).copied().collect::<Vec<_>>().into_iter().rev());

        // Deduplicate while preserving order
        let mut seen = HashSet::new();
        let deduped: Vec<&str> = kept.into_iter()
            .filter(|l| seen.insert(l.to_string()))
            .collect();

        let result = deduped.join("\n");
        let truncated = truncate_to_chars(&result, config.max_summary_chars);
        parts.push(truncated);
        parts.push(format!("[{} lines total, showing key lines]", total));
    }

    /// Summarize command output.
    fn summarize_command_output(
        cleaned: &str,
        parts: &mut Vec<String>,
        errors: &mut Vec<String>,
        config: &SmartSummarizerConfig,
    ) {
        let lines: Vec<&str> = cleaned.lines().collect();

        // Extract error messages
        if config.include_errors {
            for line in &lines {
                let lower = line.to_lowercase();
                if lower.contains("error") || lower.contains("fail") || lower.contains("panic") {
                    errors.push(line.to_string());
                }
            }
        }

        if lines.len() <= 30 {
            parts.push(cleaned.to_string());
            return;
        }

        // Keep first 10 lines + error lines + last 10 lines
        let mut kept: Vec<&str> = lines.iter().take(10).copied().collect();

        if !errors.is_empty() {
            kept.push("--- errors ---");
            kept.extend(errors.iter().map(|s| s.as_str()));
        }

        kept.push("--- tail ---");
        kept.extend(lines.iter().rev().take(10).copied().collect::<Vec<_>>().into_iter().rev());

        let result = kept.join("\n");
        parts.push(truncate_to_chars(&result, config.max_summary_chars));
        parts.push(format!("[{} lines total]", lines.len()));
    }

    /// Summarize search/grep output.
    fn summarize_search_output(
        cleaned: &str,
        parts: &mut Vec<String>,
        file_paths: &mut Vec<String>,
        config: &SmartSummarizerConfig,
    ) {
        let lines: Vec<&str> = cleaned.lines().collect();

        // Extract file paths
        for line in &lines {
            for path in extract_file_paths(line) {
                if !file_paths.contains(&path) {
                    file_paths.push(path);
                }
            }
        }

        if lines.len() <= 20 {
            parts.push(cleaned.to_string());
            return;
        }

        // Keep first 5 results + last 5 results
        let mut kept = Vec::new();
        kept.extend(lines.iter().take(5).copied());
        kept.push("...");
        kept.extend(lines.iter().rev().take(5).copied().collect::<Vec<_>>().into_iter().rev());

        let result = kept.join("\n");
        parts.push(truncate_to_chars(&result, config.max_summary_chars));
        parts.push(format!("[{} matches total]", lines.len()));
    }

    /// Summarize directory listing output.
    fn summarize_list_output(cleaned: &str, parts: &mut Vec<String>, config: &SmartSummarizerConfig) {
        let lines: Vec<&str> = cleaned.lines().collect();
        parts.push(truncate_to_chars(cleaned, config.max_summary_chars));
        parts.push(format!("[{} entries]", lines.len()));
    }

    /// Generic summarization for unknown tool types.
    fn summarize_generic_output(cleaned: &str, parts: &mut Vec<String>, config: &SmartSummarizerConfig) {
        let lines: Vec<&str> = cleaned.lines().collect();
        if lines.len() <= 30 {
            parts.push(cleaned.to_string());
        } else {
            let mut kept = Vec::new();
            kept.extend(lines.iter().take(10).copied());
            kept.push("...");
            kept.extend(lines.iter().rev().take(10).copied().collect::<Vec<_>>().into_iter().rev());
            parts.push(truncate_to_chars(&kept.join("\n"), config.max_summary_chars));
            parts.push(format!("[{} lines total]", lines.len()));
        }
    }

    /// Strip ANSI escape codes from a string.
    fn strip_ansi_codes(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' && chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }
}

impl Default for SmartSummarizer {
    fn default() -> Self {
        Self::new(SmartSummarizerConfig::default())
    }
}

// ============================================================================
// Attachment Trimmer — for large files, keep only relevant code blocks
// ============================================================================

/// Configuration for attachment trimming.
#[derive(Debug, Clone)]
pub struct AttachmentTrimConfig {
    /// Maximum lines to keep from a single file attachment.
    pub max_lines: usize,
    /// Lines of context around important lines.
    pub context_lines: usize,
    /// Whether to always keep imports/package declarations.
    pub keep_imports: bool,
    /// Minimum file size (lines) to trigger trimming.
    pub min_lines_to_trim: usize,
}

impl Default for AttachmentTrimConfig {
    fn default() -> Self {
        Self {
            max_lines: 200,
            context_lines: 3,
            keep_imports: true,
            min_lines_to_trim: 1000,
        }
    }
}

/// Result of trimming a file attachment.
#[derive(Debug, Clone)]
pub struct TrimmedAttachment {
    /// Trimmed content.
    pub content: String,
    /// Original line count.
    pub original_lines: usize,
    /// Trimmed line count.
    pub trimmed_lines: usize,
    /// Compression ratio.
    pub compression_ratio: f64,
    /// Section markers showing what was removed.
    pub sections: Vec<String>,
}

/// Trim large file attachments to keep only relevant code blocks.
pub struct AttachmentTrimmer {
    config: AttachmentTrimConfig,
}

impl AttachmentTrimmer {
    pub fn new(config: AttachmentTrimConfig) -> Self {
        Self { config }
    }

    /// Trim a file attachment to keep only relevant parts.
    ///
    /// `query_terms` — terms from the user's query to prioritize.
    /// `cursor_line` — the user's current cursor position (1-indexed).
    pub fn trim(
        &self,
        content: &str,
        query_terms: &[String],
        cursor_line: Option<usize>,
    ) -> TrimmedAttachment {
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        if total <= self.config.min_lines_to_trim {
            return TrimmedAttachment {
                content: content.to_string(),
                original_lines: total,
                trimmed_lines: total,
                compression_ratio: 1.0,
                sections: vec!["[full file]".into()],
            };
        }

        let mut important_indices: HashSet<usize> = HashSet::new();

        // 1. Always keep import/package lines
        if self.config.keep_imports {
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("import ")
                    || trimmed.starts_with("use ")
                    || trimmed.starts_with("from ")
                    || trimmed.starts_with("require(")
                    || trimmed.starts_with("#include")
                    || trimmed.starts_with("package ")
                    || trimmed.starts_with("mod ")
                {
                    important_indices.insert(i);
                }
            }
        }

        // 2. Keep function/class definition lines
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("def ")
                || trimmed.starts_with("class ")
                || trimmed.starts_with("struct ")
                || trimmed.starts_with("trait ")
                || trimmed.starts_with("impl ")
                || trimmed.starts_with("interface ")
                || trimmed.starts_with("func ")
                || trimmed.starts_with("function ")
            {
                important_indices.insert(i);
            }
        }

        // 3. Keep lines near cursor position
        if let Some(cursor) = cursor_line {
            let ctx = self.config.context_lines;
            let start = cursor.saturating_sub(ctx);
            let end = (cursor + ctx).min(total);
            for i in start..end {
                important_indices.insert(i);
            }
        }

        // 4. Keep lines matching query terms
        for term in query_terms {
            if term.len() < 3 { continue; }
            for (i, line) in lines.iter().enumerate() {
                if line.to_lowercase().contains(&term.to_lowercase()) {
                    // Include context lines around each match
                    let ctx = self.config.context_lines;
                    let start = i.saturating_sub(ctx);
                    let end = (i + ctx + 1).min(total);
                    for j in start..end {
                        important_indices.insert(j);
                    }
                }
            }
        }

        // Build trimmed output with section markers
        let mut sorted: Vec<usize> = important_indices.into_iter().collect();
        sorted.sort();

        let mut result = Vec::new();
        let mut sections = Vec::new();
        let mut last_idx = 0usize;
        let mut in_gap = false;

        for &idx in &sorted {
            if idx > last_idx + 1 && last_idx > 0 {
                if !in_gap {
                    let skipped = idx - last_idx - 1;
                    result.push(format!("// ... {} lines omitted ...", skipped));
                    sections.push(format!("[{} lines skipped at line {}]", skipped, last_idx + 1));
                }
                in_gap = true;
            } else {
                in_gap = false;
            }
            if idx < lines.len() {
                result.push(lines[idx].to_string());
            }
            last_idx = idx;
        }

        let trimmed = result.join("\n");
        let trimmed_lines = result.len();

        // If trimming produced too much, truncate further
        let final_content = if trimmed_lines > self.config.max_lines {
            let kept: Vec<&str> = result.iter().take(self.config.max_lines / 2)
                .map(|s| s.as_str())
                .collect();
            let tail: Vec<&str> = result.iter().rev().take(self.config.max_lines / 4)
                .map(|s| s.as_str())
                .collect::<Vec<_>>().into_iter().rev().collect();
            let mut final_lines: Vec<&str> = kept;
            final_lines.push("// ... truncated ...");
            final_lines.extend(tail);
            final_lines.join("\n")
        } else {
            trimmed
        };

        TrimmedAttachment {
            content: final_content,
            original_lines: total,
            trimmed_lines: trimmed_lines.min(self.config.max_lines),
            compression_ratio: trimmed_lines.min(self.config.max_lines) as f64 / total as f64,
            sections,
        }
    }
}

impl Default for AttachmentTrimmer {
    fn default() -> Self {
        Self::new(AttachmentTrimConfig::default())
    }
}

// ============================================================================
// Context Priority Scorer
// ============================================================================

/// Configuration for context scoring.
#[derive(Debug, Clone)]
pub struct ContextScorerConfig {
    /// Weight for edit distance similarity (0.0 to 1.0).
    pub edit_distance_weight: f64,
    /// Weight for recency (0.0 to 1.0).
    pub recency_weight: f64,
    /// Weight for reference frequency (0.0 to 1.0).
    pub reference_frequency_weight: f64,
    /// Maximum number of context items to keep.
    pub max_context_items: usize,
}

impl Default for ContextScorerConfig {
    fn default() -> Self {
        Self {
            edit_distance_weight: 0.4,
            recency_weight: 0.3,
            reference_frequency_weight: 0.3,
            max_context_items: 20,
        }
    }
}

/// A scored context item.
#[derive(Debug, Clone)]
pub struct ScoredContext {
    /// The context content.
    pub content: String,
    /// Source identifier (file path, tool name, etc.).
    pub source: String,
    /// Relevance score (0.0 to 1.0).
    pub score: f64,
    /// When this context was added.
    pub timestamp: Instant,
    /// How many times this context has been referenced.
    pub reference_count: u64,
}

/// Context priority scorer that ranks items by relevance.
pub struct ContextScorer {
    config: ContextScorerConfig,
    /// Tracked context items.
    items: Vec<ScoredContext>,
}

impl ContextScorer {
    pub fn new(config: ContextScorerConfig) -> Self {
        Self {
            config,
            items: Vec::new(),
        }
    }

    /// Add a context item.
    pub fn add(&mut self, content: String, source: String) {
        // Increment reference count if already exists
        if let Some(existing) = self.items.iter_mut().find(|i| i.source == source) {
            existing.reference_count += 1;
            existing.timestamp = Instant::now();
            existing.content = content;
            return;
        }

        self.items.push(ScoredContext {
            content,
            source,
            score: 0.0,
            timestamp: Instant::now(),
            reference_count: 1,
        });
    }

    /// Score and rank all context items against the user's query.
    /// Returns the top N items by relevance score.
    pub fn rank(&mut self, query: &str, max_tokens: Option<usize>) -> Vec<ScoredContext> {
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        let now = Instant::now();

        // Score each item
        for item in &mut self.items {
            let mut score = 0.0;

            // 1. Edit distance / term overlap
            let content_lower = item.content.to_lowercase();
            let mut term_matches = 0u32;
            for term in &query_terms {
                if term.len() >= 3 && content_lower.contains(term) {
                    term_matches += 1;
                }
            }
            let term_score = if query_terms.is_empty() {
                0.5
            } else {
                term_matches as f64 / query_terms.len() as f64
            };
            score += term_score * self.config.edit_distance_weight;

            // 2. Recency (newer = higher)
            let age_secs = now.duration_since(item.timestamp).as_secs_f64();
            let recency = 1.0 / (1.0 + age_secs / 300.0); // decays over 5 minutes
            score += recency * self.config.recency_weight;

            // 3. Reference frequency
            let ref_score = (item.reference_count as f64 / 10.0).min(1.0);
            score += ref_score * self.config.reference_frequency_weight;

            item.score = score;
        }

        // Sort by score descending
        self.items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Limit to max items
        let limit = self.config.max_context_items;
        if self.items.len() > limit {
            self.items.truncate(limit);
        }

        // If token budget is specified, further limit
        if let Some(max_tok) = max_tokens {
            let mut token_count = 0usize;
            let mut kept = Vec::new();
            for item in &self.items {
                let est_tokens = item.content.len() / 4;
                if token_count + est_tokens > max_tok {
                    break;
                }
                token_count += est_tokens;
                kept.push(item.clone());
            }
            kept
        } else {
            self.items.clone()
        }
    }

    /// Get the current number of tracked items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Clear all tracked items.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl Default for ContextScorer {
    fn default() -> Self {
        Self::new(ContextScorerConfig::default())
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Extract file paths from a line of text.
fn extract_file_paths(line: &str) -> Vec<String> {
    let mut paths = Vec::new();
    // Common path patterns
    let patterns = [
        ".rs", ".py", ".js", ".ts", ".go", ".java", ".cpp", ".c", ".h",
        ".toml", ".yaml", ".yml", ".json", ".md", ".txt", ".css", ".html",
    ];

    for ext in &patterns {
        if let Some(idx) = line.find(ext) {
            let end = idx + ext.len();
            // Walk backwards to find the start of the path
            let before = &line[..idx];
            let start = before.rfind(|c: char| c.is_whitespace() || c == ':' || c == '"' || c == '\'')
                .map(|s| s + 1)
                .unwrap_or(0); // Path starts at the beginning of line
            let path = line[start..end].to_string();
            if path.contains('/') || path.contains('\\') || path.contains('.') {
                paths.push(path);
            }
        }
    }

    paths
}

/// Extract important lines (function/class definitions) from a file.
fn extract_important_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut important = Vec::new();
    let keywords = ["fn ", "pub fn ", "def ", "class ", "struct ", "trait ", "impl ", "interface "];

    for &line in lines {
        let trimmed = line.trim();
        for kw in &keywords {
            if trimmed.starts_with(kw) {
                important.push(line);
                break;
            }
        }
    }

    important
}

/// Truncate a string to a maximum number of characters, preserving whole words.
fn truncate_to_chars(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }

    let truncated = &s[..max_chars];
    if let Some(last_space) = truncated.rfind(' ') {
        format!("{}...", &s[..last_space])
    } else {
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_summarizer_file_small() {
        let summarizer = SmartSummarizer::default();
        let output = "line1\nline2\nline3";
        let summary = summarizer.summarize(output, "read_file");
        assert!(summary.summary.contains("line1"));
        assert_eq!(summary.original_lines, 3);
    }

    #[test]
    fn test_smart_summarizer_file_large() {
        let summarizer = SmartSummarizer::default();
        let lines: Vec<String> = (0..100).map(|i| format!("line {}", i)).collect();
        let output = lines.join("\n");
        let summary = summarizer.summarize(&output, "read_file");
        assert!(summary.compression_ratio < 1.0);
        assert!(summary.summary.contains("[100 lines total"));
    }

    #[test]
    fn test_smart_summarizer_extracts_errors() {
        let summarizer = SmartSummarizer::default();
        let output = "compiling...\nerror: cannot find value\nmore output\nthread panicked";
        let summary = summarizer.summarize(output, "execute_command");
        assert!(!summary.errors.is_empty());
        assert!(summary.errors.iter().any(|e| e.contains("error")));
    }

    #[test]
    fn test_attachment_trimmer_small_file() {
        let trimmer = AttachmentTrimmer::default();
        let content = "line1\nline2\nline3";
        let result = trimmer.trim(content, &[], None);
        assert_eq!(result.original_lines, 3);
        assert_eq!(result.trimmed_lines, 3);
        assert_eq!(result.compression_ratio, 1.0);
    }

    #[test]
    fn test_attachment_trimmer_keeps_imports() {
        let trimmer = AttachmentTrimmer::new(AttachmentTrimConfig {
            max_lines: 200,
            context_lines: 3,
            keep_imports: true,
            min_lines_to_trim: 5, // low threshold for testing
        });

        let lines: Vec<String> = (0..20).map(|i| {
            if i == 0 { "import std::io".into() }
            else if i == 5 { "fn main() {".into() }
            else { format!("    // line {}", i) }
        }).collect();
        let content = lines.join("\n");

        let result = trimmer.trim(&content, &[], None);
        assert!(result.content.contains("import std::io"));
        assert!(result.content.contains("fn main()"));
    }

    #[test]
    fn test_context_scorer_ranking() {
        let mut scorer = ContextScorer::default();
        scorer.add("This is about Rust programming".into(), "file1.rs".into());
        scorer.add("Python data science tutorial".into(), "file2.py".into());
        scorer.add("Rust async runtime guide".into(), "file3.rs".into());

        let ranked = scorer.rank("rust async", None);
        assert!(!ranked.is_empty());
        // file3.rs should rank highest (contains both "rust" and "async")
        assert_eq!(ranked[0].source, "file3.rs");
    }

    #[test]
    fn test_ansi_stripping() {
        let input = "\x1b[32mgreen text\x1b[0m normal";
        let stripped = SmartSummarizer::strip_ansi_codes(input);
        assert_eq!(stripped, "green text normal");
    }

    #[test]
    fn test_extract_file_paths() {
        let paths = extract_file_paths("src/main.rs:42: error here");
        assert!(paths.iter().any(|p| p.contains("main.rs")), "paths: {:?}", paths);
    }

    #[test]
    fn test_truncate_to_chars() {
        let s = "hello world this is a test";
        let t = truncate_to_chars(s, 12);
        assert_eq!(t, "hello world...");
    }
}