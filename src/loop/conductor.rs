//! Conductor — Parallel Sprint execution (gstack's Conductor pattern).
//!
//! Runs multiple LoopEngine instances in parallel, each on its own
//! git worktree branch. Inspired by gstack's Conductor mode which
//! runs up to 10 parallel Claude Code sessions with isolated workspaces.
//!
//! ## Architecture
//!
//! ```text
//! Conductor
//!   ├── Sprint 0: worktree "carp-sprint-0" → LoopEngine (role=Reviewer)
//!   ├── Sprint 1: worktree "carp-sprint-1" → LoopEngine (role=QA)
//!   ├── Sprint 2: worktree "carp-sprint-2" → LoopEngine (role=Architect)
//!   └── ...
//!
//! Each sprint:
//!   1. Creates a git worktree from the current branch
//!   2. Runs the LoopEngine in that isolated workspace
//!   3. Collects results
//!   4. Cleans up worktrees (or keeps for inspection)
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::r#loop::{LoopConfig, LoopRole, LoopSummary};

/// A single parallel sprint task.
#[derive(Debug, Clone)]
pub struct SprintTask {
    /// Human-readable name for this sprint.
    pub name: String,
    /// The cognitive role for this sprint.
    pub role: LoopRole,
    /// Target file or URL to verify.
    pub target: String,
    /// Mode: "review" or "test".
    pub mode: String,
    /// Maximum rounds for this sprint.
    pub max_rounds: u32,
}

/// Result of a completed sprint.
#[derive(Debug, Clone)]
pub struct SprintResult {
    /// The task this result corresponds to.
    pub task: SprintTask,
    /// The loop summary (if the engine ran successfully).
    pub summary: Option<LoopSummary>,
    /// Path to the worktree directory used (for inspection).
    pub worktree_path: Option<PathBuf>,
    /// Error message (if the sprint failed).
    pub error: Option<String>,
    /// Duration of the sprint in milliseconds.
    pub duration_ms: u64,
}

/// Aggregated results from all sprints.
#[derive(Debug, Clone, Default)]
pub struct ConductorReport {
    pub sprints: Vec<SprintResult>,
    pub total_sprints: usize,
    pub passed_sprints: usize,
    pub total_duration_ms: u64,
}

impl ConductorReport {
    /// Format as human-readable text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("╔══════════════════════════════════════╗\n");
        out.push_str("║     Conductor Sprint Report          ║\n");
        out.push_str("╚══════════════════════════════════════╝\n\n");
        out.push_str(&format!(
            "Sprints: {}/{} | Total time: {:.1}s\n\n",
            self.passed_sprints,
            self.total_sprints,
            self.total_duration_ms as f64 / 1000.0
        ));

        for (i, sprint) in self.sprints.iter().enumerate() {
            let status = if sprint.summary.as_ref().map(|s| s.passed).unwrap_or(false) {
                "PASS"
            } else {
                "FAIL"
            };
            out.push_str(&format!(
                "[{}] {} ({}) — {} ({:.1}s)\n",
                i + 1,
                sprint.task.name,
                sprint.task.role.system_prompt_suffix(),
                status,
                sprint.duration_ms as f64 / 1000.0
            ));
            if let Some(ref err) = sprint.error {
                out.push_str(&format!("      Error: {}\n", err));
            }
            if let Some(ref summary) = sprint.summary {
                out.push_str(&format!(
                    "      Rounds: {}, Verdict: {:?}\n",
                    summary.total_rounds, summary.final_verdict
                ));
            }
        }
        out
    }
}

/// The Conductor orchestrates parallel sprint execution.
///
/// Uses git worktrees for isolation and tokio for concurrency.
pub struct Conductor {
    /// Project root path (must be a git repo).
    project_root: PathBuf,
    /// Base branch to create worktrees from.
    base_branch: String,
    /// Worktree prefix for naming.
    worktree_prefix: String,
    /// Whether to clean up worktrees after completion.
    cleanup_on_finish: bool,
}

impl Conductor {
    /// Create a new Conductor for the given project root.
    pub fn new(project_root: &Path) -> anyhow::Result<Self> {
        // Verify it's a git repo
        let status = Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(project_root)
            .output()?;
        if !status.status.success() {
            anyhow::bail!("Not inside a git repository: {}", project_root.display());
        }

        // Get current branch
        let branch_output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(project_root)
            .output()?;
        let base_branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();

        Ok(Self {
            project_root: project_root.to_path_buf(),
            base_branch,
            worktree_prefix: "carp-sprint".to_string(),
            cleanup_on_finish: true,
        })
    }

