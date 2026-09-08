//! Self-Reflection Mechanism - Validates results and learns from mistakes.
//!
//! This module provides:
//! - Result verification
//! - Self-correction
//! - Learning from feedback
//! - Confidence scoring

use std::sync::Arc;
use tokio::sync::RwLock;

/// A reflection result.
#[derive(Debug, Clone)]
pub struct ReflectionResult {
    pub is_valid: bool,
    pub confidence: f32,
    pub issues: Vec<ReflectionIssue>,
    pub suggestions: Vec<String>,
    pub corrections: Vec<Correction>,
}

/// An issue found during reflection.
#[derive(Debug, Clone)]
pub struct ReflectionIssue {
    pub severity: IssueSeverity,
    pub issue_type: IssueType,
    pub description: String,
    pub location: Option<IssueLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueType {
    SyntaxError,
    TypeError,
    LogicError,
    StyleViolation,
    SecurityIssue,
    PerformanceIssue,
    IncompleteResult,
    IncorrectAssumption,
}

#[derive(Debug, Clone)]
pub struct IssueLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

/// A suggested correction.
#[derive(Debug, Clone)]
pub struct Correction {
    pub original: String,
    pub corrected: String,
    pub reason: String,
}

/// Self-reflection engine.
pub struct SelfReflection {
    verification_rules: Vec<VerificationRule>,
    history: Arc<RwLock<Vec<ReflectionRecord>>>,
}

impl SelfReflection {
    pub fn new() -> Self {
        Self {
            verification_rules: Self::default_rules(),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Default verification rules.
    fn default_rules() -> Vec<VerificationRule> {
        vec![
            VerificationRule {
                name: "syntax_check".to_string(),
                check: VerificationCheck::Syntax,
                severity: IssueSeverity::Error,
                enabled: true,
            },
            VerificationRule {
                name: "type_check".to_string(),
                check: VerificationCheck::Types,
                severity: IssueSeverity::Error,
                enabled: true,
            },
            VerificationRule {
                name: "security_check".to_string(),
                check: VerificationCheck::Security,
                severity: IssueSeverity::Critical,
                enabled: true,
            },
            VerificationRule {
                name: "completeness_check".to_string(),
                check: VerificationCheck::Completeness,
                severity: IssueSeverity::Warning,
                enabled: true,
            },
        ]
    }

    /// Reflect on generated code.
    pub async fn reflect(&self, code: &str, context: &str) -> ReflectionResult {
        let mut issues = Vec::new();
        let mut suggestions = Vec::new();
        let mut corrections = Vec::new();

        // Run verification checks
        for rule in &self.verification_rules {
            if rule.enabled {
                let result = self.run_check(rule, code, context);
                issues.extend(result.issues);
                suggestions.extend(result.suggestions);
                corrections.extend(result.corrections);
            }
        }

        // Calculate overall validity and confidence
        let has_critical = issues.iter().any(|i| i.severity == IssueSeverity::Critical);
        let has_error = issues.iter().any(|i| i.severity == IssueSeverity::Error);

        let is_valid = !has_critical && !has_error;
        let confidence = self.calculate_confidence(&issues);

        let result = ReflectionResult {
            is_valid,
            confidence,
            issues: issues.clone(),
            suggestions: suggestions.clone(),
            corrections: corrections.clone(),
        };

        // Record in history
        let record = ReflectionRecord {
            timestamp: current_timestamp(),
            code_hash: simple_hash(code),
            is_valid,
            confidence,
            issue_count: issues.len(),
        };
        let _summary = record.format_summary();
        self.history.write().await.push(record);

        result
    }

    /// Run a verification check.
    fn run_check(&self, rule: &VerificationRule, code: &str, context: &str) -> CheckResult {
        match rule.check {
            VerificationCheck::Syntax => self.check_syntax(code),
            VerificationCheck::Types => self.check_types(code),
            VerificationCheck::Security => self.check_security(code),
            VerificationCheck::Completeness => self.check_completeness(code, context),
            VerificationCheck::Correctness => self.check_correctness(code),
        }
    }

    fn check_syntax(&self, code: &str) -> CheckResult {
        let mut issues = Vec::new();

        // Check for common syntax issues
        let lines: Vec<&str> = code.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Check for unclosed braces
            if trimmed.ends_with('{') && !trimmed.contains("if") && !trimmed.contains("for") && !trimmed.contains("fn") {
                // Potential unclosed block
            }

            // Check for unclosed parentheses
            if trimmed.matches('(').count() != trimmed.matches(')').count() {
                issues.push(ReflectionIssue {
                    severity: IssueSeverity::Error,
                    issue_type: IssueType::SyntaxError,
                    description: format!("Mismatched parentheses on line {}", i + 1),
                    location: Some(IssueLocation {
                        file: "generated".to_string(),
                        line: i + 1,
                        column: 0,
                    }),
                });
            }

            // Check for trailing semicolons in wrong places
            if trimmed.ends_with(";") && (trimmed.starts_with("fn ") || trimmed.starts_with("let ")) {
                issues.push(ReflectionIssue {
                    severity: IssueSeverity::Warning,
                    issue_type: IssueType::SyntaxError,
                    description: format!("Unexpected semicolon on line {}", i + 1),
                    location: Some(IssueLocation {
                        file: "generated".to_string(),
                        line: i + 1,
                        column: 0,
                    }),
                });
            }
        }

        CheckResult {
            issues,
            suggestions: vec![],
            corrections: vec![],
        }
    }

    fn check_types(&self, code: &str) -> CheckResult {
        let mut issues = Vec::new();

        // Simple type checking heuristics
        if code.contains("unwrap()") && !code.contains("?") {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Warning,
                issue_type: IssueType::TypeError,
                description: "Using unwrap() without ? operator".to_string(),
                location: None,
            });
        }

