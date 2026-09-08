//! Multi-Agent PR Review system.
//!
//! Parallel code review across 5 dimensions with automated judging.
//! Inspired by Cursor's multi-agent judging and Claude Code's PR review.
//!
//! ## Architecture
//!
//! ```text
//! PrReviewer::review_diff()
//!   ├── parse_git_diff()        — Extract changed files from diff text
//!   ├── run_security_review()   — SecurityScannerV2 on each changed file
//!   ├── run_performance_review()— Swarm agent for performance analysis
//!   ├── run_correctness_review()— Swarm agent for logic correctness
//!   ├── run_style_review()      — Swarm agent for code style
//!   └── run_tests_review()      — Swarm agent for test coverage
//!       │
//!       ▼
//!     judge()                    — Merge findings → final verdict
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use serde::{Deserialize, Serialize};

use crate::tools::security_scanner_v2::{
    SecurityScannerV2, SecurityFindingV2, VulnerabilitySeverity,
    Confidence,
};

// ============================================================================
// Enums
// ============================================================================

/// Review dimension / aspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReviewAspect {
    /// Security vulnerabilities (SQLi, XSS, auth issues, etc.)
    Security,
    /// Performance bottlenecks (N+1 queries, O(n²), memory leaks)
    Performance,
    /// Logic correctness (off-by-one, race conditions, edge cases)
    Correctness,
    /// Code style & conventions (naming, formatting, idioms)
    Style,
    /// Test coverage & quality (missing tests, flaky assertions)
    Tests,
}

impl std::fmt::Display for ReviewAspect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewAspect::Security => write!(f, "Security"),
            ReviewAspect::Performance => write!(f, "Performance"),
            ReviewAspect::Correctness => write!(f, "Correctness"),
            ReviewAspect::Style => write!(f, "Style"),
            ReviewAspect::Tests => write!(f, "Tests"),
        }
    }
}

impl ReviewAspect {
    /// All review aspects in canonical order.
    pub fn all() -> &'static [ReviewAspect] {
        &[
            ReviewAspect::Security,
            ReviewAspect::Performance,
            ReviewAspect::Correctness,
            ReviewAspect::Style,
            ReviewAspect::Tests,
        ]
    }

    /// Human-readable description of this review dimension.
    pub fn description(&self) -> &'static str {
        match self {
            ReviewAspect::Security => {
                "Detect security vulnerabilities: injection, auth flaws, \
                 crypto weaknesses, unsafe deserialization"
            }
            ReviewAspect::Performance => {
                "Identify performance anti-patterns: N+1 queries, \
                 unnecessary allocations, missing indexes, algorithmic complexity"
            }
            ReviewAspect::Correctness => {
                "Check logical correctness: edge cases, error handling, \
                 race conditions, off-by-one errors"
            }
            ReviewAspect::Style => {
                "Review code style: naming conventions, formatting, \
                 idiom usage, consistency with project standards"
            }
            ReviewAspect::Tests => {
                "Evaluate test coverage: missing test cases, assertion quality, \
                 boundary testing, integration gaps"
            }
        }
    }

    /// The emoji icon for this aspect (used in markdown output).
    pub fn icon(&self) -> &'static str {
        match self {
            ReviewAspect::Security => "🔒",
            ReviewAspect::Performance => "⚡",
            ReviewAspect::Correctness => "✅",
            ReviewAspect::Style => "🎨",
            ReviewAspect::Tests => "🧪",
        }
    }
}

/// Severity level for a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl std::fmt::Display for FindingSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FindingSeverity::Critical => write!(f, "CRITICAL"),
            FindingSeverity::High => write!(f, "HIGH"),
            FindingSeverity::Medium => write!(f, "MEDIUM"),
            FindingSeverity::Low => write!(f, "LOW"),
            FindingSeverity::Info => write!(f, "INFO"),
        }
    }
}

impl FindingSeverity {
    /// Numeric weight for severity sorting / scoring.
    pub fn weight(&self) -> u32 {
        match self {
            FindingSeverity::Critical => 5,
            FindingSeverity::High => 4,
            FindingSeverity::Medium => 3,
            FindingSeverity::Low => 2,
            FindingSeverity::Info => 1,
        }
    }

    /// Convert from `VulnerabilitySeverity` used by `SecurityScannerV2`.
    fn from_vulnerability_severity(v: VulnerabilitySeverity) -> Self {
        match v {
            VulnerabilitySeverity::Critical => FindingSeverity::Critical,
            VulnerabilitySeverity::High => FindingSeverity::High,
            VulnerabilitySeverity::Medium => FindingSeverity::Medium,
            VulnerabilitySeverity::Low => FindingSeverity::Low,
            VulnerabilitySeverity::Info => FindingSeverity::Info,
        }
    }

    /// ANSI color tag for terminal output.
    fn color_tag(&self) -> &'static str {
        match self {
            FindingSeverity::Critical => "\x1b[31m", // red
            FindingSeverity::High => "\x1b[91m",     // bright red
            FindingSeverity::Medium => "\x1b[33m",   // yellow
            FindingSeverity::Low => "\x1b[36m",      // cyan
            FindingSeverity::Info => "\x1b[90m",     // gray
        }
    }
}

/// Final verdict after multi-agent judging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReviewVerdict {
    /// No blocking or high-severity findings — safe to merge.
    #[default]
    Approved,
    /// Issues found that should be addressed before merging.
    NeedsChanges,
    /// Critical security or correctness blockers — must not merge.
    Blocked,
}

impl std::fmt::Display for ReviewVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewVerdict::Approved => write!(f, "✅ APPROVED"),
            ReviewVerdict::NeedsChanges => write!(f, "⚠️  NEEDS CHANGES"),
            ReviewVerdict::Blocked => write!(f, "🚫 BLOCKED"),
        }
    }
}

// ============================================================================
// Structs
// ============================================================================

