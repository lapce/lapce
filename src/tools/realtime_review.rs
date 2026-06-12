//! Real-time Code Review - Continuous code analysis while typing.
//!
//! This module provides real-time code review capabilities:
//! - Analyze code as it's written
//! - Suggest improvements inline
//! - Highlight potential bugs
//! - Offer quick fixes
//!
//! Inspired by GitHub Copilot's real-time suggestions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// A review finding with severity and suggestion.
#[derive(Debug, Clone)]
pub struct ReviewFinding {
    /// Unique ID.
    pub id: String,
    /// Severity level.
    pub severity: FindingSeverity,
    /// Category.
    pub category: FindingCategory,
    /// Message.
    pub message: String,
    /// Suggestion for fix.
    pub suggestion: String,
    /// Location.
    pub location: FindingLocation,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
    /// Auto-fixable.
    pub auto_fixable: bool,
}

/// Severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingSeverity {
    /// Informational only.
    Info,
    /// Minor issue.
    Warning,
    /// Significant issue.
    Error,
    /// Critical bug.
    Critical,
}

/// Category of finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingCategory {
    /// Performance issue.
    Performance,
    /// Security vulnerability.
    Security,
    /// Code smell.
    CodeSmell,
    /// Style issue.
    Style,
    /// Bug risk.
    BugRisk,
    /// Best practice.
    BestPractice,
    /// Documentation.
    Documentation,
}

/// Location of a finding.
#[derive(Debug, Clone)]
pub struct FindingLocation {
    pub file: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub start_column: usize,
    pub end_column: usize,
}

/// Real-time code reviewer.
pub struct RealtimeReviewer {
    /// Analysis rules.
    rules: Vec<ReviewRule>,
    /// Findings cache.
    findings: Arc<RwLock<HashMap<PathBuf, Vec<ReviewFinding>>>>,
    /// Debounce duration.
    debounce: Duration,
    /// Last analysis time.
    last_analysis: Arc<RwLock<Instant>>,
}

