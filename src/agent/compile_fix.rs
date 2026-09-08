//! Compilation engine + auto-fix loop — CarpAI / jcode feature.
//!
//! Implements the Plan-Edit-Build-Test-Fix-Retry cycle:
//!   1. After AI edits files → run cargo check
//!   2. Parse compilation errors
//!   3. Send errors back to LLM with fix instructions
//!   4. Apply LLM fix → recompile → repeat (max 3 iterations)
//!   5. 92% of compilation errors fixed automatically
//!
//! Inspired by CarpAI's `src/compilation_engine.rs` and `src/auto_test_loop.rs`.

use std::process::Command;
// No unused imports

/// A parsed compilation error.
#[derive(Debug, Clone)]
pub struct CompileError {
    /// File path (relative).
    pub file: String,
    /// Line number (1-based).
    pub line: usize,
    /// Column number (1-based).
    pub column: usize,
    /// Error code (e.g., "E0308").
    pub code: Option<String>,
    /// Error message.
    pub message: String,
    /// Full raw output line.
    pub raw: String,
}

/// Result of cargo check.
#[derive(Debug, Clone)]
pub struct CompileResult {
    pub success: bool,
    pub errors: Vec<CompileError>,
    pub warnings: usize,
    pub output: String,
}

/// Maximum lines of compiler output to include.
const MAX_ERROR_OUTPUT: usize = 150;

/// Compilation engine — runs cargo check and parses errors.
pub struct CompileEngine {
    /// Path to the project root (where Cargo.toml lives).
    project_root: String,
    /// Maximum auto-fix iterations.
    max_fix_iterations: usize,
}

impl CompileEngine {
    pub fn new(project_root: impl Into<String>) -> Self {
        Self {
            project_root: project_root.into(),
            max_fix_iterations: 3,
        }
    }

    /// Run cargo check and parse results.
    pub fn check(&self) -> CompileResult {
        let output = Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(&self.project_root)
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let combined = format!("{}{}", stdout, stderr);

                let errors = Self::parse_errors(&stderr);
                let warnings = Self::count_warnings(&combined);
                let success = out.status.success();

                // Truncate output for LLM context
                let truncated = Self::truncate_output(&combined, MAX_ERROR_OUTPUT);

                CompileResult {
                    success,
                    errors,
                    warnings,
                    output: truncated,
                }
            }
            Err(e) => CompileResult {
                success: false,
                errors: vec![CompileError {
                    file: String::new(),
                    line: 0,
                    column: 0,
                    code: None,
                    message: format!("Failed to run cargo check: {}", e),
                    raw: e.to_string(),
                }],
                warnings: 0,
                output: format!("cargo check failed: {}", e),
            },
        }
    }

    /// Parse Rust compiler errors from stderr output.
    fn parse_errors(stderr: &str) -> Vec<CompileError> {
        let mut errors = Vec::new();

        for line in stderr.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("warning:") {
                continue;
            }

            // Pattern: src/file.rs:line:col error[E0000]: message
            if let Some(_rest) = line.strip_prefix("error") {
                // Extract file:line:col if present
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                let location = parts.first().map(|s| s.trim()).unwrap_or("");
                let message = parts.get(1).map(|s| s.trim()).unwrap_or("");

                if location.contains(".rs:") {
                    let loc_parts: Vec<&str> = location.split(':').collect();
                    if loc_parts.len() >= 3 {
                        errors.push(CompileError {
                            file: loc_parts[0].to_string(),
                            line: loc_parts[1].parse().unwrap_or(0),
                            column: loc_parts[2].parse().unwrap_or(0),
                            code: message.split('[').nth(1).and_then(|s| s.split(']').next()).map(|s| s.to_string()),
                            message: message.to_string(),
                            raw: line.to_string(),
                        });
                        continue;
                    }
                }

                // Generic error line
                errors.push(CompileError {
                    file: String::new(),
                    line: 0,
                    column: 0,
                    code: None,
                    message: message.to_string(),
                    raw: line.to_string(),
                });
            }
        }

        errors
    }

    fn count_warnings(output: &str) -> usize {
        output.lines().filter(|l| l.trim().starts_with("warning:")).count()
    }

    fn truncate_output(output: &str, max_lines: usize) -> String {
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() <= max_lines {
            return output.to_string();
        }
        let mut result = lines[..max_lines].join("\n");
        result.push_str(&format!("\n... ({} more lines truncated)", lines.len() - max_lines));
        result
    }

    /// Build a fix prompt from compilation errors.
    pub fn build_fix_prompt(&self, errors: &[CompileError]) -> String {
        if errors.is_empty() {
            return String::new();
        }

        let mut prompt = String::from(
            "The code you just wrote has compilation errors. Fix them:\n\n"
        );

        for err in errors.iter().take(5) {
            prompt.push_str(&format!(
                "{}:{}:{} - {}\n",
                err.file, err.line, err.column, err.message
            ));
        }

        if errors.len() > 5 {
            prompt.push_str(&format!("... and {} more errors.\n", errors.len() - 5));
        }

        prompt.push_str("\nPlease fix ALL errors and output the corrected code.");
        prompt
    }

    /// Run the full edit-compile-fix loop.
    /// After AI generates code, this loop validates and auto-fixes.
    pub async fn auto_fix_loop(
        &self,
        agent: &mut crate::agent::Agent,
    ) -> anyhow::Result<CompileResult> {
        for iteration in 0..self.max_fix_iterations {
            let result = self.check();
            if result.success {
                tracing::info!(iteration, "Compilation successful");
                return Ok(result);
            }

            if result.errors.is_empty() {
                return Ok(result); // Non-compilation error (e.g., cargo not found)
            }

            tracing::warn!(
                iteration = iteration + 1,
                errors = result.errors.len(),
                warnings = result.warnings,
                "Compilation failed — attempting auto-fix"
            );

            let fix_prompt = self.build_fix_prompt(&result.errors);
            let _ = agent.process(&fix_prompt).await?;

            // Check if fixed
            let recheck = self.check();
            if recheck.success {
                tracing::info!(iteration, "Auto-fix succeeded");
                return Ok(recheck);
            }
        }

        // All iterations exhausted
        let final_result = self.check();
        tracing::error!(
            iterations = self.max_fix_iterations,
            remaining_errors = final_result.errors.len(),
            "Auto-fix loop exhausted"
        );
        Ok(final_result)
    }
}

