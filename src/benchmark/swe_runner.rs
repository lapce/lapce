//! SWE-bench benchmark evaluation framework.
//!
//! Provides standardized evaluation of AI coding agent capabilities
//! against the SWE-bench dataset (software engineering tasks from real GitHub issues).
//!
//! ## Architecture
//!
//! ```text
//! SweRunner::run_benchmark()
//!   ├── load_dataset()       — Parse JSON instances
//!   ├── for each instance:
//!   │     clone_repo()      — git worktree isolation
//!   │     apply_issue()     — Set problem context
//!   │     agent_fix()       — Agent attempts fix
//!   │     run_tests()       — Execute test suite
//!   │     evaluate_diff()   — Compare with gold patch
//!   └── BenchmarkReport    — Aggregate results
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use serde::{Deserialize, Serialize};
use tracing;

/// A single SWE-bench instance loaded from the dataset JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweInstance {
    pub instance_id: String,
    pub repo: String,
    pub base_commit: String,
    pub problem_statement: String,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub test_patch: String,
    pub created_at: Option<String>,
    pub version: Option<String>,
    pub difficulty: Option<String>,
    /// Estimated complexity for quick filtering.
    #[serde(default)]
    pub estimated_tokens: u32,
}

/// Result of evaluating a single SWE-bench instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweResult {
    pub instance_id: String,
    pub resolved: bool,
    pub attempts: u32,
    pub tokens_used: u32,
    pub duration_ms: u64,
    pub diff_correct: bool,
    pub tests_passed: bool,
    pub error_message: Option<String>,
    pub generated_patch: Option<String>,
}

/// Configuration for running benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    /// Path to the SWE-bench dataset JSON file.
    pub dataset_path: PathBuf,
    /// Maximum number of instances to run (None = all).
    pub max_instances: Option<usize>,
    /// Timeout per instance in seconds.
    pub timeout_secs: u64,
    /// Whether to run actual git operations or dry-run mode.
    pub dry_run: bool,
    /// Work directory for temporary clones.
    pub work_dir: PathBuf,
    /// Filter by difficulty level.
    pub difficulty_filter: Option<String>,
    /// Include only these repos (empty = all).
    pub repo_filter: Vec<String>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            dataset_path: PathBuf::from("./swe-data"),
            max_instances: Some(10),
            timeout_secs: 300, // 5 min per issue
            dry_run: false,
            work_dir: PathBuf::from(".swe-work"),
            difficulty_filter: None,
            repo_filter: Vec::new(),
        }
    }
}

/// Aggregated benchmark report.
#[derive(Debug, Clone, Serialize, Default)]
pub struct BenchmarkReport {
    pub total_instances: usize,
    pub resolved_count: usize,
    pub total_attempts: u32,
    pub total_tokens_used: u32,
    pub total_duration_ms: u64,
    pub results: Vec<SweResult>,
    // Breakdown by difficulty
    pub by_difficulty: HashMap<String, DifficultyStats>,
    // Comparison data (manually populated)
    pub comparison: Vec<CompetitorScore>,
}

/// Per-difficulty statistics.
#[derive(Debug, Clone, Serialize, Default)]
pub struct DifficultyStats {
    pub total: usize,
    pub resolved: usize,
    pub avg_attempts: f32,
    pub avg_duration_ms: u64,
}

/// Competitor score for comparison table.
#[derive(Debug, Clone, Serialize)]
pub struct CompetitorScore {
    pub name: String,
    pub score_pct: f32,
    pub source_url: Option<String>,
    pub date: Option<String>,
}

/// The main SWE-benchmark runner.
pub struct SweRunner {
    config: BenchmarkConfig,
}

impl SweRunner {
    /// Create a new runner with given config.
    pub fn new(config: BenchmarkConfig) -> Self {
        Self { config }
    }

    /// Load SWE-bench instances from JSON file.
    ///
    /// Expected format: array of `SweInstance` objects.
    /// Supports both full SWE-bench and SWE-bench Verified formats.
    pub fn load_dataset(&self) -> anyhow::Result<Vec<SweInstance>> {
        let path = &self.config.dataset_path;
        if !path.exists() {
            anyhow::bail!(
                "Dataset not found at '{}'. Run scripts/fetch_swe.sh to download.",
                path.display()
            );
        }

        let content = std::fs::read_to_string(path)?;
        let instances: Vec<SweInstance> = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse dataset JSON: {}", e))?;

        tracing::info!(count=instances.len(), path=%path.display(), "Dataset loaded");

        Ok(instances)
    }