/// A single finding from one review aspect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReviewResult {
    /// Which review dimension produced this finding.
    pub aspect: ReviewAspect,
    /// File path relative to repo root.
    pub file_path: String,
    /// Start line of the affected region (0-based).
    pub line_start: Option<u32>,
    /// End line of the affected region (0-based).
    pub line_end: Option<u32>,
    /// How severe is this issue.
    pub severity: FindingSeverity,
    /// Short title (one-liner).
    pub title: String,
    /// Detailed description of the issue.
    pub description: String,
    /// Suggested fix or remediation.
    pub suggestion: String,
    /// Confidence score 0.0 – 1.0.
    pub confidence: f32,
}

impl PrReviewResult {
    /// Create a new finding with sensible defaults.
    pub fn new(
        aspect: ReviewAspect,
        file_path: impl Into<String>,
        severity: FindingSeverity,
        title: impl Into<String>,
        description: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            aspect,
            file_path: file_path.into(),
            line_start: None,
            line_end: None,
            severity,
            title: title.into(),
            description: description.into(),
            suggestion: suggestion.into(),
            confidence: 0.8,
        }
    }

    /// Builder-style setter for line range.
    pub fn with_lines(mut self, start: u32, end: u32) -> Self {
        self.line_start = Some(start);
        self.line_end = Some(end);
        self
    }

    /// Builder-style setter for confidence.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Convert a `SecurityFindingV2` into a `PrReviewResult`.
    fn from_security_finding(finding: &SecurityFindingV2) -> Self {
        let confidence = match finding.confidence {
            Confidence::High => 0.95,
            Confidence::Medium => 0.75,
            Confidence::Low => 0.50,
        };
        Self {
            aspect: ReviewAspect::Security,
            file_path: finding.file.clone(),
            line_start: Some(finding.line as u32 - 1),
            line_end: Some(finding.line as u32),
            severity: FindingSeverity::from_vulnerability_severity(finding.severity),
            title: finding.title.clone(),
            description: finding.description.clone(),
            suggestion: finding.remediation.clone(),
            confidence,
        }
    }
}

/// Aggregated review report after all aspects have been analyzed.
#[derive(Debug, Clone, Serialize, Default)]
pub struct PrReviewReport {
    /// Final judged verdict.
    pub verdict: ReviewVerdict,
    /// Total number of findings across all aspects.
    pub total_findings: usize,
    /// Count of CRITICAL findings.
    pub critical_count: usize,
    /// Count of HIGH findings.
    pub high_count: usize,
    /// All individual findings, sorted by severity desc then aspect.
    pub findings: Vec<PrReviewResult>,
    /// Human-readable summary paragraph.
    pub summary: String,
    /// Wall-clock duration of the entire review in milliseconds.
    pub duration_ms: u64,
    /// Findings count grouped by aspect.
    pub by_aspect: HashMap<ReviewAspect, usize>,
}

// ============================================================================
// ReviewReport (for large diff chunked analysis)
// ============================================================================

/// Simplified review report used for chunked large-diff analysis.
/// Each chunk produces one ReviewReport; they are merged into a final
/// `PrReviewReport` via `merge_reports`.
#[derive(Debug, Clone, Default)]
pub struct ReviewReport {
    /// Overall score 0.0–100.0
    pub score: f64,
    /// All issues found
    pub issues: Vec<String>,
    /// Security issues
    pub security_issues: Vec<String>,
    /// Performance issues
    pub performance_issues: Vec<String>,
    /// Style issues
    pub style_issues: Vec<String>,
    /// Correctness issues
    pub correctness_issues: Vec<String>,
    /// Test issues
    pub test_issues: Vec<String>,
}

// ============================================================================
// PrReviewer
// ============================================================================

/// Multi-Agent PR Review engine.
///
/// Runs parallel review agents across 5 dimensions:
/// **Security**, **Performance**, **Correctness**, **Style**, **Tests**.
///
/// For the **Security** aspect, delegates to [`SecurityScannerV2`] for real
/// pattern-based vulnerability detection.  The other four aspects generate
/// structured prompts intended for swarm sub-agents; when no LLM provider
/// is available they produce architecture-demonstrating placeholder results.
///
/// # Example
///
/// ```ignore
/// let reviewer = PrReviewer::new();
/// let report = reviewer.review_diff(diff_text, repo_root).await?;
/// println!("{}", PrReviewer::format_markdown(&report));
/// ```
pub struct PrReviewer;

impl Default for PrReviewer {
    fn default() -> Self {
        Self::new()
    }
}

impl PrReviewer {
    /// Create a new PR reviewer instance.
    pub fn new() -> Self {
        Self
    }

    // -----------------------------------------------------------------------
    // Main entry point
    // -----------------------------------------------------------------------

