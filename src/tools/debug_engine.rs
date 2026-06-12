//! Debug Suggestion Engine - Root cause analysis and fix suggestions.
//!
//! This module provides:
//! - Error pattern recognition
//! - Root cause analysis
//! - Fix suggestion generation
//! - Stack trace analysis


/// A debug suggestion.
#[derive(Debug, Clone)]
pub struct DebugSuggestion {
    pub id: String,
    pub title: String,
    pub description: String,
    pub cause: String,
    pub fix: String,
    pub confidence: f32,
    pub severity: Severity,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

/// An error pattern with known fix.
#[derive(Debug, Clone)]
pub struct ErrorPattern {
    pub pattern: String,
    pub error_type: String,
    pub likely_cause: String,
    pub suggestion: String,
    pub fix_code: Option<String>,
    pub keywords: Vec<String>,
}

/// Debug suggestion engine.
pub struct DebugEngine {
    patterns: Vec<ErrorPattern>,
}

impl DebugEngine {
    pub fn new() -> Self {
        Self {
            patterns: Self::default_patterns(),
        }
    }

    /// Default error patterns.
    fn default_patterns() -> Vec<ErrorPattern> {
        vec![
            ErrorPattern {
                pattern: r"panicked at".to_string(),
                error_type: "Panic".to_string(),
                likely_cause: "Code panicked at runtime".to_string(),
                suggestion: "Add proper error handling with Result/Option types".to_string(),
                fix_code: Some(".unwrap_or_else(|e| handle_error(e))".to_string()),
                keywords: vec!["unwrap".to_string(), "expect".to_string(), "panic".to_string()],
            },
            ErrorPattern {
                pattern: r"connection refused".to_string(),
                error_type: "ConnectionError".to_string(),
                likely_cause: "Cannot connect to server or service".to_string(),
                suggestion: "Check if the server is running and the address is correct".to_string(),
                fix_code: None,
                keywords: vec!["tcp".to_string(), "connect".to_string(), "port".to_string()],
            },
            ErrorPattern {
                pattern: r"permission denied".to_string(),
                error_type: "PermissionError".to_string(),
                likely_cause: "Insufficient permissions to access resource".to_string(),
                suggestion: "Check file/directory permissions or run with elevated privileges".to_string(),
                fix_code: None,
                keywords: vec!["permission".to_string(), "access".to_string(), "denied".to_string()],
            },
            ErrorPattern {
                pattern: r"null pointer|NPE|panic: none".to_string(),
                error_type: "NullPointerError".to_string(),
                likely_cause: "Attempting to use a null/none value".to_string(),
                suggestion: "Add null check before accessing the value".to_string(),
                fix_code: Some(".unwrap_or(default_value)".to_string()),
                keywords: vec!["null".to_string(), "none".to_string(), "nil".to_string()],
            },
            ErrorPattern {
                pattern: r"timeout".to_string(),
                error_type: "TimeoutError".to_string(),
                likely_cause: "Operation took too long to complete".to_string(),
                suggestion: "Increase timeout or optimize the operation".to_string(),
                fix_code: None,
                keywords: vec!["timeout".to_string(), "timed out".to_string(), "duration".to_string()],
            },
            ErrorPattern {
                pattern: r"out of memory|OOM".to_string(),
                error_type: "OutOfMemoryError".to_string(),
                likely_cause: "System ran out of available memory".to_string(),
                suggestion: "Reduce memory usage or increase available memory".to_string(),
                fix_code: None,
                keywords: vec!["memory".to_string(), "alloc".to_string(), "heap".to_string()],
            },
            ErrorPattern {
                pattern: r"deadlock".to_string(),
                error_type: "DeadlockError".to_string(),
                likely_cause: "Threads waiting on each other indefinitely".to_string(),
                suggestion: "Review lock ordering or use async channels".to_string(),
                fix_code: None,
                keywords: vec!["lock".to_string(), "mutex".to_string(), "deadlock".to_string()],
            },
            ErrorPattern {
                pattern: r"race condition".to_string(),
                error_type: "RaceConditionError".to_string(),
                likely_cause: "Uncoordinated access to shared resource".to_string(),
                suggestion: "Use proper synchronization primitives".to_string(),
                fix_code: Some("Arc<Mutex<T>>".to_string()),
                keywords: vec!["race".to_string(), "concurrent".to_string(), "thread".to_string()],
            },
            ErrorPattern {
                pattern: r"invalid argument".to_string(),
                error_type: "InvalidArgumentError".to_string(),
                likely_cause: "Function received unexpected argument value".to_string(),
                suggestion: "Validate arguments before passing to function".to_string(),
                fix_code: None,
                keywords: vec!["invalid".to_string(), "argument".to_string(), "param".to_string()],
            },
            ErrorPattern {
                pattern: r"not found|404|ENOENT".to_string(),
                error_type: "NotFoundError".to_string(),
                likely_cause: "Requested resource does not exist".to_string(),
                suggestion: "Check if the resource path is correct or create it".to_string(),
                fix_code: None,
                keywords: vec!["found".to_string(), "exist".to_string(), "404".to_string()],
            },
        ]
    }