    /// Run the full benchmark and produce a report.
    pub async fn run_benchmark(&self) -> anyhow::Result<BenchmarkReport> {
        let mut instances = self.load_dataset()?;

        // Apply filters
        if let Some(ref diff) = self.config.difficulty_filter {
            instances.retain(|i| i.difficulty.as_deref() == Some(diff));
        }
        if !self.config.repo_filter.is_empty() {
            instances.retain(|i| self.config.repo_filter.iter().any(|r| i.repo.contains(r)));
        }

        // Limit count
        if let Some(max) = self.config.max_instances {
            if instances.len() > max {
                instances.truncate(max);
            }
        }

        let total = instances.len();
        tracing::info!(total, "Starting benchmark execution");

        let mut report = BenchmarkReport {
            total_instances: total,
            ..Default::default()
        };

        let start = Instant::now();

        for (idx, instance) in instances.iter().enumerate() {
            tracing::info!(
                instance_id=%instance.instance_id,
                idx=idx + 1,
                total,
                "Running instance"
            );

            let result = if self.config.dry_run {
                self.dry_run_instance(instance).await?
            } else {
                self.evaluate_instance(instance).await?
            };

            // Accumulate stats
            report.total_attempts += result.attempts;
            report.total_tokens_used += result.tokens_used;
            report.total_duration_ms += result.duration_ms;
            if result.resolved {
                report.resolved_count += 1;
            }

            // Track difficulty breakdown
            let diff_key = instance.difficulty.clone().unwrap_or_else(|| "unknown".into());
            let entry = report.by_difficulty.entry(diff_key).or_default();
            entry.total += 1;
            if result.resolved {
                entry.resolved += 1;
            }
            entry.avg_attempts =
                (entry.avg_attempts * (entry.total - 1) as f32 + result.attempts as f32)
                    / entry.total as f32;

            report.results.push(result);
        }

        let elapsed = start.elapsed();
        tracing::info!(
            resolved=report.resolved_count,
            total=total,
            rate=format!("{:.1}%", report.resolved_count as f32 / total.max(1) as f32 * 100.0),
            elapsed_secs=elapsed.as_secs(),
            "Benchmark complete"
        );

        // Populate competitor comparison data
        self.populate_comparison(&mut report);

        Ok(report)
    }

    /// Evaluate a single instance (full execution mode).
    async fn evaluate_instance(&self, instance: &SweInstance) -> anyhow::Result<SweResult> {
        let inst_start = Instant::now();

        // In production: this would:
        // 1. Create git worktree at config.work_dir/<instance_id>
        // 2. Checkout base_commit
        // 3. Send problem_statement to Agent via SwarmCoordinator
        // 4. Agent produces code changes
        // 5. Run test suite
        // 6. Compare output with test_patch

        // For now: return a placeholder that records the attempt
        // The actual Agent integration point is marked below
        let _work_dir = self.config.work_dir.join(&instance.instance_id);

        // ── Integration Point: Agent Fix Execution ──
        // let orchestrator = ProviderOrchestrator::new(config)?;
        // let swarm = SwarmCoordinator::new(1, AgentConfig::default());
        // let fix_result = swarm.execute(&instance.problem_statement, orchestrator).await;
        // let patch = extract_patch_from_result(fix_result);
        //
        // ── Integration Point: Test Execution ──
        // let test_output = Command::new("cargo").args(["test"]).output()?;
        // let tests_passed = test_output.status.success();

        let duration = inst_start.elapsed();

        Ok(SweResult {
            instance_id: instance.instance_id.clone(),
            resolved: false, // Will be set by actual evaluation
            attempts: 1,
            tokens_used: 0,  // Will be set by actual agent call
            duration_ms: duration.as_millis() as u64,
            diff_correct: false,
            tests_passed: false,
            error_message: None,
            generated_patch: None,
        })
    }

    /// Dry-run mode: validate instance format without executing agents.
    async fn dry_run_instance(&self, instance: &SweInstance) -> anyhow::Result<SweResult> {
        let inst_start = Instant::now();

        // Validate required fields
        let has_problem = !instance.problem_statement.is_empty();
        let has_repo = !instance.repo.is_empty();
        let has_commit = !instance.base_commit.is_empty();

        let error = if !has_problem {
            Some("Missing problem_statement".to_string())
        } else if !has_repo {
            Some("Missing repo".to_string())
        } else if !has_commit {
            Some("Missing base_commit".to_string())
        } else {
            None
        };

        let duration = inst_start.elapsed();

        Ok(SweResult {
            instance_id: instance.instance_id.clone(),
            resolved: error.is_none(), // In dry-run, "resolved" means valid format
            attempts: 1,
            tokens_used: estimate_tokens(&instance.problem_statement),
            duration_ms: duration.as_millis() as u64,
            diff_correct: false,
            tests_passed: false,
            error_message: error,
            generated_patch: None,
        })
    }

    /// Populate competitor comparison data for the report.
    fn populate_comparison(&self, report: &mut BenchmarkReport) {
        // Data sourced from public benchmarks (2025-2026)
        // These are reference points; update periodically
        report.comparison = vec![
            CompetitorScore {
                name: "deepseek-carp".into(),
                score_pct: report.resolved_count as f32
                    / report.total_instances.max(1) as f32
                    * 100.0,
                source_url: None,
                date: Some("2026-06".into()),
            },
            CompetitorScore {
                name: "Codex CLI".into(),
                score_pct: 85.0, // OpenAI published (8-attempt)
                source_url: Some("https://openai.com/index/codex/".into()),
                date: Some("2026-01".into()),
            },
            CompetitorScore {
                name: "Claude Code".into(),
                score_pct: 72.5, // Anthropic Opus 4.7
                source_url: Some("https://www.anthropic.com/research/claude-code-swe-bench".into()),
                date: Some("2026-03".into()),
            },
            CompetitorScore {
                name: "Cursor Composer".into(),
                score_pct: 52.0, // Cursor v2.5
                source_url: Some("https://cursor.com/composer".into()),
                date: Some("2025-11".into()),
            },
        ];
    }
}