        if code.contains("expect(") {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Info,
                issue_type: IssueType::TypeError,
                description: "Consider using ? operator or unwrap_or".to_string(),
                location: None,
            });
        }

        CheckResult {
            issues,
            suggestions: vec!["Use Result/Option handling properly".to_string()],
            corrections: vec![],
        }
    }

    fn check_security(&self, code: &str) -> CheckResult {
        let mut issues = Vec::new();
        let code_lower = code.to_lowercase();

        // Check for hardcoded secrets
        if code_lower.contains("password") && code.contains("=") && !code.contains("getenv") {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Critical,
                issue_type: IssueType::SecurityIssue,
                description: "Potential hardcoded password detected".to_string(),
                location: None,
            });
        }

        // Check for SQL injection vulnerabilities
        if code_lower.contains("sql") && code.contains("format!") && code.contains("\"") {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Critical,
                issue_type: IssueType::SecurityIssue,
                description: "Potential SQL injection vulnerability".to_string(),
                location: None,
            });
        }

        // Check for eval usage
        if code_lower.contains("eval(") {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Critical,
                issue_type: IssueType::SecurityIssue,
                description: "Use of eval() is a security risk".to_string(),
                location: None,
            });
        }

        CheckResult {
            issues,
            suggestions: vec![],
            corrections: vec![],
        }
    }

    fn check_completeness(&self, code: &str, _context: &str) -> CheckResult {
        let mut issues = Vec::new();

        // Check if code ends mid-statement
        let trimmed = code.trim();
        if !trimmed.ends_with(';') && !trimmed.ends_with('}') {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Warning,
                issue_type: IssueType::IncompleteResult,
                description: "Code may be incomplete (no proper ending)".to_string(),
                location: None,
            });
        }

        // Check for TODO comments that indicate incomplete work
        if code.contains("TODO") || code.contains("FIXME") {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Info,
                issue_type: IssueType::IncompleteResult,
                description: "Code contains TODO/FIXME markers".to_string(),
                location: None,
            });
        }

        CheckResult {
            issues,
            suggestions: vec![],
            corrections: vec![],
        }
    }

    fn check_correctness(&self, code: &str) -> CheckResult {
        let mut issues = Vec::new();

        // Check for common logic errors
        if code.contains("if (true) { return true; } else { return false; }") {
            issues.push(ReflectionIssue {
                severity: IssueSeverity::Warning,
                issue_type: IssueType::LogicError,
                description: "Simplifiable boolean expression".to_string(),
                location: None,
            });
        }

        // Check for empty blocks
        let lines: Vec<&str> = code.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim() == "{}" {
                issues.push(ReflectionIssue {
                    severity: IssueSeverity::Info,
                    issue_type: IssueType::LogicError,
                    description: format!("Empty block on line {}", i + 1),
                    location: Some(IssueLocation {
                        file: "generated".to_string(),
                        line: i + 1,
                        column: 0,
                    }),
                });
            }
        }

        CheckResult {
            issues,
            suggestions: vec![],
            corrections: vec![],
        }
    }

    fn calculate_confidence(&self, issues: &[ReflectionIssue]) -> f32 {
        if issues.is_empty() {
            return 0.95;
        }

        let mut penalty: f32 = 0.0;
        for issue in issues {
            penalty += match issue.severity {
                IssueSeverity::Critical => 0.4,
                IssueSeverity::Error => 0.2,
                IssueSeverity::Warning => 0.1,
                IssueSeverity::Info => 0.02,
            };
        }

        let confidence: f32 = (1.0_f32 - penalty).max(0.0_f32);
        confidence
    }

    /// Learn from feedback.
    pub async fn learn(&self, code: &str, was_accepted: bool) {
        let record = FeedbackRecord {
            timestamp: current_timestamp(),
            code_hash: simple_hash(code),
            was_accepted,
        };

        // In a real implementation, would update internal models
        let _summary = record.format_summary();
        let _ = record;
    }

    /// Get reflection statistics.
    pub async fn stats(&self) -> ReflectionStats {
        let history = self.history.read().await;

        let total = history.len();
        let valid_count = history.iter().filter(|r| r.is_valid).count();
        let avg_confidence = if total > 0 {
            history.iter().map(|r| r.confidence).sum::<f32>() / total as f32
        } else {
            0.0
        };

        ReflectionStats {
            total_reflections: total,
            valid_count,
            invalid_count: total - valid_count,
            avg_confidence,
        }
    }
}

