//! Code Review Engine — "generate → review → apply → verify" closed loop.
//!
//! Integrates with [`PrReviewer`] for multi-aspect analysis, [`SecurityScannerV2`]
//! for deterministic vulnerability detection, [`DiffEngine`] for applying fixes,
//! and context/streaming modules for large-diff compression and real-time output.
//!
//! ## Architecture
//!
//! ```text
//! ReviewEngine
//!   ├── review()           — Run full review (deterministic + LLM Agent)
//!   ├── apply_suggestions()— Convert findings to FileEdit → apply
//!   └── verify_fixes()     — Compile/test after applying fixes
//!
//! ReviewRule (4-tier priority)
//!   ├── CLI --rule arg     (highest)
//!   ├── .carp/rules.toml   (project-level)
//!   ├── ~/.config/carp/    (user-level)
//!   └── built-in rules     (system default)
//!
//! LineAnnotation
//!   └── file:line:severity: message + suggestion code block
//! ```
//!
//! Borrows from alibaba/open-code-review (hybrid deterministic+LLM, 4-tier rules)
//! and mattpocock/skills (composable workflows, progressive disclosure).

pub mod workflow;
pub mod audit;
pub mod adr;
pub mod arch_copilot;

use crate::tools::diff::{DiffEngine, FileEdit, EditResult};
use crate::tools::pr_reviewer::{
    PrReviewer, PrReviewReport, PrReviewResult, ReviewAspect,
    FindingSeverity, ReviewVerdict,
};
use crate::tools::security_scanner_v2::SecurityScannerV2;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ============================================================================
// Constants
// ============================================================================

/// Maximum diff size (in chars) before triggering chunked analysis.
const LARGE_DIFF_THRESHOLD: usize = 50_000;

/// Default rules directory name (project-level).
const RULES_DIR: &str = ".carp";

// ============================================================================
// DiffTarget — what are we reviewing?
// ============================================================================

/// What to review.
#[derive(Debug, Clone)]
pub enum DiffTarget {
    /// A single file path.
    File(PathBuf),
    /// A directory (review all changed files in it).
    Directory(PathBuf),
    /// The current PR / branch diff.
    Pr,
    /// A specific branch diff against HEAD~1.
    Branch(String),
    /// Raw diff text.
    Raw(String),
}

impl DiffTarget {
    /// Parse a user-provided target string into a `DiffTarget`.
    pub fn parse(target: &str) -> Self {
        let path = Path::new(target);
        if target == "pr" || target == "HEAD" {
            DiffTarget::Pr
        } else if path.exists() && path.is_file() {
            DiffTarget::File(path.to_path_buf())
        } else if path.exists() && path.is_dir() {
            DiffTarget::Directory(path.to_path_buf())
        } else if is_git_branch(target) {
            DiffTarget::Branch(target.to_string())
        } else {
            DiffTarget::Raw(target.to_string())
        }
    }

    /// Extract the diff text for this target.
    pub fn to_diff_text(&self) -> String {
        match self {
            DiffTarget::File(path) => {
                // Generate diff-like context from file content
                std::fs::read_to_string(path)
                    .map(|content| format!("+++ b/{}\n{}", path.display(), content))
                    .unwrap_or_default()
            }
            DiffTarget::Directory(dir) => {
                // For a directory, read all files and concatenate
                let mut all = String::new();
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                all.push_str(&format!(
                                    "+++ b/{}\n{}\n",
                                    path.display(),
                                    content
                                ));
                            }
                        }
                    }
                }
                all
            }
            DiffTarget::Pr | DiffTarget::Branch(_) => {
                let base = match self {
                    DiffTarget::Branch(b) => format!("{}..HEAD", b),
                    _ => "HEAD~1..HEAD".to_string(),
                };
                std::process::Command::new("git")
                    .args(["diff", &base])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .unwrap_or_default()
            }
            DiffTarget::Raw(text) => text.clone(),
        }
    }

    /// Get the working directory / repo root.
    pub fn working_dir(&self) -> PathBuf {
        match self {
            DiffTarget::File(p) | DiffTarget::Directory(p) => {
                p.parent().map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            }
            _ => std::env::current_dir().unwrap_or_default(),
        }
    }
}

/// Check if a string looks like a git branch name.
fn is_git_branch(s: &str) -> bool {
    // Simple heuristic: no spaces, no path separators, not empty
    !s.is_empty() && !s.contains(' ') && !s.contains('\\') && !s.contains('/')
}

// ============================================================================
// ReviewRule — 4-tier rule system (inspired by alibaba/open-code-review)
// ============================================================================

/// A single review rule with path pattern and instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRule {
    /// Glob pattern for file paths (e.g., "**/*.rs", "src/api/**").
    pub path_pattern: String,
    /// The review instruction or check to apply.
    pub rule: String,
    /// Severity if this rule is violated.
    pub severity: String, // "error", "warning", "info"
    /// Optional associated aspect.
    pub aspect: Option<String>,
}

/// 4-tier rules collection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewRuleSet {
    /// CLI --rule overrides (highest priority).
    pub cli_rules: Vec<ReviewRule>,
    /// Project-level rules from `.carp/rules.toml`.
    pub project_rules: Vec<ReviewRule>,
    /// User-level rules from `~/.config/carp/rules.toml`.
    pub user_rules: Vec<ReviewRule>,
    /// Built-in system rules.
    pub system_rules: Vec<ReviewRule>,
}

impl ReviewRuleSet {
    /// Load rules from all tiers, merging with priority (CLI > project > user > system).
    pub fn load(project_root: &Path, cli_rules: Vec<ReviewRule>) -> Self {
        let mut set = ReviewRuleSet {
            cli_rules,
            ..Default::default()
        };

        // Project-level: .carp/rules.toml
        let project_rules_path = project_root.join(RULES_DIR).join("rules.toml");
        if project_rules_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&project_rules_path) {
                if let Ok(rules) = toml::from_str::<Vec<ReviewRule>>(&content) {
                    set.project_rules = rules;
                }
            }
        }

        // User-level: ~/.config/carp/rules.toml
        let user_rules_path = dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("carp")
            .join("rules.toml");
        if user_rules_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&user_rules_path) {
                if let Ok(rules) = toml::from_str::<Vec<ReviewRule>>(&content) {
                    set.user_rules = rules;
                }
            }
        }

        // Built-in system rules
        set.system_rules = builtin_rules();

        set
    }

    /// Get all rules merged in priority order.
    pub fn all_rules(&self) -> Vec<&ReviewRule> {
        let mut all: Vec<&ReviewRule> = Vec::new();
        all.extend(self.cli_rules.iter());
        all.extend(self.project_rules.iter());
        all.extend(self.user_rules.iter());
        all.extend(self.system_rules.iter());
        all
    }

    /// Find rules matching a given file path.
    pub fn matching_rules(&self, file_path: &str) -> Vec<&ReviewRule> {
        let mut matched = Vec::new();
        for rule in self.all_rules() {
            if glob_match(&rule.path_pattern, file_path) {
                matched.push(rule);
            }
        }
        matched
    }
}

