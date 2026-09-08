//! ExecuteLoop — Plan→Execute→Compile→Auto-Fix closed loop.
//!
//! Implements the Claude Code `/plan+` / Cursor Composer pattern:
//! 1. Decompose task into steps
//! 2. Execute each step via Agent
//! 3. After each step: cargo check
//! 4. If errors: Agent analyzes & fixes (max N rounds)
//! 5. Report final status with per-round details

use std::path::PathBuf;
use std::time::Instant;

use crate::agent::compile_fix::{CompileEngine, CompileError, PlanExecutionPipeline};
use crate::agent::Agent;

/// Configuration for the execute loop.
#[derive(Debug, Clone)]
pub struct ExecuteLoopConfig {
    /// Maximum number of auto-fix rounds (default: 3).
    pub max_fix_rounds: u32,
    /// Whether to auto-apply fixes without user confirmation (default: false).
    pub auto_apply: bool,
    /// Path to the project root (where Cargo.toml lives).
    pub project_root: PathBuf,
    /// Stop on first error instead of attempting all rounds (default: false).
    pub stop_on_first_error: bool,
}

impl Default for ExecuteLoopConfig {
    fn default() -> Self {
        Self {
            max_fix_rounds: 3,
            auto_apply: false,
            project_root: PathBuf::from("."),
            stop_on_first_error: false,
        }
    }
}

/// Status of a single execution round.
#[derive(Debug, Clone)]
pub enum RoundStatus {
    /// Compilation passed — no errors.
    Success,
    /// Compilation failed with errors.
    CompileErrors { count: usize },
    /// Agent applied one or more fixes this round.
    FixApplied { fixes: usize },
    /// Max fix rounds exceeded — some errors remain.
    MaxRoundsExceeded,
}

/// Record of one execute-fix round.
#[derive(Debug, Clone)]
pub struct RoundRecord {
    /// 1-based round number.
    pub round_number: u32,
    /// Outcome of this round.
    pub status: RoundStatus,
    /// Raw compiler output for this round.
    pub compile_output: String,
    /// Number of compile errors at the start of this round.
    pub error_count: usize,
    /// Number of fixes the agent applied during this round.
    pub fixes_applied: usize,
    /// Tokens consumed by the agent in this round.
    pub tokens_used: u32,
    /// Wall-clock duration of this round in milliseconds.
    pub duration_ms: u64,
}

/// Final result of the entire execute loop.
#[derive(Debug, Clone)]
pub struct ExecuteLoopResult {
    /// True if compilation succeeded (0 errors after all rounds).
    pub success: bool,
    /// Total rounds executed (including the initial check).
    pub total_rounds: u32,
    /// Total fixes applied across all rounds.
    pub total_fixes_applied: u32,
    /// Total tokens consumed across all agent calls.
    pub total_tokens_used: u32,
    /// Total wall-clock duration in milliseconds.
    pub total_duration_ms: u64,
    /// Number of remaining compile errors after the final round.
    pub remaining_errors: usize,
    /// Per-round records.
    pub rounds: Vec<RoundRecord>,
    /// Final compiler output (from the last cargo check).
    pub final_compile_output: String,
}