    /// Review a git diff string.
    ///
    /// 1. Parse the diff to extract changed filenames and hunks.
    /// 2. Run **Security** review via [`SecurityScannerV2`] on each changed file.
    /// 3. Run the other 4 aspects through structured prompts (swarm-ready).
    /// 4. Merge all findings and compute a final [`ReviewVerdict`] via [`Self::judge`].
    ///
    /// Returns an aggregated [`PrReviewReport`].
    pub async fn review_diff(
        &self,
        diff_text: &str,
        repo_root: &Path,
    ) -> anyhow::Result<PrReviewReport> {
        let start = Instant::now();

        // Step 1: parse diff → list of changed files
        let changed_files = Self::parse_git_diff(diff_text);
        if changed_files.is_empty() {
            return Ok(PrReviewReport {
                verdict: ReviewVerdict::Approved,
                total_findings: 0,
                critical_count: 0,
                high_count: 0,
                findings: vec![],
                summary: "No changed files detected in diff.".to_string(),
                duration_ms: start.elapsed().as_millis() as u64,
                by_aspect: HashMap::new(),
            });
        }

        tracing::info!(
            files = changed_files.len(),
            "PR Review: parsed {} changed files from diff",
            changed_files.len()
        );

        // Step 2–5: run each review aspect
        let mut all_findings: Vec<PrReviewResult> = Vec::new();
        let mut by_aspect: HashMap<ReviewAspect, usize> = HashMap::new();

        // --- Security: real scanner ---
        let security_findings = self.run_security_review(&changed_files, repo_root).await;
        by_aspect.insert(ReviewAspect::Security, security_findings.len());
        all_findings.extend(security_findings);

        // --- Performance, Correctness, Style, Tests: swarm prompts ---
        for &aspect in &[ReviewAspect::Performance, ReviewAspect::Correctness, ReviewAspect::Style, ReviewAspect::Tests] {
            let findings = self.run_swarm_aspect(aspect, &changed_files, diff_text).await;
            by_aspect.insert(aspect, findings.len());
            all_findings.extend(findings);
        }

        // Sort: severity descending, then by aspect order
        all_findings.sort_by(|a, b| {
            b.severity.cmp(&a.severity)
                .then_with(|| {
                    let order_a = ReviewAspect::all().iter().position(|&x| x == a.aspect).unwrap_or(999);
                    let order_b = ReviewAspect::all().iter().position(|&x| x == b.aspect).unwrap_or(999);
                    order_a.cmp(&order_b)
                })
        });

        // Step 6: judge
        let verdict = Self::judge(&all_findings);

        // Counts
        let critical_count = all_findings.iter().filter(|f| f.severity == FindingSeverity::Critical).count();
        let high_count = all_findings.iter().filter(|f| f.severity == FindingSeverity::High).count();

        // Summary
        let summary = Self::build_summary(&verdict, &by_aspect, critical_count, high_count, all_findings.len());

        Ok(PrReviewReport {
            verdict,
            total_findings: all_findings.len(),
            critical_count,
            high_count,
            findings: all_findings,
            summary,
            duration_ms: start.elapsed().as_millis() as u64,
            by_aspect,
        })
    }

    // -----------------------------------------------------------------------
    // Security review (real SecurityScannerV2 integration)
    // -----------------------------------------------------------------------

    /// Run the **Security** review aspect using [`SecurityScannerV2`].
    ///
    /// Reads each changed file from disk and scans it for known vulnerability
    /// patterns.  This is the only aspect that produces *real* findings without
    /// requiring an LLM provider.
    async fn run_security_review(
        &self,
        changed_files: &[(String, String, String)],
        repo_root: &Path,
    ) -> Vec<PrReviewResult> {
        let scanner = SecurityScannerV2::new();

        // Build scan input: read actual file contents
        let mut scan_inputs: Vec<(String, String, String)> = Vec::new();
        for (file_path, _old_content, _new_content) in changed_files {
            let full_path = repo_root.join(file_path);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let language = Self::detect_language(file_path);
                scan_inputs.push((file_path.clone(), content, language));
            } else {
                tracing::warn!(path = %full_path.display(), "Cannot read file for security scan");
            }
        }

        if scan_inputs.is_empty() {
            return Vec::new();
        }

        // Use batch scan
        let report = scanner.scan_files(&scan_inputs);

        report
            .findings
            .iter()
            .map(PrReviewResult::from_security_finding)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Non-security aspects (swarm-agent prompts / placeholders)
    // -----------------------------------------------------------------------

    /// Run a non-security review aspect via structured prompt.
    ///
    /// In production this would dispatch to [`SwarmCoordinator`] agents.
    /// Without a running provider it returns placeholder results that
    /// demonstrate the architecture and prompt engineering.
    async fn run_swarm_aspect(
        &self,
        aspect: ReviewAspect,
        changed_files: &[(String, String, String)],
        diff_text: &str,
    ) -> Vec<PrReviewResult> {
        // Build the review prompt for this aspect
        let prompt = self.build_aspect_prompt(aspect, changed_files, diff_text);

        tracing::debug!(
            aspect = %aspect,
            prompt_len = prompt.len(),
            "Generated {} review prompt",
            aspect
        );

        // In production we would do:
        //
        //   let config = AgentConfig::default();
        //   let coordinator = SwarmCoordinator::new(4, config);
        //   coordinator.add_agent(aspect.to_string().to_lowercase().as_str(), "pr-review").await;
        //   let orchestrator = ProviderOrchestrator::new(config.provider_config());
        //   let result = coordinator.execute(&prompt, orchestrator).await;
        //   return parse_agent_output(result, aspect);
        //
        // For now, return lightweight heuristic-based placeholder results
        // that prove the pipeline works end-to-end.
        self.heuristic_placeholder(aspect, changed_files, diff_text)
    }