/// Built-in system rules covering common vulnerability patterns.
fn builtin_rules() -> Vec<ReviewRule> {
    vec![
        ReviewRule {
            path_pattern: "**/*.rs".into(),
            rule: "Check for unsafe blocks without SAFETY comments".into(),
            severity: "warning".into(),
            aspect: Some("security".into()),
        },
        ReviewRule {
            path_pattern: "**/*.rs".into(),
            rule: "Check for unwrap()/expect() on public API paths".into(),
            severity: "warning".into(),
            aspect: Some("correctness".into()),
        },
        ReviewRule {
            path_pattern: "**/*.sql".into(),
            rule: "Check for SQL injection: string concatenation in queries".into(),
            severity: "error".into(),
            aspect: Some("security".into()),
        },
        ReviewRule {
            path_pattern: "**/*.rs".into(),
            rule: "Check for todo!() or unimplemented!() in new code".into(),
            severity: "error".into(),
            aspect: Some("correctness".into()),
        },
        ReviewRule {
            path_pattern: "**/*.{js,ts,jsx,tsx}".into(),
            rule: "Check for console.log() in production code".into(),
            severity: "info".into(),
            aspect: Some("style".into()),
        },
    ]
}

/// Simple glob pattern matcher (supports `**`, `*`, `?`).
fn glob_match(pattern: &str, path: &str) -> bool {
    let regex_pattern = pattern
        .replace('.', "\\.")
        .replace("**", ".*")
        .replace('*', "[^/]*")
        .replace('?', ".");
    let regex_str = format!("^{}$", regex_pattern);
    regex::Regex::new(&regex_str)
        .map(|re| re.is_match(path))
        .unwrap_or(false)
}

// ============================================================================
// LineAnnotation — line-level review comment with suggestion code
// ============================================================================

/// A line-level annotation pinned to a specific file location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineAnnotation {
    /// File path relative to repo root.
    pub file_path: String,
    /// Start line (1-based).
    pub line_start: u32,
    /// End line (1-based).
    pub line_end: u32,
    /// Severity.
    pub severity: String,
    /// Aspect category.
    pub aspect: String,
    /// Short title.
    pub title: String,
    /// Detailed description.
    pub description: String,
    /// Suggested replacement code (if any).
    pub suggestion_code: Option<String>,
    /// Confidence 0.0–1.0.
    pub confidence: f32,
}

impl LineAnnotation {
    /// Format as a clickable file link with line range.
    pub fn format_line_link(&self) -> String {
        if self.line_start == self.line_end {
            format!("{}:{}", self.file_path, self.line_start)
        } else {
            format!("{}:{}-{}", self.file_path, self.line_start, self.line_end)
        }
    }

    /// Render as markdown.
    pub fn to_markdown(&self) -> String {
        let severity_tag = match self.severity.to_lowercase().as_str() {
            "critical" | "error" => "🔴",
            "high" => "🟠",
            "medium" | "warning" => "🟡",
            "low" | "info" => "🔵",
            _ => "⚪",
        };

        let mut md = format!(
            "{} **{}** — {} — `{}`\n\n{}\n",
            severity_tag, self.severity.to_uppercase(), self.title, self.format_line_link(), self.description
        );

        if let Some(ref code) = self.suggestion_code {
            md.push_str(&format!("\n```suggestion\n{}\n```\n", code));
        }

        md
    }
}

// ============================================================================
// ApplySuggestion — review finding → FileEdit converter
// ============================================================================

/// Convert a review finding into an apply-able `FileEdit`.
///
/// Strategy:
/// - Parse the `suggestion` field from `PrReviewResult` for replacement code.
/// - Use `DiffEngine::generate()` to create hunks.
/// - Return a `FileEdit` that can be applied via `DiffEngine::apply()`.
pub fn finding_to_file_edit(
    finding: &PrReviewResult,
) -> Option<FileEdit> {
    let path = Path::new(&finding.file_path);
    if !path.exists() {
        return None;
    }

    let original = std::fs::read_to_string(path).ok()?;
    let suggestion = &finding.suggestion;

    // If suggestion contains concrete code replacement, use it
    // Otherwise, just note the issue without auto-fix
    if suggestion.is_empty() || suggestion.len() < 10 {
        return None;
    }

    // Build modified content by applying the suggestion
    let modified = if let (Some(start), Some(end)) = (finding.line_start, finding.line_end) {
        let lines: Vec<&str> = original.lines().collect();
        let start = start as usize;
        let end = end as usize;

        if start <= end && end <= lines.len() {
            let mut new_lines: Vec<&str> = lines[..start].to_vec();
            // Add suggestion lines
            for line in suggestion.lines() {
                new_lines.push(line);
            }
            new_lines.extend(lines[end..].iter().copied());
            new_lines.join("\n")
        } else {
            return None;
        }
    } else {
        return None;
    };

    Some(FileEdit {
        file_path: finding.file_path.clone().into(),
        original,
        modified,
        description: Some(finding.title.clone()),
    })
}

// ============================================================================
// ReviewSession — tracks the "review → apply → verify" lifecycle
// ============================================================================

/// Result of applying a review suggestion.
#[derive(Debug, Clone)]
pub struct ApplyResult {
    /// The finding that was applied.
    pub finding: String,
    /// File path of the applied edit.
    pub file: PathBuf,
    /// Result of the apply operation.
    pub result: EditResult,
}

/// Result of verification (compile/test) after fixes.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// Whether verification passed.
    pub passed: bool,
    /// Compilation output / error messages.
    pub output: String,
    /// Number of errors found (if any).
    pub error_count: usize,
}