impl ExecuteLoopResult {
    /// Format a human-readable Markdown report of the entire execution.
    pub fn format_report(&self) -> String {
        let mut report = String::new();
        report.push_str("# Execute Loop Report\n\n");

        // Summary line
        let status_icon = if self.success { "✅" } else { "❌" };
        let summary_msg = if self.success {
            "All compilation errors resolved".to_string()
        } else {
            format!("{} error(s) remain", self.remaining_errors)
        };
        report.push_str(&format!(
            "**{}** {} | Rounds: {}/{} | Fixes: {} | Tokens: {} | Duration: {:.1}s\n\n",
            status_icon,
            summary_msg,
            self.total_rounds,
            self.total_rounds,
            self.total_fixes_applied,
            self.total_tokens_used,
            self.total_duration_ms as f64 / 1000.0,
        ));

        // Per-round table
        if !self.rounds.is_empty() {
            report.push_str("## Round Details\n\n");
            report.push_str("| Round | Status          | Errors | Fixes | Tokens   | Duration |\n");
            report.push_str("|-------|-----------------|--------|-------|----------|----------|\n");

            for r in &self.rounds {
                let status_str = match &r.status {
                    RoundStatus::Success => "✅ Pass".to_string(),
                    RoundStatus::CompileErrors { count } => format!("❌ Err({})", count),
                    RoundStatus::FixApplied { fixes } => format!("🔧 Fix({})", fixes),
                    RoundStatus::MaxRoundsExceeded => "⛔ Max".to_string(),
                };
                report.push_str(&format!(
                    "| {:>5} | {:<15} | {:>6} | {:>5} | {:>8} | {:>8.1}s |\n",
                    r.round_number,
                    status_str,
                    r.error_count,
                    r.fixes_applied,
                    r.tokens_used,
                    r.duration_ms as f64 / 1000.0,
                ));
            }
            report.push('\n');
        }

        // Final compiler output (truncated)
        if !self.final_compile_output.is_empty() {
            report.push_str("## Final Compiler Output\n\n");
            report.push_str("```\n");
            // Limit output length in report
            let output = if self.final_compile_output.len() > 2000 {
                format!(
                    "{}\n... ({} chars truncated)",
                    &self.final_compile_output[..2000],
                    self.final_compile_output.len() - 2000,
                )
            } else {
                self.final_compile_output.clone()
            };
            report.push_str(&output);
            report.push_str("\n```\n");
        }

        report
    }
}

/// The main execute loop controller.
///
/// Wraps the plan→execute→compile-check→auto-fix cycle into a single
/// orchestrator that tracks per-round progress and produces detailed reports.
pub struct ExecuteLoop {
    config: ExecuteLoopConfig,
}

impl ExecuteLoop {
    /// Create a new `ExecuteLoop` with the given configuration.
    pub fn new(config: ExecuteLoopConfig) -> Self {
        Self { config }
    }

    /// Create an `ExecuteLoop` with default configuration, setting only the project root.
    pub fn with_project_root(project_root: impl Into<PathBuf>) -> Self {
        Self::new(ExecuteLoopConfig {
            project_root: project_root.into(),
            ..Default::default()
        })
    }

    /// Run the full execute loop: create an internal Agent, then iterate plan→execute→compile→fix.
    ///
    /// This is a convenience entry point that owns the Agent lifecycle.
    /// For IDE integration where the Agent is managed externally, use [`run_with_agent`](Self::run_with_agent).
    pub async fn run(task_description: &str) -> anyhow::Result<ExecuteLoopResult> {
        let config = ExecuteLoopConfig::default();
        let _loop_ctl = Self::new(config);
        // Note: creating an Agent requires DeepSeekConfig + ProviderOrchestrator;
        // this method is provided for API completeness. In practice, callers should
        // use `run_with_agent` which accepts an externally-managed Agent.
        //
        // For now we delegate to run_with_agent with a stub — the caller must supply
        // a real Agent via run_with_agent or we return an informative error.
        anyhow::bail!(
            "ExecuteLoop::run() requires an external Agent. \
             Use ExecuteLoop::new(config).run_with_agent(&mut agent, task_description) instead. \
             Task description: {}",
            task_description
        )
    }