    /// Set whether to clean up worktrees after sprints complete.
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.cleanup_on_finish = cleanup;
        self
    }

    /// Run all sprint tasks sequentially (for now; async version pending tokio runtime).
    ///
    /// Each sprint:
    /// 1. Creates a git worktree
    /// 2. Configures LoopConfig with the sprint's role
    /// 3. Runs the LoopEngine (placeholder — actual execution depends on runtime context)
    /// 4. Collects results
    pub fn run_sequential(
        &self,
        tasks: &[SprintTask],
    ) -> anyhow::Result<ConductorReport> {
        use std::time::Instant;
        let start = Instant::now();
        let mut results = Vec::with_capacity(tasks.len());

        for (idx, task) in tasks.iter().enumerate() {
            let sprint_start = Instant::now();

            // Create worktree
            let worktree_name = format!("{}-{}", self.worktree_prefix, idx);
            let worktree_path = self.project_root.join(".carp").join("worktrees").join(&worktree_name);

            match self.create_worktree(&worktree_path) {
                Ok(()) => {
                    // Build config for this sprint
                    let _config = LoopConfig {
                        max_rounds: task.max_rounds,
                        verbose: false,
                        role: task.role,
                        use_iron_laws: true,
                        enforce_review_gate: true,
                        round_timeout_secs: 300,
                        ratchet_mode: false,
                    };

                    // Note: Actual LoopEngine execution requires async runtime.
                    // The worktree is created and ready; the caller can run the engine
                    // in the worktree directory.
                    results.push(SprintResult {
                        task: task.clone(),
                        summary: None, // Populated by actual engine run
                        worktree_path: Some(worktree_path),
                        error: None,
                        duration_ms: sprint_start.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    results.push(SprintResult {
                        task: task.clone(),
                        summary: None,
                        worktree_path: None,
                        error: Some(e.to_string()),
                        duration_ms: sprint_start.elapsed().as_millis() as u64,
                    });
                }
            }

            // Cleanup if configured
            if self.cleanup_on_finish {
                let _ = self.remove_worktree(&worktree_name);
            }
        }

        let passed = results.iter()
            .filter(|r| r.summary.as_ref().map(|s| s.passed).unwrap_or(false))
            .count();

        Ok(ConductorReport {
            sprints: results,
            total_sprints: tasks.len(),
            passed_sprints: passed,
            total_duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Create a git worktree at the given path.
    fn create_worktree(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let output = Command::new("git")
            .args([
                "worktree", "add",
                "-b", &format!("{}-branch", &self.worktree_prefix),
                path.to_str().unwrap(),
                &self.base_branch,
            ])
            .current_dir(&self.project_root)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to create worktree at {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Remove a git worktree by name.
    fn remove_worktree(&self, name: &str) -> anyhow::Result<()> {
        let _output = Command::new("git")
            .args(["worktree", "remove", "--force", name])
            .current_dir(&self.project_root)
            .output()?;
        Ok(())
    }

    /// List existing carp worktrees.
    pub fn list_worktrees(&self) -> anyhow::Result<Vec<String>> {
        let output = Command::new("git")
            .args(["worktree", "list"])
            .current_dir(&self.project_root)
            .output()?;

        let mut worktrees = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if line.contains(&self.worktree_prefix) {
                worktrees.push(line.trim().to_string());
            }
        }
        Ok(worktrees)
    }

    /// Clean up all carp worktrees.
    pub fn cleanup_all(&self) -> anyhow::Result<usize> {
        let worktrees = self.list_worktrees()?;
        let mut count = 0;
        for wt in &worktrees {
            // Extract worktree name (first word before space)
            let name = wt.split_whitespace().next().unwrap_or(wt);
            if (name.starts_with('.') || name.contains("carp-sprint"))
                && self.remove_worktree(name).is_ok() {
                    count += 1;
                }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conductor_report_empty() {
        let report = ConductorReport::default();
        assert!(report.to_text().contains("0/0"));
    }

    #[test]
    fn test_conductor_report_with_results() {
        let report = ConductorReport {
            sprints: vec![SprintResult {
                task: SprintTask {
                    name: "review-sprint".into(),
                    role: LoopRole::Reviewer,
                    target: "src/main.rs".into(),
                    mode: "review".into(),
                    max_rounds: 3,
                },
                summary: None,
                worktree_path: Some(PathBuf::from("/tmp/carp-sprint-0")),
                error: None,
                duration_ms: 5000,
            }],
            total_sprints: 1,
            passed_sprints: 0,
            total_duration_ms: 5000,
        };
        let text = report.to_text();
        assert!(text.contains("review-sprint"));
        assert!(text.contains("Reviewer"));
    }

    #[test]
    fn test_sprint_task_default() {
        let task = SprintTask {
            name: "test".into(),
            role: LoopRole::Developer,
            target: ".".into(),
            mode: "review".into(),
            max_rounds: 5,
        };
        assert_eq!(task.name, "test");
        assert_eq!(task.mode, "review");
    }
}