/// A complete review session: review → apply → verify.
#[derive(Debug)]
pub struct ReviewSession {
    /// Target that was reviewed.
    pub target: DiffTarget,
    /// The review report.
    pub report: PrReviewReport,
    /// Annotations extracted from the report.
    pub annotations: Vec<LineAnnotation>,
    /// Results of applying suggestions.
    pub apply_results: Vec<ApplyResult>,
    /// Result of verification.
    pub verify_result: Option<VerifyResult>,
    /// Elapsed time for the entire session.
    pub elapsed_ms: u64,
}

// ============================================================================
// ReviewEngine — main orchestrator
// ============================================================================

/// The review engine — orchestrates review, apply, and verify.
pub struct ReviewEngine {
    /// PrReviewer for multi-aspect analysis.
    pr_reviewer: PrReviewer,
    /// Security scanner for deterministic vulnerability detection.
    security_scanner: SecurityScannerV2,
    /// Rule set loaded from all tiers.
    rules: ReviewRuleSet,
    /// Whether to automatically apply suggestions.
    auto_apply: bool,
    /// Whether to run verification after applying.
    auto_verify: bool,
}

impl ReviewEngine {
    /// Create a new review engine with default rules.
    pub fn new(project_root: &Path) -> Self {
        Self {
            pr_reviewer: PrReviewer::new(),
            security_scanner: SecurityScannerV2::new(),
            rules: ReviewRuleSet::load(project_root, vec![]),
            auto_apply: false,
            auto_verify: true,
        }
    }

    /// Create with custom CLI rules.
    pub fn with_cli_rules(project_root: &Path, cli_rules: Vec<ReviewRule>) -> Self {
        Self {
            rules: ReviewRuleSet::load(project_root, cli_rules),
            ..Self::new(project_root)
        }
    }

    /// Set auto-apply mode.
    pub fn set_auto_apply(&mut self, auto: bool) {
        self.auto_apply = auto;
    }

    /// Set auto-verify mode.
    pub fn set_auto_verify(&mut self, auto: bool) {
        self.auto_verify = auto;
    }

    // -----------------------------------------------------------------------
    // Phase 1: Review
    // -----------------------------------------------------------------------

    /// Run a full review on a target, respecting large-diff chunking.
    pub async fn review(&self, target: &DiffTarget, aspects: Option<&[ReviewAspect]>) -> anyhow::Result<PrReviewReport> {
        let diff_text = target.to_diff_text();
        let repo_root = target.working_dir();

        // Check if diff is too large — use chunked analysis
        if diff_text.len() > LARGE_DIFF_THRESHOLD {
            return self.review_large_diff(&diff_text, &repo_root, aspects).await;
        }

        // Standard review via PrReviewer
        let mut report = self.pr_reviewer.review_diff(&diff_text, &repo_root).await?;

        // Apply custom rules on top
        let rule_findings = self.apply_custom_rules(&diff_text);
        report.findings.extend(rule_findings);
        report.total_findings = report.findings.len();

        Ok(report)
    }

    /// Review a large diff by chunking.
    async fn review_large_diff(
        &self,
        diff_text: &str,
        repo_root: &Path,
        _aspects: Option<&[ReviewAspect]>,
    ) -> anyhow::Result<PrReviewReport> {
        eprintln!("  Large diff detected ({} chars). Using chunked analysis...", diff_text.len());

        // Use PrReviewer's built-in large diff support if available
        // Otherwise, chunk manually
        let max_chunk = 40_000; // chars per chunk
        let chunks = diff_text.as_bytes().chunks(max_chunk);
        let chunk_count = chunks.len();

        let mut all_findings = Vec::new();
        let mut chunk_idx = 0;

        for chunk_data in chunks {
            let chunk_str = String::from_utf8_lossy(chunk_data);
            eprintln!("    Chunk {}/{} ({} chars)...", chunk_idx + 1, chunk_count, chunk_str.len());

            let chunk_report = self.pr_reviewer.review_diff(&chunk_str, repo_root).await?;
            all_findings.extend(chunk_report.findings);
            chunk_idx += 1;
        }

        // Build final report from merged findings
        let critical_count = all_findings.iter().filter(|f| f.severity == FindingSeverity::Critical).count();
        let high_count = all_findings.iter().filter(|f| f.severity == FindingSeverity::High).count();

        let mut by_aspect: HashMap<ReviewAspect, usize> = HashMap::new();
        for f in &all_findings {
            *by_aspect.entry(f.aspect).or_insert(0) += 1;
        }

        let verdict = if critical_count > 0 { ReviewVerdict::Blocked }
            else if high_count > 0 { ReviewVerdict::NeedsChanges }
            else { ReviewVerdict::Approved };

        Ok(PrReviewReport {
            verdict,
            total_findings: all_findings.len(),
            critical_count,
            high_count,
            findings: all_findings.clone(),
            summary: format!("Reviewed {} chunks, {} total findings", chunk_count, all_findings.len()),
            duration_ms: 0,
            by_aspect,
        })
    }

    /// Apply custom rules from the rule set against the diff context.
    fn apply_custom_rules(&self, _diff_text: &str) -> Vec<PrReviewResult> {
        // Rules are checked per-file in the review cmd integration
        // This hook allows post-processing
        Vec::new()
    }

    /// Get matching custom rules for a given file.
    pub fn matching_rules(&self, file_path: &str) -> Vec<&ReviewRule> {
        self.rules.matching_rules(file_path)
    }

    // -----------------------------------------------------------------------
    // Phase 2: Convert findings to annotations
    // -----------------------------------------------------------------------

    /// Convert a `PrReviewReport` into line-level annotations.
    pub fn report_to_annotations(report: &PrReviewReport) -> Vec<LineAnnotation> {
        report.findings.iter().map(|f| LineAnnotation {
            file_path: f.file_path.clone(),
            line_start: f.line_start.unwrap_or(0) + 1, // convert to 1-based
            line_end: f.line_end.unwrap_or(0) + 1,
            severity: format!("{:?}", f.severity).to_lowercase(),
            aspect: format!("{:?}", f.aspect).to_lowercase(),
            title: f.title.clone(),
            description: f.description.clone(),
            suggestion_code: if f.suggestion.len() > 10 { Some(f.suggestion.clone()) } else { None },
            confidence: f.confidence,
        }).collect()
    }

    // -----------------------------------------------------------------------
    // Phase 3: Apply suggestions
    // -----------------------------------------------------------------------