    /// Run the execute loop with an externally-owned Agent.
    ///
    /// # Workflow
    ///
    /// 1. **Baseline check** — run `cargo check` before any changes.
    /// 2. **Execute** — send `task_description` to the Agent so it performs edits.
    /// 3. **Compile check** — run `cargo check` again.
    /// 4. **Fix loop** (up to `config.max_fix_rounds`):
    ///    a. If no errors → success, break.
    ///    b. Format errors into a structured prompt.
    ///    c. Call `agent.process(fix_prompt)` — Agent uses its tool_calls to edit files.
    ///    d. Record round result.
    ///    e. Re-run `cargo check`.
    /// 5. Return `ExecuteLoopResult` with all round records.
    pub async fn run_with_agent(
        &self,
        agent: &mut Agent,
        task_description: &str,
    ) -> anyhow::Result<ExecuteLoopResult> {
        let project_root = self
            .config
            .project_root
            .to_string_lossy()
            .to_string();
        let engine = CompileEngine::new(&project_root);
        let _pipeline = PlanExecutionPipeline::new(&project_root);

        let overall_start = Instant::now();
        let mut rounds: Vec<RoundRecord> = Vec::new();
        let mut total_tokens: u32 = 0;
        let mut total_fixes: u32 = 0;

        tracing::info!(
            project_root = %self.config.project_root.display(),
            max_rounds = self.config.max_fix_rounds,
            auto_apply = self.config.auto_apply,
            "ExecuteLoop started"
        );

        // ── Step 0: Baseline compile check ──
        let baseline_start = Instant::now();
        let baseline_result = engine.check();
        let baseline_duration = baseline_start.elapsed().as_millis() as u64;

        let baseline_record = RoundRecord {
            round_number: 0,
            status: if baseline_result.success {
                RoundStatus::Success
            } else {
                RoundStatus::CompileErrors {
                    count: baseline_result.errors.len(),
                }
            },
            compile_output: baseline_result.output.clone(),
            error_count: baseline_result.errors.len(),
            fixes_applied: 0,
            tokens_used: 0,
            duration_ms: baseline_duration,
        };
        rounds.push(baseline_record);

        tracing::info!(
            errors = baseline_result.errors.len(),
            warnings = baseline_result.warnings,
            success = baseline_result.success,
            "Baseline compile check complete"
        );

        // If baseline already passes, nothing to do
        if baseline_result.success {
            return Ok(ExecuteLoopResult {
                success: true,
                total_rounds: 1,
                total_fixes_applied: 0,
                total_tokens_used: 0,
                total_duration_ms: overall_start.elapsed().as_millis() as u64,
                remaining_errors: 0,
                rounds,
                final_compile_output: baseline_result.output,
            });
        }

        // ── Step 1: Initial task execution via Agent ──
        let exec_start = Instant::now();
        let exec_turn = agent.process(task_description).await?;
        total_tokens += exec_turn.total_tokens;
        let _exec_duration = exec_start.elapsed().as_millis() as u64;

        tracing::info!(
            tokens = exec_turn.total_tokens,
            iterations = exec_turn.iterations,
            provider = %exec_turn.provider,
            "Initial task execution complete"
        );

        // ── Step 2: Post-execution compile check ──
        let post_exec_result = engine.check();
        if post_exec_result.success {
            tracing::info!("Compilation passed after initial execution");
            return Ok(ExecuteLoopResult {
                success: true,
                total_rounds: 2,
                total_fixes_applied: 0,
                total_tokens_used: total_tokens,
                total_duration_ms: overall_start.elapsed().as_millis() as u64,
                remaining_errors: 0,
                rounds,
                final_compile_output: post_exec_result.output,
            });
        }

        // ── Step 3: Auto-fix loop ──
        let mut last_result = post_exec_result;

        for round_num in 1..=self.config.max_fix_rounds {
            let round_start = Instant::now();

            tracing::info!(
                round = round_num,
                max_rounds = self.config.max_fix_rounds,
                errors = last_result.errors.len(),
                "Starting auto-fix round"
            );

            // Build fix prompt from current errors
            let fix_prompt = format_fix_prompt(&last_result.errors, round_num);

            // Send to Agent for analysis & fix
            let fix_turn = agent.process(&fix_prompt).await?;
            total_tokens += fix_turn.total_tokens;

            let round_duration = round_start.elapsed().as_millis() as u64;

            // Re-compile to see if fixed
            let recheck = engine.check();

            // Determine how many fixes were applied (heuristic: reduction in error count)
            let prev_error_count = last_result.errors.len();
            let new_error_count = recheck.errors.len();
            let fixes_this_round = if new_error_count < prev_error_count {
                prev_error_count.saturating_sub(new_error_count)
            } else {
                0
            };
            total_fixes += fixes_this_round as u32;

            let round_status = if recheck.success {
                RoundStatus::Success
            } else if fixes_this_round > 0 {
                RoundStatus::FixApplied {
                    fixes: fixes_this_round,
                }
            } else if round_num == self.config.max_fix_rounds {
                RoundStatus::MaxRoundsExceeded
            } else {
                RoundStatus::CompileErrors {
                    count: new_error_count,
                }
            };

            let record = RoundRecord {
                round_number: round_num,
                status: round_status.clone(),
                compile_output: recheck.output.clone(),
                error_count: new_error_count,
                fixes_applied: fixes_this_round,
                tokens_used: fix_turn.total_tokens,
                duration_ms: round_duration,
            };
            rounds.push(record);

            tracing::info!(
                round = round_num,
                status = ?round_status,
                errors_before = prev_error_count,
                errors_after = new_error_count,
                fixes = fixes_this_round,
                tokens = fix_turn.total_tokens,
                "Auto-fix round complete"
            );

            // If compiled successfully or stop_on_first_error with no progress
            if recheck.success {
                return Ok(ExecuteLoopResult {
                    success: true,
                    total_rounds: round_num + 1, // +1 for baseline round
                    total_fixes_applied: total_fixes,
                    total_tokens_used: total_tokens,
                    total_duration_ms: overall_start.elapsed().as_millis() as u64,
                    remaining_errors: 0,
                    rounds,
                    final_compile_output: recheck.output,
                });
            }

            // Stop early if configured and no fixes were applied
            if self.config.stop_on_first_error && fixes_this_round == 0 {
                tracing::warn!(
                    round = round_num,
                    "stop_on_first_error enabled, no progress made — stopping"
                );
                break;
            }

            last_result = recheck;
        }

        // Exhausted all rounds
        let final_check = engine.check();
        tracing::error!(
            rounds_executed = self.config.max_fix_rounds,
            remaining_errors = final_check.errors.len(),
            "Auto-fix loop exhausted"
        );

        Ok(ExecuteLoopResult {
            success: false,
            total_rounds: self.config.max_fix_rounds + 1, // +1 for baseline
            total_fixes_applied: total_fixes,
            total_tokens_used: total_tokens,
            total_duration_ms: overall_start.elapsed().as_millis() as u64,
            remaining_errors: final_check.errors.len(),
            rounds,
            final_compile_output: final_check.output,
        })
    }