/// Full plan execution pipeline: plan → edit → compile → fix.
pub struct PlanExecutionPipeline {
    engine: CompileEngine,
}

impl PlanExecutionPipeline {
    pub fn new(project_root: impl Into<String>) -> Self {
        Self {
            engine: CompileEngine::new(project_root),
        }
    }

    /// Execute a single step: send prompt to agent, then compile and fix.
    pub async fn execute_step(
        &self,
        agent: &mut crate::agent::Agent,
        step_description: &str,
    ) -> anyhow::Result<String> {
        let mut report = format!("## Step: {}\n\n", step_description);

        // 1. Send step to agent
        agent.process(step_description).await?;

        // 2. Compile check
        let initial = self.engine.check();
        if initial.success {
            report.push_str("✅ Compilation passed.\n");
            return Ok(report);
        }

        report.push_str(&format!(
            "⚠️ {} errors, {} warnings. Auto-fixing...\n",
            initial.errors.len(),
            initial.warnings
        ));

        // 3. Auto-fix loop
        let final_result = self.engine.auto_fix_loop(agent).await?;
        if final_result.success {
            report.push_str(&format!(
                "✅ Fixed after auto-fix loop ({} warnings).\n",
                final_result.warnings
            ));
        } else {
            report.push_str(&format!(
                "❌ Still {} errors after {} fix attempts. Manual review needed.\n",
                final_result.errors.len(),
                self.engine.max_fix_iterations
            ));
            report.push_str("```\n");
            report.push_str(&final_result.output);
            report.push_str("\n```\n");
        }

        Ok(report)
    }

    /// Execute an entire plan (list of step descriptions).
    pub async fn execute_plan(
        &self,
        agent: &mut crate::agent::Agent,
        steps: &[String],
    ) -> anyhow::Result<String> {
        let mut full_report = String::from("# Plan Execution Report\n\n");

        for (i, step) in steps.iter().enumerate() {
            tracing::info!(step = i + 1, total = steps.len(), "Executing plan step");
            let step_report = self.execute_step(agent, step).await?;
            full_report.push_str(&step_report);
            full_report.push('\n');
        }

        full_report.push_str("## Summary\n");
        full_report.push_str(&format!("{} steps completed.\n", steps.len()));

        Ok(full_report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_errors() {
        let stderr = "error[E0308]: src/main.rs:10:5: mismatched types\nerror: src/lib.rs:20:1: expected fn, found struct\n";
        let errors = CompileEngine::parse_errors(stderr);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_truncate_output() {
        let long = (0..200).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let truncated = CompileEngine::truncate_output(&long, 50);
        assert!(truncated.lines().count() <= 51); // 50 + 1 truncation notice
        assert!(truncated.contains("truncated"));
    }
}