    /// Analyze error message and suggest fixes.
    pub fn analyze(&self, error: &str) -> Vec<DebugSuggestion> {
        let mut suggestions = Vec::new();

        for pattern in &self.patterns {
            if error.to_lowercase().contains(&pattern.pattern.to_lowercase()) {
                suggestions.push(DebugSuggestion {
                    id: format!("debug_{}", pattern.error_type.to_lowercase()),
                    title: format!("Potential {}", pattern.error_type),
                    description: format!("Found pattern matching: {}", pattern.error_type),
                    cause: pattern.likely_cause.clone(),
                    fix: pattern.suggestion.clone(),
                    confidence: self.calculate_confidence(error, &pattern.keywords),
                    severity: self.classify_severity(&pattern.error_type),
                    examples: vec![],
                });
            }
        }

        // Sort by confidence
        suggestions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).expect("unwrap failed: debug_engine.rs:160"));
        suggestions
    }

    /// Calculate confidence based on keyword matches.
    fn calculate_confidence(&self, error: &str, keywords: &[String]) -> f32 {
        let error_lower = error.to_lowercase();
        let mut matches = 0;

        for keyword in keywords {
            if error_lower.contains(&keyword.to_lowercase()) {
                matches += 1;
            }
        }

        let base = 0.5;
        let bonus = (matches as f32) * 0.1;
        (base + bonus).min(0.95)
    }

    /// Classify severity from error type.
    fn classify_severity(&self, error_type: &str) -> Severity {
        match error_type {
            "Panic" | "OutOfMemoryError" | "DeadlockError" => Severity::Critical,
            "NullPointerError" | "RaceConditionError" => Severity::Error,
            "TimeoutError" | "PermissionError" | "ConnectionError" => Severity::Warning,
            _ => Severity::Info,
        }
    }

    /// Analyze stack trace and extract useful info.
    pub fn analyze_stack_trace(&self, stack: &str) -> StackTraceAnalysis {
        let lines: Vec<&str> = stack.lines().collect();
        let mut frames = Vec::new();

        for line in lines {
            let trimmed = line.trim();
            // Common stack trace formats
            if trimmed.starts_with("at ") || trimmed.contains(".rs:") {
                frames.push(StackFrame {
                    location: trimmed.to_string(),
                    file: self.extract_file(trimmed),
                    line: self.extract_line(trimmed),
                    function: self.extract_function(trimmed),
                });
            }
        }

        // Extract error location and call chain before moving frames
        let error_location = frames.first().cloned();
        let call_chain = self.extract_call_chain(&frames);

        StackTraceAnalysis {
            frames,
            error_location,
            call_chain,
        }
    }

    fn extract_file(&self, line: &str) -> Option<String> {
        line.split(".rs:")
            .next()
            .map(|s| s.split_whitespace().last().unwrap_or("").to_string())
    }

    fn extract_line(&self, line: &str) -> Option<usize> {
        line.split(".rs:")
            .nth(1)
            .and_then(|s| s.split(|c: char| c.is_whitespace() || c == ')').next())
            .and_then(|s| s.parse().ok())
    }

    fn extract_function(&self, line: &str) -> Option<String> {
        // Try to extract function name from common patterns
        if line.contains("::") {
            let parts: Vec<&str> = line.split("::").collect();
            parts.last().map(|s| s.split('(').next().unwrap_or(s).to_string())
        } else {
            None
        }
    }

    fn extract_call_chain(&self, frames: &[StackFrame]) -> Vec<String> {
        frames.iter()
            .filter_map(|f| f.function.clone())
            .collect()
    }
}

impl Default for DebugEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub location: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub function: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StackTraceAnalysis {
    pub frames: Vec<StackFrame>,
    pub error_location: Option<StackFrame>,
    pub call_chain: Vec<String>,
}

/// Root cause analyzer for complex errors.
pub struct RootCauseAnalyzer {
    debug_engine: DebugEngine,
}

impl RootCauseAnalyzer {
    pub fn new() -> Self {
        Self {
            debug_engine: DebugEngine::new(),
        }
    }