    /// Run the execute loop using the `PlanExecutionPipeline` for multi-step plans.
    ///
    /// Accepts a list of step descriptions; executes each step through the pipeline,
    /// running compile+auto-fix between steps.
    pub async fn run_plan(
        &self,
        agent: &mut Agent,
        steps: &[String],
    ) -> anyhow::Result<ExecuteLoopResult> {
        let project_root = self
            .config
            .project_root
            .to_string_lossy()
            .to_string();
        let pipeline = PlanExecutionPipeline::new(&project_root);

        let overall_start = Instant::now();
        let mut rounds: Vec<RoundRecord> = Vec::new();
        let mut total_tokens: u32 = 0;
        let mut total_fixes: u32 = 0;

        // Baseline
        let engine = CompileEngine::new(&project_root);
        let baseline = engine.check();
        let baseline_record = RoundRecord {
            round_number: 0,
            status: if baseline.success {
                RoundStatus::Success
            } else {
                RoundStatus::CompileErrors {
                    count: baseline.errors.len(),
                }
            },
            compile_output: baseline.output.clone(),
            error_count: baseline.errors.len(),
            fixes_applied: 0,
            tokens_used: 0,
            duration_ms: 0,
        };
        rounds.push(baseline_record);

        if baseline.success && steps.is_empty() {
            return Ok(ExecuteLoopResult {
                success: true,
                total_rounds: 1,
                total_fixes_applied: 0,
                total_tokens_used: 0,
                total_duration_ms: overall_start.elapsed().as_millis() as u64,
                remaining_errors: 0,
                rounds,
                final_compile_output: baseline.output,
            });
        }

        // Execute each step through the pipeline
        for (step_idx, step_desc) in steps.iter().enumerate() {
            let step_start = Instant::now();
            tracing::info!(step = step_idx + 1, total = steps.len(), desc = %step_desc, "Executing plan step");

            // Execute step via pipeline (which handles internal compile-fix)
            let _step_report = pipeline.execute_step(agent, step_desc).await?;
            let step_duration = step_start.elapsed().as_millis() as u64;

            // Post-step compile check
            let mut post_step = engine.check();
            let record = RoundRecord {
                round_number: (step_idx + 1) as u32,
                status: if post_step.success {
                    RoundStatus::Success
                } else {
                    RoundStatus::CompileErrors {
                        count: post_step.errors.len(),
                    }
                },
                compile_output: post_step.output.clone(),
                error_count: post_step.errors.len(),
                fixes_applied: 0, // Pipeline tracks this internally
                tokens_used: 0,    // Pipeline doesn't expose token counts per step
                duration_ms: step_duration,
            };
            rounds.push(record);

            // If step introduced errors, run auto-fix loop
            if !post_step.success {
                for fix_round in 1..=self.config.max_fix_rounds {
                    let fix_start = Instant::now();
                    let fix_prompt = format_fix_prompt(&post_step.errors, fix_round);
                    let fix_turn = agent.process(&fix_prompt).await?;
                    total_tokens += fix_turn.total_tokens;

                    let recheck = engine.check();
                    let prev_errs = post_step.errors.len();
                    let new_errs = recheck.errors.len();
                    let fixes = prev_errs.saturating_sub(new_errs);
                    total_fixes += fixes as u32;

                    let fix_record = RoundRecord {
                        round_number: (step_idx + 1 + fix_round as usize) as u32,
                        status: if recheck.success {
                            RoundStatus::Success
                        } else if fixes > 0 {
                            RoundStatus::FixApplied { fixes }
                        } else if fix_round == self.config.max_fix_rounds {
                            RoundStatus::MaxRoundsExceeded
                        } else {
                            RoundStatus::CompileErrors { count: new_errs }
                        },
                        compile_output: recheck.output.clone(),
                        error_count: new_errs,
                        fixes_applied: fixes,
                        tokens_used: fix_turn.total_tokens,
                        duration_ms: fix_start.elapsed().as_millis() as u64,
                    };
                    rounds.push(fix_record);

                    if recheck.success || (self.config.stop_on_first_error && fixes == 0) {
                        break;
                    }
                    post_step = recheck;
                }
            }
        }

        // Final compile
        let final_result = engine.check();
        Ok(ExecuteLoopResult {
            success: final_result.success,
            total_rounds: rounds.len() as u32,
            total_fixes_applied: total_fixes,
            total_tokens_used: total_tokens,
            total_duration_ms: overall_start.elapsed().as_millis() as u64,
            remaining_errors: final_result.errors.len(),
            rounds,
            final_compile_output: final_result.output,
        })
    }
}