    /// Build the structured review prompt for a given aspect.
    fn build_aspect_prompt(
        &self,
        aspect: ReviewAspect,
        changed_files: &[(String, String, String)],
        diff_text: &str,
    ) -> String {
        let file_list: String = changed_files
            .iter()
            .map(|(path, _, _)| format!("  - {}", path))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"You are an expert code reviewer specializing in {aspect}.

## Task
Review the following code changes for {aspect_desc}.

## Files Changed
{file_list}

## Diff
```
{diff_text}
```

## Instructions
1. Analyze each changed file for {aspect}-related issues.
2. Report findings in this JSON format:
   {{
     "findings": [
       {{
         "file": "<file path>",
         "line_start": <number>,
         "line_end": <number>,
         "severity": "CRITICAL|HIGH|MEDIUM|LOW|INFO",
         "title": "<short title>",
         "description": "<detailed explanation>",
         "suggestion": "<how to fix>",
         "confidence": 0.0-1.0
       }}
     ]
   }}

Focus ONLY on {aspect} concerns. Do not report issues from other dimensions."#,
            aspect = aspect,
            aspect_desc = aspect.description(),
            file_list = file_list,
            diff_text = diff_text,
        )
    }

    /// Generate heuristic placeholder findings for non-security aspects.
    ///
    /// These are lightweight pattern-matching results that demonstrate
    /// the multi-aspect architecture without needing an LLM provider.
    fn heuristic_placeholder(
        &self,
        aspect: ReviewAspect,
        changed_files: &[(String, String, String)],
        _diff_text: &str,
    ) -> Vec<PrReviewResult> {
        let mut findings = Vec::new();

        for (file_path, _old_content, new_content) in changed_files {
            match aspect {
                ReviewAspect::Performance => {
                    // Detect common performance anti-patterns in added lines
                    let added_lines: Vec<&str> = new_content.lines()
                        .filter(|l| l.starts_with('+') || (!l.starts_with('-') && !l.starts_with('@') && !l.starts_with("diff") && !l.starts_with("index") && !l.starts_with("---") && !l.starts_with("+++")))
                        .collect();

                    for line in &added_lines {
                        let line_str = line.trim_start_matches('+').trim_start_matches(' ');
                        // Heuristic: nested loops
                        if line_str.contains(".iter()")
                            && (line_str.contains(".for_each(") || line_str.contains(".collect::<Vec")
                            || line_str.contains(".map("))
                        {
                            findings.push(PrReviewResult::new(
                                ReviewAspect::Performance,
                                file_path,
                                FindingSeverity::Medium,
                                "Potential iterator overhead in hot path",
                                "Chained iterators with collection may allocate unnecessarily.",
                                "Consider using lazy evaluation or pre-allocated buffers.",
                            ));
                            break; // one per file max
                        }
                        // Heuristic: string concatenation in loop context
                        if line_str.contains("format!(") && line_str.contains("loop") {
                            findings.push(PrReviewResult::new(
                                ReviewAspect::Performance,
                                file_path,
                                FindingSeverity::Medium,
                                "String formatting inside loop",
                                "Repeated format!() calls in a loop can cause heap allocation pressure.",
                                "Use a pre-allocated String::with_capacity() and push_str(), or build outside the loop.",
                            ));
                            break;
                        }
                    }
                }
                ReviewAspect::Correctness => {
                    // Check for unwrap/expect on user-facing paths
                    let added_lines: Vec<&str> = new_content.lines()
                        .filter(|l| l.starts_with('+'))
                        .collect();

                    for line in &added_lines {
                        let line_str = line.trim_start_matches('+').trim();
                        if (line_str.contains(".unwrap()")
                            || line_str.contains(".expect("))
                            && !line_str.contains("// ok:")
                            && !line_str.contains("#[allow")
                        {
                            findings.push(PrReviewResult::new(
                                ReviewAspect::Correctness,
                                file_path,
                                FindingSeverity::Medium,
                                "Potential panic via unwrap/expect",
                                "unwrap() or expect() on added code may panic at runtime if value is None/Err.",
                                "Prefer unwrap_or(), unwrap_or_else(), ok_or?, or the ? operator.",
                            ));
                            break;
                        }
                    }
                    // Check for TODO/FIXME/HACK comments in new code
                    for line in new_content.lines().filter(|l| l.starts_with('+')) {
                        let lower = line.to_lowercase();
                        if lower.contains("todo!") || lower.contains("unimplemented!") {
                            findings.push(PrReviewResult::new(
                                ReviewAspect::Correctness,
                                file_path,
                                FindingSeverity::High,
                                "Unimplemented code path in new code",
                                "Found todo!() or unimplemented!() macro in newly added code.",
                                "Implement the logic before merging, or replace with a proper stub/trait.",
                            ));
                            break;
                        }
                    }
                }
                ReviewAspect::Style => {
                    // Check line length in added lines
                    for line in new_content.lines().filter(|l| l.starts_with('+')) {
                        if line.len() > 100 {
                            findings.push(PrReviewResult::new(
                                ReviewAspect::Style,
                                file_path,
                                FindingSeverity::Low,
                                "Line exceeds recommended length",
                                format!("Line is {} characters long (recommended ≤ 100).", line.len()),
                                "Break the line at a logical point for readability.",
                            ));
                            break;
                        }
                    }
                    // Check for trailing whitespace in added lines
                    for (i, line) in new_content.lines().enumerate().filter(|(_, l)| l.starts_with('+')) {
                        if line.ends_with(' ') || line.ends_with('\t') {
                            findings.push(PrReviewResult::new(
                                ReviewAspect::Style,
                                file_path,
                                FindingSeverity::Info,
                                "Trailing whitespace on added line",
                                "Added line has trailing whitespace.",
                                "Remove trailing whitespace (most editors can trim automatically).",
                            ).with_lines(i as u32, i as u32));
                            break;
                        }
                    }
                }
                ReviewAspect::Tests => {
                    // Check if any test file was modified
                    let is_test_file = file_path.contains("test")
                        || file_path.contains("_test")
                        || file_path.contains(".spec.")
                        || file_path.contains("__tests__");

                    if is_test_file {
                        // Good — tests were touched, just note it
                        continue;
                    }

                    // Non-test source file changed: check if there's a corresponding test change
                    let base_name = file_path
                        .rsplit('.')
                        .next()
                        .unwrap_or("");
                    let has_test_change = changed_files.iter().any(|(p, _, _)| {
                        p != file_path
                            && (p.contains(base_name.replace(".", "_").as_str())
                                || p.contains("test"))
                    });

                    if !has_test_change {
                        findings.push(PrReviewResult::new(
                            ReviewAspect::Tests,
                            file_path,
                            FindingSeverity::Low,
                            "No corresponding test changes detected",
                            format!(
                                "{} was modified but no test file changes were found in this diff.",
                                file_path
                            ),
                            "Consider adding or updating unit/integration tests for the changed behavior.",
                        ));
                    }
                }
                ReviewAspect::Security => {
                    // Handled separately in run_security_review
                }
            }
        }

        findings
    }

    // -----------------------------------------------------------------------
    // Judge
    // -----------------------------------------------------------------------

    /// Merge all aspect results into a final verdict.
    ///
    /// # Rules
    ///
    /// | Condition                     | Verdict       |
    /// |-------------------------------|---------------|
    /// | Any **Critical** finding      | `Blocked`     |
    /// | Any **High** finding          | `NeedsChanges`|
    /// | Only Medium/Low/Info or empty | `Approved`    |
    pub fn judge(results: &[PrReviewResult]) -> ReviewVerdict {
        let has_critical = results.iter().any(|r| r.severity == FindingSeverity::Critical);
        let has_high = results.iter().any(|r| r.severity == FindingSeverity::High);

        if has_critical {
            ReviewVerdict::Blocked
        } else if has_high {
            ReviewVerdict::NeedsChanges
        } else {
            ReviewVerdict::Approved
        }
    }

    // -----------------------------------------------------------------------
    // Diff parsing
    // -----------------------------------------------------------------------

    /// Parse a unified git diff into `(file_path, old_content, new_content)` tuples.
    ///
    /// Extracts `+++ b/path/to/file` headers and groups subsequent `@@` hunks.
    /// Does **not** need full hunk parsing — it extracts filenames and captures
    /// the raw diff text per file so downstream reviewers have full context.
    fn parse_git_diff(diff_text: &str) -> Vec<(String, String, String)> {
        let mut files = Vec::new();
        let mut current_file: Option<String> = None;
        let mut current_old = String::new();
        let mut current_new = String::new();

        for line in diff_text.lines() {
            if let Some(rest) = line.strip_prefix("+++ b/") {
                // Flush previous file
                if let Some(ref _path) = current_file {
                    if !current_new.is_empty() {
                        files.push((current_file.take().expect("option empty: pr_reviewer.rs:766"), current_old.clone(), current_new.clone()));
                    }
                    current_old.clear();
                    current_new.clear();
                }
                // New file
                let path = rest.trim().to_string();
                // Skip binary/deleted files
                if path == "/dev/null" {
                    current_file = None;
                    continue;
                }
                current_file = Some(path);
            } else if current_file.is_some() {
                // Accumulate hunk content
                current_old.push_str(line);
                current_old.push('\n');
                current_new.push_str(line);
                current_new.push('\n');
            }
        }

        // Flush last file
        if let Some(path) = current_file.take() {
            if !current_new.is_empty() {
                files.push((path, current_old, current_new));
            }
        }

        files
    }

    // -----------------------------------------------------------------------
    // Markdown formatting
    // -----------------------------------------------------------------------

    /// Format the review report as a Markdown table suitable for CLI/IDE display.
    ///
    /// Output style matches the box-drawing tables used in `swe_runner.rs`.
    pub fn format_markdown(report: &PrReviewReport) -> String {
        let mut out = String::with_capacity(4096);

        // Header box
        out.push_str("╔══════════════════════════════════════════════════════════════════╗\n");
        out.push_str("║                   📋 PR REVIEW REPORT                           ║\n");
        out.push_str("╠══════════════════════════════════════════════════════════════════╣\n");
        out.push_str(&format!("║  Verdict: {:<54}║\n", report.verdict));
        out.push_str(&format!("║  Total Findings: {:<46}║\n", report.total_findings));
        out.push_str(&format!(
            "║  Severity: CRITICAL={}  HIGH={}  MEDIUM={}  LOW={}  INFO={:<10}║\n",
            report.critical_count,
            report.high_count,
            report.by_aspect.get(&ReviewAspect::Security).copied().unwrap_or(0)
                + report.by_aspect.get(&ReviewAspect::Performance).copied().unwrap_or(0)
                + report.by_aspect.get(&ReviewAspect::Correctness).copied().unwrap_or(0)
                + report.by_aspect.get(&ReviewAspect::Style).copied().unwrap_or(0)
                - report.critical_count
                - report.high_count,
            report.by_aspect.get(&ReviewAspect::Tests).copied().unwrap_or(0),
            ""
        ));

        // Simpler severity breakdown
        let medium = report.findings.iter().filter(|f| f.severity == FindingSeverity::Medium).count();
        let low = report.findings.iter().filter(|f| f.severity == FindingSeverity::Low).count();
        let info = report.findings.iter().filter(|f| f.severity == FindingSeverity::Info).count();
        out.push_str(&format!(
            "║           [{}C] [{}H] [{}M] [{}L] [{}I]                       ║\n",
            report.critical_count, report.high_count, medium, low, info
        ));
        out.push_str(&format!("║  Duration: {}ms{:>45}║\n", report.duration_ms, ""));
        out.push_str("╠══════════════════════════════════════════════════════════════════╣\n");

        // By-aspect breakdown
        out.push_str("║  FINDINGS BY ASPECT                                          ║\n");
        out.push_str("╠══════════════════════════════════════════════════════════════════╣\n");
        for aspect in ReviewAspect::all() {
            let count = report.by_aspect.get(aspect).copied().unwrap_or(0);
            out.push_str(&format!(
                "║  {} {}: {:>4} finding(s){:>36}║\n",
                aspect.icon(),
                aspect,
                count,
                "",
            ));
        }
        out.push_str("╚══════════════════════════════════════════════════════════════════╝\n");

        // Detailed findings table
        if !report.findings.is_empty() {
            out.push_str("\n┌─────────────────────────────────────────────────────────────────────┐\n");
            out.push_str("│  DETAILED FINDINGS                                                  │\n");
            out.push_str("├──────┬──────────────┬──────────┬───────┬──────────────────────────────┤\n");
            out.push_str("│ SEV  │ ASPECT       │ FILE     │ LINE  │ TITLE                        │\n");
            out.push_str("├──────┼──────────────┼──────────┼───────┼──────────────────────────────┤\n");

            for finding in &report.findings {
                let sev_str = format!("{}", finding.severity);
                let line_str = finding
                    .line_start
                    .map(|l| format!("{}", l + 1)) // display as 1-based
                    .unwrap_or_else(|| "-".to_string());

                // Truncate fields for table alignment
                let file_display = truncate_str(&finding.file_path, 14);
                let title_display = truncate_str(&finding.title, 30);

                out.push_str(&format!(
                    "│ {:<4} │ {:<12} │ {:<8} │ {:>5} │ {:<28} │\n",
                    sev_str,
                    finding.aspect,
                    file_display,
                    line_str,
                    title_display,
                ));

                // Description row (indented)
                let desc_lines = wrap_text(&finding.description, 64);
                for desc_line in desc_lines {
                    out.push_str(&format!(
                        "│      │              │          │       │ {:<28} │\n",
                        truncate_str(desc_line, 28),
                    ));
                }

                // Suggestion row
                let suggestion_text = format!("💡 {}", &finding.suggestion);
                let sug_lines = wrap_text(&suggestion_text, 64);
                for sug_line in sug_lines {
                    out.push_str(&format!(
                        "│      │              │          │       │ {:<28} │\n",
                        truncate_str(sug_line, 28),
                    ));
                }

                out.push_str("├──────┼──────────────┼──────────┼───────┼──────────────────────────────┤\n");
            }

            out.push_str("└──────────────┴──────────────┴──────────┴───────┴──────────────────────────────┘\n");
        }

        // Summary
        out.push('\n');
        out.push_str(&format!("{}\n\n", report.summary));

        out
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Build a human-readable summary paragraph for the report.
    fn build_summary(
        verdict: &ReviewVerdict,
        by_aspect: &HashMap<ReviewAspect, usize>,
        critical_count: usize,
        high_count: usize,
        total: usize,
    ) -> String {
        let mut parts = Vec::new();

        match verdict {
            ReviewVerdict::Approved => {
                parts.push("PR review completed successfully.".to_string());
                if total == 0 {
                    parts.push("No issues found — looks good to merge!".to_string());
                } else {
                    parts.push(format!(
                        "Only minor suggestions found ({} finding(s)). Safe to merge after optional cleanup.",
                        total
                    ));
                }
            }
            ReviewVerdict::NeedsChanges => {
                parts.push(format!(
                    "PR review identified {} issue(s) requiring attention before merge.",
                    total
                ));
                if high_count > 0 {
                    parts.push(format!("{} high-severity item(s) should be addressed.", high_count));
                }
            }
            ReviewVerdict::Blocked => {
                parts.push(format!(
                    "🚫 PR BLOCKED: {} critical finding(s) detected. Must not merge until resolved.",
                    critical_count
                ));
            }
        }

        // Aspect breakdown sentence
        let active_aspects: Vec<String> = by_aspect
            .iter()
            .filter(|(_, &count)| count > 0)
            .map(|(aspect, &count)| format!("{}({})", aspect, count))
            .collect();
        if !active_aspects.is_empty() {
            parts.push(format!(
                "Breakdown: {}.",
                active_aspects.join(", ")
            ));
        }

        parts.join(" ")
    }

    /// Detect programming language from file extension.
    fn detect_language(file_path: &str) -> String {
        match file_path.rsplit('.').next().unwrap_or("") {
            "rs" => "rust".to_string(),
            "py" | "pyi" => "python".to_string(),
            "js" | "jsx" | "mjs" => "javascript".to_string(),
            "ts" | "tsx" => "typescript".to_string(),
            "go" => "go".to_string(),
            "java" => "java".to_string(),
            "c" => "c".to_string(),
            "cpp" | "cc" | "cxx" => "cpp".to_string(),
            "rb" => "ruby".to_string(),
            "php" => "php".to_string(),
            "cs" => "csharp".to_string(),
            "swift" => "swift".to_string(),
            "kt" | "kts" => "kotlin".to_string(),
            "scala" => "scala".to_string(),
            "ex" | "exs" => "elixir".to_string(),
            _ => "rust".to_string(), // default fallback
        }
    }

    // ── Large diff chunked analysis ──────────────────────────────────────

    /// Analyze a large diff by splitting into chunks.
    /// Each chunk is analyzed independently, then results are merged.
    pub async fn review_large_diff(&self, diff: &str, max_chunk_size: usize) -> ReviewReport {
        let chunks = self.split_diff(diff, max_chunk_size);
        let mut merged = ReviewReport::default();

        for chunk in &chunks {
            // For each chunk, run heuristic placeholder analysis
            let changed_files = Self::parse_git_diff(chunk);
            let report = self.analyze_chunk(&changed_files, chunk);
            merged = self.merge_reports(merged, report);
        }

        merged
    }

    /// Split a diff into independent chunks by file boundaries.
    fn split_diff(&self, diff: &str, max_chunk_size: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current = String::new();

        for line in diff.lines() {
            // File boundary: "diff --git a/... b/..."
            if line.starts_with("diff --git") && current.len() > max_chunk_size && !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            current.push_str(line);
            current.push('\n');
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        chunks
    }

    /// Merge two review reports.
    fn merge_reports(&self, a: ReviewReport, b: ReviewReport) -> ReviewReport {
        ReviewReport {
            score: (a.score + b.score) / 2.0,
            issues: [a.issues, b.issues].concat(),
            security_issues: [a.security_issues, b.security_issues].concat(),
            performance_issues: [a.performance_issues, b.performance_issues].concat(),
            style_issues: [a.style_issues, b.style_issues].concat(),
            correctness_issues: [a.correctness_issues, b.correctness_issues].concat(),
            test_issues: [a.test_issues, b.test_issues].concat(),
        }
    }

    /// Analyze a single chunk using heuristic placeholders.
    fn analyze_chunk(&self, changed_files: &[(String, String, String)], _diff_text: &str) -> ReviewReport {
        let mut report = ReviewReport::default();

        for (file_path, _old_content, new_content) in changed_files {
            let added_lines: Vec<&str> = new_content.lines()
                .filter(|l| l.starts_with('+'))
                .collect();

            // Security check: hardcoded secrets
            for line in &added_lines {
                let line_str = line.trim_start_matches('+').trim();
                if line_str.contains("password") || line_str.contains("secret") || line_str.contains("api_key") {
                    report.security_issues.push(format!("{}: Potential secret in code", file_path));
                    report.issues.push(format!("{}: Security issue found", file_path));
                }
            }

            // Performance check: nested loops / iterator chains
            for line in &added_lines {
                let line_str = line.trim_start_matches('+').trim();
                if line_str.contains(".iter()") && (line_str.contains(".for_each(") || line_str.contains(".map(")) {
                    report.performance_issues.push(format!("{}: Potential iterator overhead", file_path));
                    report.issues.push(format!("{}: Performance issue found", file_path));
                }
            }

            // Style check: line length
            for line in &added_lines {
                if line.len() > 100 {
                    report.style_issues.push(format!("{}: Line exceeds 100 chars", file_path));
                    report.issues.push(format!("{}: Style issue found", file_path));
                    break;
                }
            }

            // Correctness check: unwrap/expect
            for line in &added_lines {
                let line_str = line.trim_start_matches('+').trim();
                if (line_str.contains(".unwrap()") || line_str.contains(".expect("))
                    && !line_str.contains("// ok:")
                {
                    report.correctness_issues.push(format!("{}: Potential panic via unwrap/expect", file_path));
                    report.issues.push(format!("{}: Correctness issue found", file_path));
                    break;
                }
            }
        }

        // Compute score based on issues found
        let total_issues = report.issues.len();
        report.score = if total_issues == 0 {
            100.0
        } else {
            (100.0 - (total_issues as f64) * 15.0).max(0.0)
        };

        report
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Truncate a string to `max_len`, appending "…" if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_len.saturating_sub(1).max(1)).collect::<String>())
    }
}