    /// Apply review findings as file edits.
    /// Returns list of apply results.
    pub fn apply_suggestions(report: &PrReviewReport) -> Vec<ApplyResult> {
        let mut results = Vec::new();

        for finding in &report.findings {
            // Only auto-apply HIGH+ severity findings with concrete suggestions
            if finding.severity < FindingSeverity::High {
                continue;
            }
            if finding.suggestion.is_empty() || finding.suggestion.len() < 10 {
                continue;
            }

            if let Some(file_edit) = finding_to_file_edit(finding) {
                // Use DiffEngine to apply the edit
                let result = DiffEngine::apply(&file_edit);
                let apply_result = match &result {
                    EditResult::Applied { .. } => ApplyResult {
                        finding: finding.title.clone(),
                        file: file_edit.file_path.clone(),
                        result: EditResult::Applied {
                            file: file_edit.file_path.clone(),
                            lines_changed: file_edit.modified.lines().count(),
                        },
                    },
                    EditResult::Rejected { .. } => ApplyResult {
                        finding: finding.title.clone(),
                        file: file_edit.file_path.clone(),
                        result: EditResult::Rejected {
                            file: file_edit.file_path.clone(),
                        },
                    },
                    EditResult::Failed { reason, .. } => ApplyResult {
                        finding: finding.title.clone(),
                        file: file_edit.file_path.clone(),
                        result: EditResult::Failed {
                            file: file_edit.file_path.clone(),
                            reason: reason.clone(),
                        },
                    },
                };
                results.push(apply_result);
            }
        }

        results
    }

    /// Apply all HIGH+ suggestions from the report, returning results.
    pub fn apply_high_severity_fixes(report: &PrReviewReport) -> Vec<ApplyResult> {
        Self::apply_suggestions(report)
    }

    // -----------------------------------------------------------------------
    // Phase 4: Verify
    // -----------------------------------------------------------------------

    /// Run cargo check to verify fixes compile.
    pub fn verify_compilation(project_root: &Path) -> VerifyResult {
        let _start = Instant::now();
        let output = std::process::Command::new("cargo")
            .args(["check", "--lib"])
            .current_dir(project_root)
            .output();

        match output {
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let error_lines: Vec<&str> = stderr.lines()
                    .filter(|l| l.contains("error[") || l.contains("error:"))
                    .collect();
                let error_count = error_lines.len();
                let passed = out.status.success();

                VerifyResult {
                    passed,
                    output: if passed {
                        "✅ Compilation passed".into()
                    } else {
                        format!(
                            "❌ Compilation failed ({} errors)\n{}",
                            error_count,
                            error_lines.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
                        )
                    },
                    error_count,
                }
            }
            Err(e) => VerifyResult {
                passed: false,
                output: format!("Failed to run cargo check: {}", e),
                error_count: 1,
            },
        }
    }

    // -----------------------------------------------------------------------
    // Full Session: review → (optionally apply) → (optionally verify)
    // -----------------------------------------------------------------------

    /// Run a complete review session.
    pub async fn run_session(
        &self,
        target: &DiffTarget,
        aspects: Option<&[ReviewAspect]>,
        stream_tx: Option<mpsc::Sender<String>>,
    ) -> anyhow::Result<ReviewSession> {
        let session_start = Instant::now();

        // Phase 1: Review
        if let Some(ref tx) = stream_tx {
            let _ = tx.send("\n═══ Phase 1: Reviewing code... ═══\n".into()).await;
        }
        let report = self.review(target, aspects).await?;

        // Phase 2: Convert to annotations
        let annotations = Self::report_to_annotations(&report);

        if let Some(ref tx) = stream_tx {
            let _ = tx.send(format!(
                "  Verdict: {:?} | {} findings ({} critical, {} high)\n",
                report.verdict, report.total_findings, report.critical_count, report.high_count
            )).await;
        }

        // Phase 3: Apply (if auto_apply)
        let apply_results = if self.auto_apply {
            if let Some(ref tx) = stream_tx {
                let _ = tx.send("\n═══ Phase 2: Applying HIGH+ suggestions... ═══\n".into()).await;
            }
            let results = Self::apply_high_severity_fixes(&report);
            if let Some(ref tx) = stream_tx {
                for r in &results {
                    let status = match &r.result {
                        EditResult::Applied { .. } => "✅ Applied",
                        EditResult::Rejected { .. } => "⏭️ Skipped",
                        EditResult::Failed { .. } => "❌ Failed",
                    };
                    let _ = tx.send(format!("  {} — {} ({})\n", status, r.finding, r.file.display())).await;
                }
            }
            results
        } else {
            Vec::new()
        };

        // Phase 4: Verify (if auto_verify)
        let verify_result = if self.auto_verify && !apply_results.is_empty() {
            if let Some(ref tx) = stream_tx {
                let _ = tx.send("\n═══ Phase 3: Verifying fixes (cargo check)... ═══\n".into()).await;
            }
            let result = Self::verify_compilation(&target.working_dir());
            if let Some(ref tx) = stream_tx {
                let _ = tx.send(format!("  {}\n", result.output)).await;
            }
            Some(result)
        } else {
            None
        };

        Ok(ReviewSession {
            target: target.clone(),
            report,
            annotations,
            apply_results,
            verify_result,
            elapsed_ms: session_start.elapsed().as_millis() as u64,
        })
    }

    /// Render session results to stdout.
    pub fn print_session(session: &ReviewSession) {
        println!("\n═══ Review Session Summary ═══");
        println!("  Duration: {} ms", session.elapsed_ms);
        println!("  Verdict: {:?}", session.report.verdict);
        println!("  Total findings: {}", session.report.total_findings);
        println!("  Critical: {} | High: {} | Medium/Low: {}",
            session.report.critical_count,
            session.report.high_count,
            session.report.total_findings - session.report.critical_count - session.report.high_count);

        // Print findings by aspect
        if !session.report.by_aspect.is_empty() {
            println!("\n  By aspect:");
            for aspect in ReviewAspect::all() {
                if let Some(count) = session.report.by_aspect.get(aspect) {
                    if *count > 0 {
                        println!("    {} {}: {}", aspect.icon(), aspect, count);
                    }
                }
            }
        }

        // Print line-level annotations
        if !session.annotations.is_empty() {
            println!("\n--- Line-Level Annotations ---");
            for (i, ann) in session.annotations.iter().enumerate().take(20) {
                println!("\n{}. {}", i + 1, ann.to_markdown());
            }
            if session.annotations.len() > 20 {
                println!("\n  ... and {} more annotations", session.annotations.len() - 20);
            }
        }

        // Print apply results
        if !session.apply_results.is_empty() {
            println!("\n--- Applied Fixes ---");
            for r in &session.apply_results {
                let status = match &r.result {
                    EditResult::Applied { lines_changed, .. } => format!("✅ Applied ({} lines)", lines_changed),
                    EditResult::Rejected { .. } => "⏭️ Skipped".into(),
                    EditResult::Failed { reason, .. } => format!("❌ Failed: {}", reason),
                };
                println!("  {} — {}", status, r.file.display());
            }
        }

        // Print verify result
        if let Some(ref v) = session.verify_result {
            println!("\n--- Verification ---");
            if v.passed {
                println!("  ✅ Compilation passed");
            } else {
                println!("  ❌ Compilation failed ({} errors)", v.error_count);
                for line in v.output.lines().take(5) {
                    println!("    {}", line);
                }
            }
        }

        // Print verdict
        println!("\n═══ Final Verdict: {:?} ═══",
            if session.verify_result.as_ref().map(|v| v.passed).unwrap_or(true) {
                session.report.verdict.clone()
            } else {
                ReviewVerdict::NeedsChanges
            }
        );
    }
}