impl Default for SelfReflection {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct VerificationRule {
    name: String,
    check: VerificationCheck,
    severity: IssueSeverity,
    enabled: bool,
}

impl VerificationRule {
    /// Get the rule name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the severity level of this verification rule.
    pub fn severity(&self) -> &IssueSeverity {
        &self.severity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationCheck {
    Syntax,
    Types,
    Security,
    Completeness,
    Correctness,
}

#[derive(Debug)]
struct CheckResult {
    issues: Vec<ReflectionIssue>,
    suggestions: Vec<String>,
    corrections: Vec<Correction>,
}

#[derive(Debug, Clone)]
struct ReflectionRecord {
    timestamp: u64,
    code_hash: u64,
    is_valid: bool,
    confidence: f32,
    issue_count: usize,
}

impl ReflectionRecord {
    /// Get the Unix timestamp of this reflection.
    fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Get the hash of the code that was reflected upon.
    fn code_hash(&self) -> u64 {
        self.code_hash
    }

    /// Get the number of issues found during this reflection.
    fn issue_count(&self) -> usize {
        self.issue_count
    }

    /// Format a human-readable summary using all fields.
    pub fn format_summary(&self) -> String {
        format!(
            "ReflectionRecord(ts={}, hash={:x}, issues={}, valid={})",
            self.timestamp(),
            self.code_hash(),
            self.issue_count(),
            self.is_valid,
        )
    }
}

#[derive(Debug, Clone)]
struct FeedbackRecord {
    timestamp: u64,
    code_hash: u64,
    was_accepted: bool,
}

impl FeedbackRecord {
    /// Get the Unix timestamp of this feedback record.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Get the hash of the code this feedback is for.
    pub fn code_hash(&self) -> u64 {
        self.code_hash
    }

    /// Whether the suggested change was accepted.
    pub fn was_accepted(&self) -> bool {
        self.was_accepted
    }

    /// Format a human-readable summary using all fields.
    pub fn format_summary(&self) -> String {
        format!(
            "FeedbackRecord(ts={}, hash={:x}, accepted={})",
            self.timestamp(),
            self.code_hash(),
            self.was_accepted(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct ReflectionStats {
    pub total_reflections: usize,
    pub valid_count: usize,
    pub invalid_count: usize,
    pub avg_confidence: f32,
}

/// Simple hash function.
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    hash
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: self_reflection.rs:541")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reflect_valid_code() {
        let reflection = SelfReflection::new();
        let code = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;

        let result = reflection.reflect(code, "").await;
        assert!(result.is_valid);
        assert!(result.confidence > 0.8);
    }

    #[tokio::test]
    async fn test_reflect_security_issue() {
        let reflection = SelfReflection::new();
        let code = r#"
let password = "hardcoded123";
"#;

        let result = reflection.reflect(code, "").await;
        assert!(result.issues.iter().any(|i| i.severity == IssueSeverity::Critical));
    }

    #[tokio::test]
    async fn test_stats() {
        let reflection = SelfReflection::new();
        reflection.reflect("fn test() {}", "").await;

        let stats = reflection.stats().await;
        assert_eq!(stats.total_reflections, 1);
    }
}