/// Generate a structured fix prompt from compilation errors for a given round.
///
/// The prompt instructs the Agent to analyze specific errors and apply targeted fixes
/// using its available tools (`write_file`, `edit_file`, etc.).
pub fn format_fix_prompt(errors: &[CompileError], round: u32) -> String {
    if errors.is_empty() {
        return String::from("No compilation errors to fix.");
    }

    let mut prompt = String::new();
    prompt.push_str(&format!(
        "## Auto-Fix Round {}\n\n\
         The code has compilation errors that need to be fixed.\n\n\
         ### Errors ({})\n\n",
        round,
        errors.len()
    ));

    for (i, err) in errors.iter().enumerate().take(10) {
        prompt.push_str(&format!(
            "{}. **{}**:{}:{} — `{}`\n",
            i + 1,
            if err.file.is_empty() { "<unknown>" } else { &err.file },
            err.line,
            err.column,
            err.message
        ));
        if let Some(ref code) = err.code {
            prompt.push_str(&format!("   Error code: `{}`\n", code));
        }
    }

    if errors.len() > 10 {
        prompt.push_str(&format!(
            "... and {} more errors.\n",
            errors.len() - 10
        ));
    }

    prompt.push_str(
        "\n### Instructions\n\n\
         1. Read the affected files to understand context.\n\
         2. Apply the minimal fix needed for EACH error above.\n\
         3. Use `write_file` or `edit_file` tools to make changes.\n\
         4. Do NOT refactor unrelated code.\n\
         5. After fixing, confirm what changes you made.\n\n\
         Fix ALL errors now.",
    );

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = ExecuteLoopConfig::default();
        assert_eq!(cfg.max_fix_rounds, 3);
        assert!(!cfg.auto_apply);
        assert!(!cfg.stop_on_first_error);
    }

    #[test]
    fn test_format_fix_prompt_empty() {
        let prompt = format_fix_prompt(&[], 1);
        assert_eq!(prompt, "No compilation errors to fix.");
    }

    #[test]
    fn test_format_fix_prompt_with_errors() {
        let errors = vec![
            CompileError {
                file: "src/main.rs".to_string(),
                line: 42,
                column: 5,
                code: Some("E0308".to_string()),
                message: "mismatched types".to_string(),
                raw: String::new(),
            },
            CompileError {
                file: "src/lib.rs".to_string(),
                line: 10,
                column: 1,
                code: None,
                message: "expected fn, found struct".to_string(),
                raw: String::new(),
            },
        ];
        let prompt = format_fix_prompt(&errors, 2);
        assert!(prompt.contains("Round 2"));
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("mismatched types"));
        assert!(prompt.contains("E0308"));
        assert!(prompt.contains("src/lib.rs"));
        assert!(prompt.contains("expected fn, found struct"));
        assert!(prompt.contains("write_file"));
        assert!(prompt.contains("edit_file"));
    }

    #[test]
    fn test_execute_loop_new() {
        let cfg = ExecuteLoopConfig {
            max_fix_rounds: 5,
            ..Default::default()
        };
        let loop_ctl = ExecuteLoop::new(cfg);
        assert_eq!(loop_ctl.config.max_fix_rounds, 5);
    }

    #[test]
    fn test_execute_loop_with_project_root() {
        let loop_ctl = ExecuteLoop::with_project_root("/tmp/project");
        assert_eq!(
            loop_ctl.config.project_root,
            PathBuf::from("/tmp/project")
        );
        assert_eq!(loop_ctl.config.max_fix_rounds, 3); // default
    }

    #[test]
    fn test_round_status_display_variants() {
        // Just verify all variants are constructible
        let _ = RoundStatus::Success;
        let _ = RoundStatus::CompileErrors { count: 3 };
        let _ = RoundStatus::FixApplied { fixes: 2 };
        let _ = RoundStatus::MaxRoundsExceeded;
    }

    #[test]
    fn test_format_report_success() {
        let result = ExecuteLoopResult {
            success: true,
            total_rounds: 2,
            total_fixes_applied: 1,
            total_tokens_used: 1500,
            total_duration_ms: 2500,
            remaining_errors: 0,
            rounds: vec![
                RoundRecord {
                    round_number: 0,
                    status: RoundStatus::CompileErrors { count: 3 },
                    compile_output: "error[E0308]".to_string(),
                    error_count: 3,
                    fixes_applied: 0,
                    tokens_used: 0,
                    duration_ms: 100,
                },
                RoundRecord {
                    round_number: 1,
                    status: RoundStatus::FixApplied { fixes: 3 },
                    compile_output: String::new(),
                    error_count: 0,
                    fixes_applied: 3,
                    tokens_used: 1500,
                    duration_ms: 2400,
                },
            ],
            final_compile_output: String::new(),
        };
        let report = result.format_report();
        assert!(report.contains("✅"));
        assert!(report.contains("Round Details"));
        assert!(report.contains("Fix(3)"));
        // Summary line should show success
        assert!(report.starts_with("# Execute Loop Report\n\n**✅**"));
    }

    #[test]
    fn test_format_report_failure() {
        let result = ExecuteLoopResult {
            success: false,
            total_rounds: 4,
            total_fixes_applied: 1,
            total_tokens_used: 3000,
            total_duration_ms: 5000,
            remaining_errors: 2,
            rounds: vec![RoundRecord {
                round_number: 3,
                status: RoundStatus::MaxRoundsExceeded,
                compile_output: "error: unresolved import".to_string(),
                error_count: 2,
                fixes_applied: 0,
                tokens_used: 500,
                duration_ms: 1000,
            }],
            final_compile_output: "error[E0432]: unresolved import".to_string(),
        };
        let report = result.format_report();
        assert!(report.contains("❌"));
        assert!(report.contains("2 error(s) remain"));
        assert!(report.contains("Max"));
        assert!(report.contains("Final Compiler Output"));
    }
}