/// Wrap text into lines of at most `width` chars.
fn wrap_text(text: &str, width: usize) -> Vec<&str> {
    if text.len() <= width {
        if text.is_empty() { Vec::new() } else { vec![text] }
    } else {
        // Simple greedy wrapping
        let mut result = Vec::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            if remaining.len() <= width {
                result.push(remaining);
                break;
            }
            // Find last space within width
            let split_point = remaining[..width]
                .rmatch_indices(' ')
                .next()
                .map(|(i, _)| i)
                .unwrap_or(width);
            result.push(&remaining[..split_point]);
            remaining = remaining[split_point.min(remaining.len() - 1)..].trim_start_matches(' ');
        }
        result
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = r#"diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,6 +1,10 @@
 fn main() {
-    println!("hello");
+    println!("hello world");
+    let password = "admin123";
+    let result = some_value.unwrap();
+    let formatted = format!("user: {}", name);  // in loop context
+    todo!("implement later");
 }
 
 diff --git a/tests/test_main.rs b/tests/test_main.rs
 index 1111111..2222222 100644
 --- a/tests/test_main.rs
 +++ b/tests/test_main.rs
 @@ -0,0 +1,5 @@
 +#[cfg(test)]
 +mod tests {
 +    #[test]
 +    fn it_works() {
 +        assert_eq!(2 + 2, 4);
 +    }
 +}
 "#;

    #[tokio::test]
    async fn test_parse_git_diff() {
        let files = PrReviewer::parse_git_diff(SAMPLE_DIFF);
        assert_eq!(files.len(), 2, "Should find 2 changed files");
        assert!(files[0].0.contains("main.rs"));
        assert!(files[1].0.contains("test_main.rs"));
    }

    #[test]
    fn test_judge_approved() {
        let results = vec![
            PrReviewResult::new(ReviewAspect::Style, "foo.rs", FindingSeverity::Low, "Minor style", "desc", "fix"),
            PrReviewResult::new(ReviewAspect::Tests, "foo.rs", FindingSeverity::Info, "Info", "desc", "fix"),
        ];
        assert_eq!(PrReviewer::judge(&results), ReviewVerdict::Approved);
    }

    #[test]
    fn test_judge_needs_changes() {
        let results = vec![
            PrReviewResult::new(ReviewAspect::Security, "foo.rs", FindingSeverity::High, "SQLi", "desc", "fix"),
        ];
        assert_eq!(PrReviewer::judge(&results), ReviewVerdict::NeedsChanges);
    }

    #[test]
    fn test_judge_blocked() {
        let results = vec![
            PrReviewResult::new(ReviewAspect::Security, "foo.rs", FindingSeverity::Critical, "Cmd Injection", "desc", "fix"),
        ];
        assert_eq!(PrReviewer::judge(&results), ReviewVerdict::Blocked);
    }

    #[test]
    fn test_judge_empty() {
        assert_eq!(PrReviewer::judge(&[]), ReviewVerdict::Approved);
    }

    #[test]
    fn test_review_aspect_display() {
        assert_eq!(ReviewAspect::Security.to_string(), "Security");
        assert_eq!(ReviewAspect::Performance.to_string(), "Performance");
        assert_eq!(ReviewAspect::Correctness.to_string(), "Correctness");
        assert_eq!(ReviewAspect::Style.to_string(), "Style");
        assert_eq!(ReviewAspect::Tests.to_string(), "Tests");
    }

    #[test]
    fn test_finding_severity_ordering() {
        assert!(FindingSeverity::Critical > FindingSeverity::High);
        assert!(FindingSeverity::High > FindingSeverity::Medium);
        assert!(FindingSeverity::Medium > FindingSeverity::Low);
        assert!(FindingSeverity::Low > FindingSeverity::Info);
    }

    #[test]
    fn test_pr_review_result_builder() {
        let result = PrReviewResult::new(
            ReviewAspect::Security,
            "src/auth.rs",
            FindingSeverity::Critical,
            "Hardcoded secret",
            "Password in plaintext",
            "Use env var",
        )
        .with_lines(42, 43)
        .with_confidence(0.95);

        assert_eq!(result.aspect, ReviewAspect::Security);
        assert_eq!(result.line_start, Some(42));
        assert_eq!(result.line_end, Some(43));
        assert!((result.confidence - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(PrReviewer::detect_language("src/main.rs"), "rust");
        assert_eq!(PrReviewer::detect_language("app.py"), "python");
        assert_eq!(PrReviewer::detect_language("index.ts"), "typescript");
        assert_eq!(PrReviewer::detect_language("server.go"), "go");
        assert_eq!(PrReviewer::detect_language("unknown.xyz"), "rust"); // default
    }

    #[tokio::test]
    async fn test_review_diff_empty() {
        let reviewer = PrReviewer::new();
        let report = reviewer.review_diff("", Path::new("/fake")).await.unwrap();
        assert_eq!(report.verdict, ReviewVerdict::Approved);
        assert_eq!(report.total_findings, 0);
    }

    #[tokio::test]
    async fn test_format_markdown_no_findings() {
        let report = PrReviewReport {
            verdict: ReviewVerdict::Approved,
            total_findings: 0,
            critical_count: 0,
            high_count: 0,
            findings: vec![],
            summary: "All clear!".to_string(),
            duration_ms: 12,
            by_aspect: HashMap::new(),
        };
        let md = PrReviewer::format_markdown(&report);
        assert!(md.contains("PR REVIEW REPORT"));
        assert!(md.contains("APPROVED"));
        assert!(md.contains("All clear!"));
    }

    #[tokio::test]
    async fn test_format_markdown_with_findings() {
        let mut by_aspect = HashMap::new();
        by_aspect.insert(ReviewAspect::Security, 1);
        let report = PrReviewReport {
            verdict: ReviewVerdict::NeedsChanges,
            total_findings: 1,
            critical_count: 0,
            high_count: 1,
            findings: vec![PrReviewResult::new(
                ReviewAspect::Security,
                "src/auth.rs",
                FindingSeverity::High,
                "SQL Injection",
                "Concatenated query",
                "Use prepared statements",
            ).with_lines(10, 12).with_confidence(0.9)],
            summary: "Issues found.".to_string(),
            duration_ms: 42,
            by_aspect,
        };
        let md = PrReviewer::format_markdown(&report);
        assert!(md.contains("NEEDS CHANGES"));
        assert!(md.contains("SQL Injection"));
        assert!(md.contains("Security"));
        assert!(md.contains("auth.rs"));
    }

    #[test]
    fn test_build_aspect_prompt() {
        let reviewer = PrReviewer::new();
        let files = [("src/foo.rs".into(), "".into(), "".into())];
        let prompt = reviewer.build_aspect_prompt(ReviewAspect::Performance, &files, "+let x = 1;");
        assert!(prompt.contains("Performance"));
        assert!(prompt.contains("src/foo.rs"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hell…");
    }

    #[test]
    fn test_wrap_text() {
        let short = "short";
        assert_eq!(wrap_text(short, 20), vec!["short"]);
        let empty: Vec<&str> = Vec::new();
        assert_eq!(wrap_text("", 20), empty);
    }

    // ── Large diff chunked analysis tests ─────────────────────────────────

    #[test]
    fn test_split_diff_small() {
        let reviewer = PrReviewer::new();
        let small_diff = "diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-foo\n+bar\n";
        let chunks = reviewer.split_diff(small_diff, 100);
        assert_eq!(chunks.len(), 1, "Small diff stays as one chunk");
    }

    #[test]
    fn test_split_diff_large() {
        let reviewer = PrReviewer::new();
        // Simulate a diff with two files where each file's diff exceeds max_chunk_size
        let large_diff = format!(
            "diff --git a/a.rs b/a.rs\n{}\ndiff --git b/b.rs b/b.rs\n{}\n",
            "a\n".repeat(50),
            "b\n".repeat(50)
        );
        let chunks = reviewer.split_diff(&large_diff, 30);
        assert!(chunks.len() >= 2, "Large diff should be split, got {} chunks", chunks.len());
    }

    #[test]
    fn test_merge_reports_basic() {
        let reviewer = PrReviewer::new();
        let a = ReviewReport {
            score: 80.0,
            issues: vec!["issue1".to_string()],
            security_issues: vec!["sec1".to_string()],
            performance_issues: vec![],
            style_issues: vec![],
            correctness_issues: vec![],
            test_issues: vec![],
        };
        let b = ReviewReport {
            score: 60.0,
            issues: vec!["issue2".to_string()],
            security_issues: vec![],
            performance_issues: vec!["perf1".to_string()],
            style_issues: vec!["style1".to_string()],
            correctness_issues: vec![],
            test_issues: vec![],
        };
        let merged = reviewer.merge_reports(a, b);
        assert!((merged.score - 70.0).abs() < f64::EPSILON, "Score should be average: {}", merged.score);
        assert_eq!(merged.issues.len(), 2, "Should have both issues");
        assert_eq!(merged.security_issues.len(), 1, "Should have security issue");
        assert_eq!(merged.performance_issues.len(), 1, "Should have performance issue");
        assert_eq!(merged.style_issues.len(), 1, "Should have style issue");
    }

    #[tokio::test]
    async fn test_review_large_diff_empty() {
        let reviewer = PrReviewer::new();
        let report = reviewer.review_large_diff("", 100).await;
        assert!((report.score - 100.0).abs() < f64::EPSILON, "Empty diff should have perfect score");
        assert!(report.issues.is_empty(), "Empty diff should have no issues");
    }
}