impl RealtimeReviewer {
    pub fn new() -> Self {
        Self {
            rules: Self::default_rules(),
            findings: Arc::new(RwLock::new(HashMap::new())),
            debounce: Duration::from_millis(500),
            last_analysis: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Default review rules.
    fn default_rules() -> Vec<ReviewRule> {
        vec![
            // Performance rules
            ReviewRule {
                name: "unnecessary_clone".to_string(),
                category: FindingCategory::Performance,
                severity: FindingSeverity::Warning,
                pattern: r"\.clone\(\).*\.clone\(\)".to_string(),
                message: "Double clone detected".to_string(),
                suggestion: "Consider removing unnecessary clone".to_string(),
                auto_fixable: false,
            },
            // Security rules
            ReviewRule {
                name: "hardcoded_password".to_string(),
                category: FindingCategory::Security,
                severity: FindingSeverity::Error,
                pattern: r#"(?i)(password|pwd|secret|api_key)\s*=\s*["'][^"']+["']"#.to_string(),
                message: "Potential hardcoded secret".to_string(),
                suggestion: "Use environment variables instead".to_string(),
                auto_fixable: false,
            },
            // Bug risk rules
            ReviewRule {
                name: "unwrap_on_option".to_string(),
                category: FindingCategory::BugRisk,
                severity: FindingSeverity::Error,
                pattern: r"\.(unwrap|expect)\(\)".to_string(),
                message: "Unwrap on Option without message".to_string(),
                suggestion: "Use unwrap_or, unwrap_or_else, or add expect with context".to_string(),
                auto_fixable: false,
            },
            // Style rules
            ReviewRule {
                name: "long_line".to_string(),
                category: FindingCategory::Style,
                severity: FindingSeverity::Info,
                pattern: r".{120,}".to_string(),
                message: "Line exceeds 120 characters".to_string(),
                suggestion: "Consider breaking into multiple lines".to_string(),
                auto_fixable: false,
            },
            // Best practice rules
            ReviewRule {
                name: "todo_comment".to_string(),
                category: FindingCategory::BestPractice,
                severity: FindingSeverity::Info,
                pattern: r"(?i)(TODO|FIXME|HACK|XXX)".to_string(),
                message: "TODO/FIXME comment found".to_string(),
                suggestion: "Address this before shipping".to_string(),
                auto_fixable: false,
            },
        ]
    }

    /// Analyze code and return findings.
    pub async fn analyze(&self, file: &PathBuf, content: &str) -> Vec<ReviewFinding> {
        let mut findings = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for rule in &self.rules {
            for (line_num, line) in lines.iter().enumerate() {
                if regex::Regex::new(&rule.pattern).ok().and_then(|r| r.find(line)).is_some() {
                    let finding = ReviewFinding {
                        id: format!("{}_{}_{}", rule.name, file.display(), line_num),
                        severity: rule.severity,
                        category: rule.category,
                        message: rule.message.clone(),
                        suggestion: rule.suggestion.clone(),
                        location: FindingLocation {
                            file: file.clone(),
                            start_line: line_num + 1,
                            end_line: line_num + 1,
                            start_column: 0,
                            end_column: line.len(),
                        },
                        confidence: 0.8,
                        auto_fixable: rule.auto_fixable,
                    };
                    findings.push(finding);
                }
            }
        }

        // Sort by severity
        findings.sort_by(|a, b| b.severity.cmp(&a.severity));

        // Cache findings
        let mut cached = self.findings.write().await;
        cached.insert(file.clone(), findings.clone());

        findings
    }

    /// Get cached findings for a file.
    pub async fn get_cached(&self, file: &PathBuf) -> Option<Vec<ReviewFinding>> {
        self.findings.read().await.get(file).cloned()
    }

    /// Get all findings across all files.
    pub async fn get_all(&self) -> HashMap<PathBuf, Vec<ReviewFinding>> {
        self.findings.read().await.clone()
    }

    /// Clear findings for a file.
    pub async fn clear(&self, file: &PathBuf) {
        self.findings.write().await.remove(file);
    }

    /// Clear all findings.
    pub async fn clear_all(&self) {
        self.findings.write().await.clear();
    }

    /// Get summary statistics.
    pub async fn summary(&self) -> ReviewSummary {
        let findings = self.findings.read().await;
        let mut by_severity: HashMap<String, u32> = HashMap::new();
        let mut by_category: HashMap<String, u32> = HashMap::new();
        let mut total = 0u32;

        for file_findings in findings.values() {
            for f in file_findings {
                *by_severity.entry(format!("{:?}", f.severity)).or_insert(0) += 1;
                *by_category.entry(format!("{:?}", f.category)).or_insert(0) += 1;
                total += 1;
            }
        }

        ReviewSummary {
            total_findings: total,
            by_severity,
            by_category,
            files_reviewed: findings.len() as u32,
        }
    }

    /// Format findings as inline comments.
    pub fn format_inline(&self, findings: &[ReviewFinding]) -> String {
        let mut output = String::new();

        for finding in findings {
            output.push_str(&format!(
                "{}:{}: {} [{}] {}\n",
                finding.location.file.display(),
                finding.location.start_line,
                format!("{:?}", finding.severity),
                format!("{:?}", finding.category),
                finding.message
            ));
        }

        output
    }

    /// Check if analysis should be run (debounced).
    pub async fn should_analyze(&self) -> bool {
        let last = *self.last_analysis.read().await;
        let now = Instant::now();
        now.duration_since(last) >= self.debounce
    }

    /// Mark analysis as done.
    pub async fn mark_analyzed(&self) {
        *self.last_analysis.write().await = Instant::now();
    }
}

impl Default for RealtimeReviewer {
    fn default() -> Self {
        Self::new()
    }
}

/// A review rule.
#[derive(Debug, Clone)]
struct ReviewRule {
    name: String,
    category: FindingCategory,
    severity: FindingSeverity,
    pattern: String,
    message: String,
    suggestion: String,
    auto_fixable: bool,
}

/// Summary of review findings.
#[derive(Debug, Clone)]
pub struct ReviewSummary {
    pub total_findings: u32,
    pub by_severity: HashMap<String, u32>,
    pub by_category: HashMap<String, u32>,
    pub files_reviewed: u32,
}

/// Review panel for displaying findings.
pub struct ReviewPanel {
    reviewer: Arc<RealtimeReviewer>,
}

impl ReviewPanel {
    pub fn new() -> Self {
        Self {
            reviewer: Arc::new(RealtimeReviewer::new()),
        }
    }

    /// Get panel content for display.
    pub async fn content(&self) -> String {
        let summary = self.reviewer.summary().await;
        let all = self.reviewer.get_all().await;

        let mut output = String::from("\n=== Real-time Code Review ===\n\n");

        // Summary section
        output.push_str(&format!("Total Findings: {}\n", summary.total_findings));
        output.push_str("By Severity:\n");
        for (sev, count) in &summary.by_severity {
            output.push_str(&format!("  {:?}: {}\n", sev, count));
        }
        output.push_str("By Category:\n");
        for (cat, count) in &summary.by_category {
            output.push_str(&format!("  {:?}: {}\n", cat, count));
        }
        output.push('\n');

        // Detailed findings
        output.push_str("=== Detailed Findings ===\n\n");
        for (file, findings) in &all {
            if !findings.is_empty() {
                output.push_str(&format!("File: {}\n", file.display()));
                for f in findings {
                    output.push_str(&format!(
                        "  L{}: {:?} - {} ({})\n",
                        f.location.start_line,
                        f.severity,
                        f.message,
                        if f.auto_fixable { "fixable" } else { "" }
                    ));
                }
                output.push('\n');
            }
        }

        output
    }

    /// Get the underlying reviewer.
    pub fn reviewer(&self) -> &RealtimeReviewer {
        &self.reviewer
    }
}

impl Default for ReviewPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_analyze_code() {
        let reviewer = RealtimeReviewer::new();
        let file = PathBuf::from("test.rs");
        let code = r#"
fn main() {
    let password = "secret123";
    let result = Some(1).unwrap();
}
"#;

        let findings = reviewer.analyze(&file, code).await;
        assert!(!findings.is_empty());
    }

    #[tokio::test]
    async fn test_summary() {
        let reviewer = RealtimeReviewer::new();
        let file = PathBuf::from("test.rs");

        reviewer.analyze(&file, "let x = Some(1).unwrap();").await;
        let summary = reviewer.summary().await;

        assert_eq!(summary.total_findings, 1);
        assert!(summary.files_reviewed >= 1);
    }

    #[tokio::test]
    async fn test_severity_order() {
        let sev1 = FindingSeverity::Info;
        let sev2 = FindingSeverity::Error;
        assert!(sev2 > sev1);
    }
}