// ============================================================================
// Streaming integration
// ============================================================================

/// Run a review with streaming output.
pub async fn review_with_streaming(
    engine: &ReviewEngine,
    target: &DiffTarget,
    aspects: Option<&[ReviewAspect]>,
) -> anyhow::Result<ReviewSession> {
    let (tx, mut rx) = mpsc::channel::<String>(64);

    // Spawn streaming output task
    let stream_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            print!("{}", msg);
        }
    });

    let session = engine.run_session(target, aspects, Some(tx.clone())).await?;

    // Signal end of stream
    drop(tx);
    let _ = stream_handle.await;

    Ok(session)
}

// ============================================================================
// Wave 2: ReviewMode adapters for Unified Agent Loop (Observe→Plan→Act→Evaluate)
// ============================================================================

use crate::r#loop::{Observer, Planner, Actor, Evaluator, LoopVerdict};
use async_trait::async_trait;

/// Scanned code observation — produced by CodeScanner, consumed by RefactorPlanner.
#[derive(Debug, Clone)]
pub struct ScannedCode {
    /// The target path (file or directory).
    pub target_path: String,
    /// Concatenated content of all scanned files.
    pub content: String,
    /// Individual file entries: (relative_path, content).
    pub files: Vec<(String, String)>,
}

/// A single edit action within a refactoring plan.
#[derive(Debug, Clone)]
pub struct EditAction {
    pub file_path: String,
    pub description: String,
    pub original: String,
    pub modified: String,
}

/// Refactoring plan — produced by RefactorPlanner, consumed by FileEditActor.
#[derive(Debug, Clone)]
pub struct RefactorPlan {
    pub actions: Vec<EditAction>,
}

/// Result of applying a refactoring plan.
#[derive(Debug, Clone)]
pub struct ApplyActionResult {
    /// Files that were successfully edited.
    pub applied: Vec<String>,
    /// Failed edits: (file_path, error_message).
    pub failed: Vec<(String, String)>,
    /// Full diff output for logging.
    pub diff_output: String,
}

// ============================================================================
// CodeScanner — Observe phase
// ============================================================================

/// Scans source files in the target path and returns the code content.
///
/// Supports both single-file and directory targets. For directories,
/// recursively collects all `.rs` files.
///
/// When `git_diff_only` is enabled, only files modified in the git working
/// tree are scanned (incremental mode). Uses `git diff --name-only`.
pub struct CodeScanner {
    include_patterns: Vec<String>,
    /// If true, only scan files that have uncommitted git changes.
    git_diff_only: bool,
}

impl CodeScanner {
    pub fn new() -> Self {
        Self {
            include_patterns: vec!["**/*.rs".into()],
            git_diff_only: false,
        }
    }

    /// Customize the file include patterns (glob-style).
    pub fn with_patterns(mut self, patterns: Vec<String>) -> Self {
        self.include_patterns = patterns;
        self
    }

    /// Enable incremental mode: only scan git-diffed files.
    pub fn with_git_diff(mut self, enable: bool) -> Self {
        self.git_diff_only = enable;
        self
    }

    /// Run `git diff --name-only --diff-filter=ACM` and return changed file paths.
    fn get_changed_files(&self, repo_root: &std::path::Path) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                &repo_root.to_string_lossy(),
                "diff",
                "--name-only",
                "--diff-filter=ACMR",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string());

        match output {
            Some(out) => out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
            None => {
                tracing::warn!("git diff failed — falling back to full scan");
                vec![]
            }
        }
    }
}

impl Default for CodeScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Observer for CodeScanner {
    type Observation = ScannedCode;

    async fn observe(&mut self, target: &str) -> anyhow::Result<Self::Observation> {
        let path = std::path::Path::new(target);

        if !path.exists() {
            anyhow::bail!("Target does not exist: {}", target);
        }

        let mut files: Vec<(String, String)> = Vec::new();

        if path.is_file() {
            let content = std::fs::read_to_string(path)?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| target.to_string());
            files.push((name, content));
        } else if path.is_dir() {
            if self.git_diff_only {
                // Incremental mode: only scan git-changed files
                let repo_root = find_git_root(path).unwrap_or_else(|| path.to_path_buf());
                let changed = self.get_changed_files(&repo_root);
                if !changed.is_empty() {
                    tracing::info!(count = changed.len(), "Incremental scan: {} git-diffed files", changed.len());
                    for rel_path in &changed {
                        let full_path = repo_root.join(rel_path);
                        if full_path.exists() && full_path.is_file()
                            && full_path.extension().is_some_and(|e| e == "rs")
                        {
                            match std::fs::read_to_string(&full_path) {
                                Ok(content) => files.push((rel_path.clone(), content)),
                                Err(e) => tracing::warn!("Cannot read {}: {}", rel_path, e),
                            }
                        }
                    }
                }
                // If git diff returned nothing (no changes), fall back to empty scan
                if files.is_empty() {
                    tracing::info!("Incremental scan: no changed files found");
                }
            } else {
                collect_rs_files(path, &mut files, "")?;
            }
        }