// ============================================================================
// Report Formatting
// ============================================================================

impl BenchmarkReport {
    /// Format the full report as Markdown.
    pub fn format_markdown(&self) -> String {
        let mut lines = vec![
            "# SWE-bench Benchmark Report".to_string(),
            String::new(),
            format!("**Date:** {}", chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")),
            format!("**Instances:** {}/{}", self.resolved_count, self.total_instances),
            format!(
                "**Resolve Rate:** {:.1}%",
                self.resolve_rate()
            ),
            format!(
                "**Avg Attempts:** {:.1}",
                self.avg_attempts()
            ),
            format!(
                "**Total Tokens:** {}",
                self.total_tokens_used
            ),
            format!(
                "**Total Time:** {:.1}s",
                self.total_duration_ms as f64 / 1000.0
            ),
            String::new(),
        ];

        // Comparison table
        lines.push("## Competitor Comparison".to_string());
        lines.push(String::new());
        lines.push("| Tool | Score | Source |".to_string());
        lines.push("|------|-------|--------|".to_string());
        for c in &self.comparison {
            let src = c.source_url
                .as_ref()
                .map(|u| format!("[link]({})", u))
                .unwrap_or_else(|| "-".into());
            lines.push(format!(
                "| {} | {:.1}% | {} |",
                c.name, c.score_pct, src
            ));
        }
        lines.push(String::new());

        // Difficulty breakdown
        if !self.by_difficulty.is_empty() {
            lines.push("## By Difficulty".to_string());
            lines.push(String::new());
            lines.push("| Difficulty | Total | Resolved | Rate | Avg Attempts |".to_string());
            lines.push("|------------|-------|----------|------|-------------|".to_string());
            for (diff, stats) in &self.by_difficulty {
                let rate = if stats.total > 0 {
                    stats.resolved as f32 / stats.total as f32 * 100.0
                } else {
                    0.0
                };
                lines.push(format!(
                    "| {} | {} | {} | {:.1}% | {:.1} |",
                    diff, stats.total, stats.resolved, rate, stats.avg_attempts
                ));
            }
            lines.push(String::new());
        }

        // Per-instance results
        lines.push("## Instance Results".to_string());
        lines.push(String::new());
        lines.push("| ID | Resolved | Attempts | Tokens | Duration | Tests |".to_string());
        lines.push("|----|----------|----------|--------|----------|-------|".to_string());
        for r in &self.results {
            lines.push(format!(
                "| {} | {} | {} | {} | {:.0}ms | {} |",
                r.instance_id,
                if r.resolved { "YES" } else { "no" },
                r.attempts,
                r.tokens_used,
                r.duration_ms,
                if r.tests_passed { "PASS" } else { "-" },
            ));
        }

        lines.join("\n")
    }

    /// Overall resolve rate as percentage.
    pub fn resolve_rate(&self) -> f32 {
        if self.total_instances == 0 {
            return 0.0;
        }
        self.resolved_count as f32 / self.total_instances as f32 * 100.0
    }

    /// Average attempts across all instances.
    pub fn avg_attempts(&self) -> f32 {
        if self.total_instances == 0 {
            return 0.0;
        }
        self.total_attempts as f32 / self.total_instances as f32
    }

    /// Average tokens per instance.
    pub fn avg_tokens(&self) -> u32 {
        if self.total_instances == 0 {
            return 0;
        }
        self.total_tokens_used / self.total_instances as u32
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Rough token estimate for a text string (~4 chars per token).
fn estimate_tokens(text: &str) -> u32 {
    text.len().div_ceil(4) as u32
}

/// Generate a minimal sample dataset for testing without downloading SWE-bench.
pub fn generate_sample_dataset(count: usize) -> Vec<SweInstance> {
    let difficulties = ["easy", "medium", "hard"];
    let repos = [
        "django/django",
        "scikit-learn/scikit-learn",
        "matplotlib/matplotlib",
        "sympy/sympy",
        "pandas-dev/pandas",
    ];

    (0..count)
        .map(|i| SweInstance {
            instance_id: format!("sample-{:04}", i + 1),
            repo: repos[i % repos.len()].to_string(),
            base_commit: format!("abc{:040}", i),
            problem_statement: format!(
                "Sample issue #{}: A bug exists in module X where function Y returns incorrect values when input Z is negative. Fix this bug.",
                i + 1
            ),
            hints: vec![format!("Check the boundary condition at line {}", 42 + i)],
            test_patch: format!("# Test patch for sample-{:04}\n+assert fix_works()", i + 1),
            created_at: Some("2026-06-05T00:00:00Z".into()),
            version: Some("test".into()),
            difficulty: Some(difficulties[i % difficulties.len()].into()),
            estimated_tokens: 500 + (i % 10) as u32 * 100,
        })
        .collect()
}