    /// Perform root cause analysis.
    pub fn analyze(&self, error: &str, context: &str) -> RootCauseResult {
        // First, get basic suggestions
        let suggestions = self.debug_engine.analyze(error);

        // Analyze stack trace if present
        let stack_analysis = if context.contains("at ") || context.contains(".rs:") {
            self.debug_engine.analyze_stack_trace(context)
        } else {
            StackTraceAnalysis {
                frames: Vec::new(),
                error_location: None,
                call_chain: Vec::new(),
            }
        };

        // Determine root cause category
        let category = self.categorize_error(error, context);

        // Get first suggestion for immediate fix
        let immediate_fix = suggestions.first().map(|s| s.fix.clone()).unwrap_or_default();

        // Generate detailed analysis
        RootCauseResult {
            error_type: category.0.clone(),
            likely_causes: self.list_likely_causes(error, context),
            immediate_fix,
            prevention: self.suggest_prevention(&category.0),
            related_issues: self.find_related_issues(error),
            stack_analysis,
            suggestions,
        }
    }

    fn categorize_error(&self, error: &str, context: &str) -> (String, &'static str) {
        let error_lower = error.to_lowercase();
        let context_lower = context.to_lowercase();

        if error_lower.contains("deadlock") || context_lower.contains("waiting") {
            ("Concurrency".to_string(), "Deadlock or race condition detected")
        } else if error_lower.contains("memory") || context_lower.contains("alloc") {
            ("Memory".to_string(), "Memory-related issue")
        } else if error_lower.contains("connection") || error_lower.contains("timeout") {
            ("Network".to_string(), "Network or I/O issue")
        } else if error_lower.contains("permission") || error_lower.contains("access") {
            ("Permission".to_string(), "Access control issue")
        } else if error_lower.contains("parse") || error_lower.contains("invalid") {
            ("Data".to_string(), "Data validation or parsing issue")
        } else {
            ("Unknown".to_string(), "Unable to determine root cause")
        }
    }

    fn list_likely_causes(&self, error: &str, context: &str) -> Vec<String> {
        let mut causes = Vec::new();

        // Check common patterns
        if error.contains("unwrap") || context.contains(".unwrap()") {
            causes.push("Unwrapped Option/Result without handling None/Err case".to_string());
        }
        if error.contains("borrow") || context.contains("borrow checker") {
            causes.push("Memory borrow violation - check lifetimes".to_string());
        }
        if error.contains("thread") || context.contains("spawn") {
            causes.push("Threading issue - verify proper synchronization".to_string());
        }
        if error.contains("async") || context.contains("await") {
            causes.push("Async operation issue - check futures completion".to_string());
        }

        if causes.is_empty() {
            causes.push("Review error stack trace for more details".to_string());
        }

        causes
    }

    fn suggest_prevention(&self, error_type: &str) -> Vec<String> {
        match error_type {
            "Concurrency" => vec![
                "Use structured concurrency with join!".to_string(),
                "Implement proper lock ordering".to_string(),
                "Consider using channels instead of shared state".to_string(),
            ],
            "Memory" => vec![
                "Profile memory usage with heaptrack/dhat".to_string(),
                "Consider using arena allocators".to_string(),
                "Review large allocations".to_string(),
            ],
            "Network" => vec![
                "Implement retry with exponential backoff".to_string(),
                "Add connection pooling".to_string(),
                "Set appropriate timeouts".to_string(),
            ],
            _ => vec![
                "Add comprehensive error handling".to_string(),
                "Write unit tests for edge cases".to_string(),
                "Enable logging for debugging".to_string(),
            ],
        }
    }

    fn find_related_issues(&self, _error: &str) -> Vec<String> {
        // In a real implementation, this would search issue tracker
        Vec::new()
    }
}

impl Default for RootCauseAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct RootCauseResult {
    pub error_type: String,
    pub likely_causes: Vec<String>,
    pub immediate_fix: String,
    pub prevention: Vec<String>,
    pub related_issues: Vec<String>,
    pub stack_analysis: StackTraceAnalysis,
    pub suggestions: Vec<DebugSuggestion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_panic_error() {
        let engine = DebugEngine::new();
        let error = "thread 'main' panicked at 'called Option::unwrap() on a None value'";

        let suggestions = engine.analyze(error);
        assert!(!suggestions.is_empty());
        assert!(suggestions[0].title.contains("Panic") || suggestions[0].cause.contains("panic"));
    }

    #[test]
    fn test_analyze_timeout_error() {
        let engine = DebugEngine::new();
        let error = "connection timeout after 30 seconds";

        let suggestions = engine.analyze(error);
        assert!(!suggestions.is_empty());
        assert!(suggestions[0].title.contains("Timeout"));
    }

    #[test]
    fn test_root_cause_analysis() {
        let analyzer = RootCauseAnalyzer::new();
        let result = analyzer.analyze(
            "connection refused",
            "Error: connection refused\n   at main (src/main.rs:10)",
        );

        assert_eq!(result.error_type, "Network");
    }
}