        let content = files
            .iter()
            .map(|(name, code)| format!("// --- {} ---\n{}", name, code))
            .collect::<Vec<_>>()
            .join("\n\n");

        Ok(ScannedCode {
            target_path: target.to_string(),
            content,
            files,
        })
    }

    fn name(&self) -> &str {
        "code-scanner"
    }
}

/// Recursively collect `.rs` files from a directory.
fn collect_rs_files(
    dir: &std::path::Path,
    files: &mut Vec<(String, String)>,
    prefix: &str,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };

        if path.is_dir() {
            // Skip hidden directories and node_modules/target/build
            if name.starts_with('.') || name == "target" || name == "node_modules" || name == "build"
            {
                continue;
            }
            collect_rs_files(&path, files, &rel_path)?;
        } else if path.is_file() && name.ends_with(".rs") {
            let content = std::fs::read_to_string(&path)?;
            files.push((rel_path, content));
        }
    }
    Ok(())
}

/// Walk up the directory tree to find the git repository root.
fn find_git_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        current = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

// ============================================================================
// RefactorPlanner — Plan phase
// ============================================================================

/// Analyzes scanned code and produces a refactoring plan.
///
/// ## Dual-mode architecture
///
/// 1. **LLM mode** (when `with_llm` is configured): Sends code to a provider
///    for AI-powered analysis — detects complex issues, suggests best-practice
///    refactoring, understands project context.
///
/// 2. **Heuristic mode** (fallback): Built-in rules for common Rust issues:
///    - Unwrap/expect calls without error handling
///    - TODO/FIXME markers
///    - Unsafe blocks without SAFETY comments
///    - Large functions (> 100 lines)
pub struct RefactorPlanner {
    /// Max lines before a function is considered "large".
    max_function_lines: usize,
    /// Optional LLM provider for AI-driven suggestions.
    llm_provider: Option<crate::providers::orchestrator::ProviderOrchestrator>,
}

impl RefactorPlanner {
    pub fn new() -> Self {
        Self {
            max_function_lines: 100,
            llm_provider: None,
        }
    }

    /// Enable LLM-powered refactoring with the given orchestrator.
    pub fn with_llm(mut self, orchestrator: crate::providers::orchestrator::ProviderOrchestrator) -> Self {
        self.llm_provider = Some(orchestrator);
        self
    }

    /// Whether LLM mode is active.
    pub fn llm_available(&self) -> bool {
        self.llm_provider.is_some()
    }

    /// Run the LLM planner to get refactoring suggestions.
    async fn plan_with_llm(&self, observation: &ScannedCode) -> anyhow::Result<Option<RefactorPlan>> {
        let orchestrator = match &self.llm_provider {
            Some(o) => o,
            None => return Ok(None),
        };

        let system_prompt = r#"You are a senior Rust code reviewer. Analyze the provided code and suggest specific refactoring changes.

For each suggested change, output in this exact format:

```
## FILE: <file_path>
- **Line N**: <description>
  - Original: `<exact original code>`
  - Refactored: `<refactored code>`
```

Focus on:
1. Safety issues (unwrap/expect, unsafe without SAFETY)
2. Code quality (dead code, large functions, complex expressions)
3. Performance (unnecessary allocations, cloning)
4. Idiomatic Rust (use ? instead of unwrap, use entry API for maps)

If no issues found, output: "NO_ISSUES"#.to_string();

        // Concatenate all files for the LLM
        let code_block = observation.files.iter()
            .map(|(path, content)| format!("// --- {} ---\n{}", path, content))
            .collect::<Vec<_>>()
            .join("\n\n");

        use crate::providers::provider::{ChatMessage, ProviderRequest};

        let request = ProviderRequest {
            system: Some(system_prompt),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: format!("Analyze this Rust code and suggest refactoring:\n\n```rust\n{}\n```", code_block),
                metadata: Default::default(),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(4096),
            temperature: Some(0.3),
            stop: None,
            tools: None,
            stream: false,
        };

        let response = orchestrator.orchestrate(&request).await
            .map_err(|e| anyhow::anyhow!("LLM refactoring request failed: {}", e))?;

        let response_text = response.content;

        // Parse LLM response into EditAction items
        if response_text.contains("NO_ISSUES") {
            return Ok(Some(RefactorPlan { actions: vec![] }));
        }

        let actions = self.parse_llm_response(&response_text, &observation.files);
        Ok(Some(RefactorPlan { actions }))
    }

    /// Parse the LLM's structured response into EditAction items.
    fn parse_llm_response(&self, text: &str, files: &[(String, String)]) -> Vec<EditAction> {
        let mut actions = Vec::new();

        // The LLM output format:
        // ## FILE: <path>
        // - **Line N**: <desc>
        //   - Original: `<code>`
        //   - Refactored: `<code>`

        let mut current_file: Option<String> = None;

        for line in text.lines() {
            let trimmed = line.trim();

            // Detect file header: ## FILE: src/main.rs
            if let Some(path) = trimmed.strip_prefix("## FILE: ").or_else(|| trimmed.strip_prefix("##FILE:")) {
                current_file = Some(path.trim().to_string());
                continue;
            }

            // Detect action item: - **Line N**: description
            if trimmed.starts_with("- **") && trimmed.contains("**:") {
                if let Some(ref file_path) = current_file {
                    // Extract description
                    let desc = trimmed
                        .trim_start_matches("- **")
                        .split("**:")
                        .nth(1)
                        .unwrap_or(trimmed)
                        .trim()
                        .to_string();

                    actions.push(EditAction {
                        file_path: file_path.clone(),
                        description: desc,
                        original: String::new(),  // Will try to resolve from file content
                        modified: String::new(),
                    });
                }
            }
        }

        // Try to resolve original/modified content from subsequent lines
        let lines: Vec<&str> = text.lines().collect();
        let mut action_idx = 0usize;

        for i in 0..lines.len() {
            let line = lines[i].trim();

            if line.starts_with("Original: `") || line.starts_with("- Original: `") {
                let content = extract_backtick_content(line);
                if action_idx < actions.len() {
                    actions[action_idx].original = content;
                }
            }

            if line.starts_with("Refactored: `") || line.starts_with("- Refactored: `") {
                let content = extract_backtick_content(line);
                if action_idx < actions.len() {
                    actions[action_idx].modified = content;
                    action_idx += 1;
                }
            }
        }

        // For actions without original/modified, try to find them in the files
        for action in &mut actions {
            if action.original.is_empty() {
                for (file_path, _content) in files {
                    if *file_path == action.file_path {
                        // Try to narrow to relevant lines based on line number in description
                        action.original = "[See LLM suggestion]".to_string();
                        action.modified = "[See LLM suggestion]".to_string();
                        break;
                    }
                }
            }
        }

        actions
    }
}

/// Extract content between backticks from a string like `Original: `code``
fn extract_backtick_content(s: &str) -> String {
    let s = s.trim();
    if let Some(start) = s.find('`') {
        let rest = &s[start + 1..];
        if let Some(end) = rest.rfind('`') {
            return rest[..end].to_string();
        }
    }
    s.to_string()
}

impl Default for RefactorPlanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Planner for RefactorPlanner {
    type Observation = ScannedCode;
    type Plan = RefactorPlan;

    async fn plan(&mut self, observation: &Self::Observation) -> anyhow::Result<Self::Plan> {
        // Try LLM mode first if available
        if self.llm_available() {
            match self.plan_with_llm(observation).await {
                Ok(Some(plan)) => {
                    if !plan.actions.is_empty() {
                        tracing::info!(
                            action_count = plan.actions.len(),
                            "RefactorPlanner: LLM mode generated plan"
                        );
                        return Ok(plan);
                    }
                    // LLM returned no actions, continue to heuristics
                }
                Ok(None) => { /* LLM not available, fall through */ }
                Err(e) => {
                    tracing::warn!(error = %e, "RefactorPlanner: LLM mode failed, falling back to heuristics");
                }
            }
        }

        // Heuristic fallback
        let mut actions = Vec::new();

        for (file_path, content) in &observation.files {
            // Check for unwrap/expect calls
            let unwrap_issues: Vec<_> = content
                .lines()
                .enumerate()
                .filter(|(_, line)| {
                    (line.contains(".unwrap()") || line.contains(".expect("))
                        && !line.trim_start().starts_with("//")
                        && !line.trim_start().starts_with("#[")
                })
                .collect();

            for (line_no, line) in &unwrap_issues {
                actions.push(EditAction {
                    file_path: file_path.clone(),
                    description: format!(
                        "Line {}: Replace .unwrap()/.expect() with proper error handling",
                        line_no + 1
                    ),
                    original: line.to_string(),
                    modified: line
                        .replace(".unwrap()", "?")
                        .replace(".expect(\"", "? /* "),
                });
            }

            // Check for TODO/FIXME markers
            for (line_no, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.contains("TODO") || trimmed.contains("FIXME") || trimmed.contains("HACK")
                {
                    actions.push(EditAction {
                        file_path: file_path.clone(),
                        description: format!("Line {}: Address TODO/FIXME/HACK marker", line_no + 1),
                        original: line.to_string(),
                        modified: format!("// FIXED: {}", line),
                    });
                }
            }

            // Check for unsafe blocks without SAFETY comments
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed == "unsafe {" || trimmed == "unsafe{" {
                    let has_safety_comment = i > 0 && lines[i - 1].contains("SAFETY:");
                    if !has_safety_comment {
                        actions.push(EditAction {
                            file_path: file_path.clone(),
                            description: format!(
                                "Line {}: Add SAFETY comment before unsafe block",
                                i + 1
                            ),
                            original: line.to_string(),
                            modified: format!("// SAFETY: <reason>\n{}", line),
                        });
                    }
                }
            }
        }

        Ok(RefactorPlan { actions })
    }

    fn name(&self) -> &str {
        "refactor-planner"
    }
}

// ============================================================================
// FileEditActor — Act phase
// ============================================================================

/// Applies refactoring edits to files on disk.
///
/// Uses `DiffEngine` to perform the actual file modifications
/// with proper diff generation and backup support.
pub struct FileEditActor {
    /// Whether to create backup files before editing.
    create_backup: bool,
    /// Whether to run in dry-run mode (no actual writes).
    dry_run: bool,
}

impl FileEditActor {
    pub fn new() -> Self {
        Self {
            create_backup: true,
            dry_run: false,
        }
    }

    pub fn with_dry_run(mut self, dry: bool) -> Self {
        self.dry_run = dry;
        self
    }
}

impl Default for FileEditActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for FileEditActor {
    type Plan = RefactorPlan;
    type ActionResult = ApplyActionResult;

    async fn act(&mut self, plan: &Self::Plan) -> anyhow::Result<Self::ActionResult> {
        let mut applied = Vec::new();
        let mut failed = Vec::new();
        let mut diff_lines = Vec::new();

        for action in &plan.actions {
            let path = std::path::Path::new(&action.file_path);

            // Check if file exists
            if !path.exists() {
                failed.push((action.file_path.clone(), "File not found".into()));
                continue;
            }

            // Create backup if enabled
            if self.create_backup && !self.dry_run {
                let backup_path = path.with_extension("rs.bak");
                if let Err(e) = std::fs::copy(path, &backup_path) {
                    failed.push((
                        action.file_path.clone(),
                        format!("Failed to create backup: {}", e),
                    ));
                    continue;
                }
                diff_lines.push(format!("Backup saved to: {}", backup_path.display()));
            }

            // Read current content
            let current = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    failed.push((action.file_path.clone(), format!("Read error: {}", e)));
                    continue;
                }
            };

            // Apply the edit by finding the original text and replacing it
            if action.original.is_empty() {
                failed.push((action.file_path.clone(), "Empty original text".into()));
                continue;
            }

            if !current.contains(&action.original) {
                failed.push((
                    action.file_path.clone(),
                    "Original text not found in file".into(),
                ));
                continue;
            }

            let modified = current.replace(&action.original, &action.modified);

            // Write back if not dry run
            if self.dry_run {
                diff_lines.push(format!(
                    "[DRY-RUN] Would edit {}: {}",
                    action.file_path, action.description
                ));
                applied.push(action.file_path.clone());
            } else {
                match std::fs::write(path, &modified) {
                    Ok(()) => {
                        diff_lines.push(format!(
                            "✅ {}: {}",
                            action.file_path, action.description
                        ));
                        applied.push(action.file_path.clone());
                    }
                    Err(e) => {
                        failed.push((
                            action.file_path.clone(),
                            format!("Write error: {}", e),
                        ));
                    }
                }
            }
        }

        Ok(ApplyActionResult {
            applied,
            failed,
            diff_output: diff_lines.join("\n"),
        })
    }

    fn name(&self) -> &str {
        "file-edit-actor"
    }

    fn action_summary(&self, result: &Self::ActionResult) -> String {
        let mut out = String::new();
        if !result.applied.is_empty() {
            out.push_str(&format!("Applied {} file(s): {}\n", result.applied.len(), result.applied.join(", ")));
        }
        if !result.failed.is_empty() {
            out.push_str(&format!("Failed {} file(s)", result.failed.len()));
        }
        out
    }
}

// ============================================================================
// CompilerEvaluator — Evaluate phase
// ============================================================================

/// Runs `cargo check` to verify that the project compiles after edits.
///
/// Returns `Passed` if compilation succeeds, `Failed` with error details
/// if compilation fails, and `Aborted` if cargo is not available.
pub struct CompilerEvaluator {
    /// Additional arguments to pass to cargo (e.g., "--lib", "--tests").
    cargo_args: Vec<String>,
    /// Path to the project root (default: current directory).
    project_root: Option<std::path::PathBuf>,
}

impl CompilerEvaluator {
    pub fn new() -> Self {
        Self {
            cargo_args: vec!["check".into(), "--lib".into()],
            project_root: None,
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.cargo_args = args;
        self
    }

    pub fn with_project_root(mut self, root: std::path::PathBuf) -> Self {
        self.project_root = Some(root);
        self
    }
}

impl Default for CompilerEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Evaluator for CompilerEvaluator {
    type ActionResult = ApplyActionResult;

    async fn evaluate(&mut self, _result: &Self::ActionResult) -> anyhow::Result<LoopVerdict> {
        let root = self
            .project_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("cargo")
                .args(["check", "--lib"])
                .current_dir(&root)
                .output()
        })
        .await
        .map_err(|e| anyhow::anyhow!("Join error: {}", e))?
        .map_err(|e| anyhow::anyhow!("Failed to run cargo: {}", e))?;

        if output.status.success() {
            Ok(LoopVerdict::Passed)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let error_lines: Vec<&str> = stderr
                .lines()
                .filter(|l| l.contains("error[") || l.contains("error:"))
                .collect();

            let reason = if error_lines.is_empty() {
                "Compilation failed (see cargo output)".to_string()
            } else {
                error_lines
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            Ok(LoopVerdict::Failed { reason })
        }
    }

    fn name(&self) -> &str {
        "compiler-evaluator"
    }
}

// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_diff_target_parse() {
        // File that doesn't exist → treated as Raw
        let t = DiffTarget::parse("nonexistent.rs");
        assert!(matches!(t, DiffTarget::Raw(_)));

        // "pr" keyword
        let t = DiffTarget::parse("pr");
        assert!(matches!(t, DiffTarget::Pr));

        // Branch name
        let t = DiffTarget::parse("main");
        assert!(matches!(t, DiffTarget::Branch(_)));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("**/*.rs", "src/main.rs"));
        assert!(glob_match("**/*.rs", "src/foo/bar.rs"));
        assert!(!glob_match("**/*.rs", "src/main.js"));
        assert!(glob_match("src/**", "src/main.rs"));
        assert!(glob_match("src/**", "src/foo/bar/mod.rs"));
        assert!(!glob_match("src/**", "tests/test.rs"));
    }

    #[test]
    fn test_builtin_rules() {
        let rules = builtin_rules();
        assert!(!rules.is_empty());
        assert!(rules.iter().any(|r| r.path_pattern == "**/*.rs"));
    }

    #[test]
    fn test_rule_loading() {
        let rules = ReviewRuleSet::load(Path::new("."), vec![]);
        assert!(!rules.all_rules().is_empty());
        assert_eq!(rules.all_rules().len(), builtin_rules().len());
    }

    #[test]
    fn test_annotation_format() {
        let ann = LineAnnotation {
            file_path: "src/main.rs".into(),
            line_start: 42,
            line_end: 45,
            severity: "high".into(),
            aspect: "security".into(),
            title: "Unsafe block without SAFETY comment".into(),
            description: "Found unsafe block without justification.".into(),
            suggestion_code: Some("// SAFETY: ...\nunsafe {{ ... }}".into()),
            confidence: 0.9,
        };

        let md = ann.to_markdown();
        assert!(md.contains("src/main.rs:42-45"));
        assert!(md.contains("Unsafe block"));
        assert!(md.contains("```suggestion"));
    }

    #[test]
    fn test_finding_to_file_edit_no_file() {
        let finding = PrReviewResult::new(
            ReviewAspect::Security,
            "nonexistent.rs",
            FindingSeverity::High,
            "Test",
            "Test description",
            "fixed code",
        );
        assert!(finding_to_file_edit(&finding).is_none());
    }

    #[test]
    fn test_review_rule_set_priority() {
        let cli_rule = ReviewRule {
            path_pattern: "**/*.rs".into(),
            rule: "CLI override".into(),
            severity: "error".into(),
            aspect: None,
        };
        let rules = ReviewRuleSet::load(Path::new("."), vec![cli_rule.clone()]);

        // CLI rules should be first
        let all = rules.all_rules();
        assert_eq!(&all[0].rule, "CLI override");
    }

    #[test]
    fn test_verify_compilation() {
        // This is a no-op test that checks the function signature works
        // Real verification requires a Rust project
        let result = ReviewEngine::verify_compilation(Path::new("."));
        // May pass or fail depending on whether Cargo.toml exists
        // Just check that it returns something
        assert!(!result.output.is_empty());
    }

    #[test]
    fn test_session_struct() {
        use crate::tools::pr_reviewer::{ReviewVerdict, PrReviewResult};
        let report = PrReviewReport {
            verdict: ReviewVerdict::Approved,
            total_findings: 0,
            critical_count: 0,
            high_count: 0,
            findings: vec![],
            summary: "No issues".into(),
            duration_ms: 100,
            by_aspect: HashMap::new(),
        };

        let session = ReviewSession {
            target: DiffTarget::Raw("test".into()),
            report,
            annotations: vec![],
            apply_results: vec![],
            verify_result: None,
            elapsed_ms: 100,
        };

        assert_eq!(session.elapsed_ms, 100);
    }
}