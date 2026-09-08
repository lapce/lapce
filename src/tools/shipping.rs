//! Ship automation — sync/push/PR creation workflow (gstack's /ship skill).
//!
//! Encapsulates the "land the plane" sequence:
//!
//! ```text
//! ShipWorkflow
//!   1. Sync: git fetch + rebase onto main
//!   2. Verify: cargo check + test (or configured verification)
//!   3. Stage: git add changed files
//!   4. Commit: generate conventional commit message
//!   5. Push: git push to remote
//!   6. PR: create pull request via gh CLI or API
//! ```
//!
//! Inspired by gstack's `/ship` command which enforces shipping hygiene.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration for a ship operation.
#[derive(Debug, Clone)]
pub struct ShipConfig {
    /// Remote name to push to (default: "origin").
    pub remote: String,
    /// Base branch for PR (default: "main").
    pub base_branch: String,
    /// Whether to run `cargo check` before pushing.
    pub verify_before_push: bool,
    /// Whether to run tests before pushing.
    pub test_before_push: bool,
    /// Commit message prefix (e.g., "feat:", "fix:").
    pub commit_prefix: Option<String>,
    /// PR title template (use {prefix} and {description}).
    pub pr_title_template: Option<String>,
    /// PR body template.
    pub pr_body_template: Option<String>,
}

impl Default for ShipConfig {
    fn default() -> Self {
        Self {
            remote: "origin".into(),
            base_branch: "main".into(),
            verify_before_push: true,
            test_before_push: true,
            commit_prefix: None,
            pr_title_template: Some("{prefix} {description}".into()),
            pr_body_template: Some(
                "## Summary\n{description}\n\
                 \n\
                 ## Verification\n\
                 - [x] cargo check passed\n\
                 - [x] Tests passed\n"
                    .into(),
            ),
        }
    }
}

/// Result of a ship operation.
#[derive(Debug, Clone)]
pub struct ShipResult {
    /// Each step that was executed.
    pub steps: Vec<ShipStepResult>,
    /// Overall success status.
    pub success: bool,
    /// PR URL if one was created.
    pub pr_url: Option<String>,
    /// Total duration in milliseconds.
    pub duration_ms: u64,
}

/// Result of an individual ship step.
#[derive(Debug, Clone)]
pub struct ShipStepResult {
    pub step_name: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl ShipStepResult {
    fn ok(name: &str, output: impl Into<String>, ms: u64) -> Self {
        Self {
            step_name: name.into(),
            success: true,
            output: output.into(),
            error: None,
            duration_ms: ms,
        }
    }

    fn fail(name: &str, err: impl Into<String>, ms: u64) -> Self {
        Self {
            step_name: name.into(),
            success: false,
            output: String::new(),
            error: Some(err.into()),
            duration_ms: ms,
        }
    }
}

impl ShipResult {
    /// Format as human-readable text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let icon = if self.success { "✅" } else { "❌" };
        out.push_str(&format!("Ship Result {} ({:.1}s)\n", icon, self.duration_ms as f64 / 1000.0));
        out.push_str("────────────────────────────\n");

        for step in &self.steps {
            let s = if step.success { "✓" } else { "✗" };
            out.push_str(&format!(
                "  {} {} ({:.1}s)\n",
                s, step.step_name, step.duration_ms as f64 / 1000.0
            ));
            if !step.output.is_empty() && step.output.len() < 200 {
                out.push_str(&format!("      {}\n", &step.output[..step.output.len().min(100)]));
            }
            if let Some(ref e) = step.error {
                out.push_str(&format!("      Error: {}\n", e));
            }
        }

        if let Some(ref url) = self.pr_url {
            out.push_str(&format!("\nPR: {}\n", url));
        }
        out
    }
}

/// The Ship orchestrator — runs the full sync→verify→commit→push→PR pipeline.
pub struct ShipWorkflow {
    project_root: PathBuf,
    config: ShipConfig,
}

impl ShipWorkflow {
    /// Create a new ShipWorkflow for the given project root.
    pub fn new(project_root: &Path) -> anyhow::Result<Self> {
        // Verify it's a git repo
        let status = Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(project_root)
            .output()?;
        if !status.status.success() {
            anyhow::bail!("Not inside a git repository");
        }

        Ok(Self {
            project_root: project_root.to_path_buf(),
            config: ShipConfig::default(),
        })
    }

    /// Set custom configuration.
    pub fn with_config(mut self, config: ShipConfig) -> Self {
        self.config = config;
        self
    }

    /// Execute the full ship pipeline.
    ///
    /// Returns a detailed result with each step's outcome.
    pub async fn execute(
        &self,
        description: &str,
    ) -> anyhow::Result<ShipResult> {
        use std::time::Instant;
        let start = Instant::now();
        let mut steps = Vec::new();

        // Step 1: Sync with remote
        let s_start = Instant::now();
        match self.sync() {
            Ok(output) => steps.push(ShipStepResult::ok("sync", output, s_start.elapsed().as_millis() as u64)),
            Err(e) => steps.push(ShipStepResult::fail("sync", e.to_string(), s_start.elapsed().as_millis() as u64)),
        }

        // Step 2: Verify (cargo check)
        if self.config.verify_before_push {
            let v_start = Instant::now();
            match self.verify() {
                Ok(output) => steps.push(ShipStepResult::ok("verify (cargo check)", output, v_start.elapsed().as_millis() as u64)),
                Err(e) => steps.push(ShipStepResult::fail("verify (cargo check)", e.to_string(), v_start.elapsed().as_millis() as u64)),
            }
        }

        // Step 3: Test (cargo test)
        if self.config.test_before_push {
            let t_start = Instant::now();
            match self.test() {
                Ok(output) => steps.push(ShipStepResult::ok("test (cargo test)", output, t_start.elapsed().as_millis() as u64)),
                Err(e) => steps.push(ShipStepResult::fail("test (cargo test)", e.to_string(), t_start.elapsed().as_millis() as u64)),
            }
        }

        // Step 4: Commit
        let c_start = Instant::now();
        match self.commit(description).await {
            Ok(output) => steps.push(ShipStepResult::ok("commit", output, c_start.elapsed().as_millis() as u64)),
            Err(e) => steps.push(ShipStepResult::fail("commit", e.to_string(), c_start.elapsed().as_millis() as u64)),
        }

        // Step 5: Push
        let p_start = Instant::now();
        match self.push().await {
            Ok(output) => steps.push(ShipStepResult::ok("push", output, p_start.elapsed().as_millis() as u64)),
            Err(e) => steps.push(ShipStepResult::fail("push", e.to_string(), p_start.elapsed().as_millis() as u64)),
        }

        // Step 6: Create PR (if gh CLI available)
        let pr_url = if self.steps_all_ok(&steps) {
            let r_start = Instant::now();
            match self.create_pr(description).await {
                Ok(url) => {
                    steps.push(ShipStepResult::ok("create PR", format!("Created: {}", url), r_start.elapsed().as_millis() as u64));
                    Some(url)
                }
                Err(e) => {
                    // PR creation is non-fatal; push succeeded
                    tracing::warn!("PR creation failed (non-fatal): {}", e);
                    steps.push(ShipStepResult::fail("create PR", e.to_string(), r_start.elapsed().as_millis() as u64));
                    None
                }
            }
        } else {
            None
        };

        let all_ok = self.steps_all_ok(&steps);

        Ok(ShipResult {
            steps,
            success: all_ok,
            pr_url,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Step 1: Fetch + rebase onto base branch.
    fn sync(&self) -> anyhow::Result<String> {
        let output = Command::new("git")
            .args(["fetch", &self.config.remote])
            .current_dir(&self.project_root)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("git fetch failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        let output = Command::new("git")
            .args(["rebase", &format!("{}/{}", &self.config.remote, &self.config.base_branch)])
            .current_dir(&self.project_root)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("git rebase failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Step 2a: Run cargo check.
    fn verify(&self) -> anyhow::Result<String> {
        let output = Command::new("cargo")
            .args(["check", "--lib"])
            .current_dir(&self.project_root)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("cargo check failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok("cargo check passed".to_string())
    }

    /// Step 2b: Run cargo test.
    fn test(&self) -> anyhow::Result<String> {
        let output = Command::new("cargo")
            .args(["test", "--lib"])
            .current_dir(&self.project_root)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("cargo test failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok("cargo test passed".to_string())
    }

    /// Step 3: Generate commit message and stage+commit.
    async fn commit(&self, description: &str) -> anyhow::Result<String> {
        let msg = match &self.config.commit_prefix {
            Some(prefix) => format!("{} {}", prefix, description),
            None => description.to_string(),
        };

        // Stage all changes
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.project_root)
            .output()?;

        // Check if there are staged changes
        let diff_output = Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(&self.project_root)
            .output()?;

        if String::from_utf8_lossy(&diff_output.stdout).trim().is_empty() {
            return Ok("No changes to commit".to_string());
        }

        let output = Command::new("git")
            .args(["commit", "-m", &msg])
            .current_dir(&self.project_root)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("git commit failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(format!("Committed: {}", msg))
    }

    /// Step 4: Push to remote.
    async fn push(&self) -> anyhow::Result<String> {
        let output = Command::new("git")
            .args(["push", &self.config.remote, "HEAD"])
            .current_dir(&self.project_root)
            .output()?;
        if !output.status.success() {
            anyhow::bail!("git push failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Step 5: Create PR using gh CLI.
    async fn create_pr(&self, description: &str) -> anyhow::Result<String> {
        // Check if gh CLI is available
        let check = Command::new("gh").arg("--version").output();
        match check {
            Ok(o) if o.status.success() => {}
            _ => anyhow::bail!("gh CLI not found"),
        }

        let title = self.config.pr_title_template.as_deref()
            .unwrap_or("{description}")
            .replace("{description}", description);

        let body = self.config.pr_body_template.as_deref()
            .unwrap_or("")
            .replace("{description}", description);

        let output = Command::new("gh")
            .args([
                "pr", "create",
                "--title", &title,
                "--body", &body,
                "--base", &self.config.base_branch,
            ])
            .current_dir(&self.project_root)
            .output()?;

        if !output.status.success() {
            anyhow::bail!("gh pr create failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        let url = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|l| l.starts_with("https://"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "(URL not parsed)".to_string());

        Ok(url)
    }

    /// Check if all steps succeeded.
    fn steps_all_ok(&self, steps: &[ShipStepResult]) -> bool {
        steps.iter().all(|s| s.success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ship_result_text_success() {
        let result = ShipResult {
            steps: vec![ShipStepResult::ok("sync", "done", 100)],
            success: true,
            pr_url: Some("https://github.com/test/pull/42".into()),
            duration_ms: 500,
        };
        let text = result.to_text();
        assert!(text.contains("✅"));
        assert!(text.contains("pull/42"));
    }

    #[test]
    fn test_ship_result_text_failure() {
        let result = ShipResult {
            steps: vec![ShipStepResult::fail("sync", "no network", 200)],
            success: false,
            pr_url: None,
            duration_ms: 200,
        };
        let text = result.to_text();
        assert!(text.contains("❌"));
        assert!(text.contains("no network"));
    }

    #[test]
    fn test_ship_config_default() {
        let config = ShipConfig::default();
        assert_eq!(config.remote, "origin");
        assert_eq!(config.base_branch, "main");
        assert!(config.verify_before_push);
    }

    #[test]
    fn test_ship_step_result_helpers() {
        let ok = ShipStepResult::ok("test", "output", 50);
        assert!(ok.success);
        assert_eq!(ok.step_name, "test");

        let fail = ShipStepResult::fail("test", "err", 30);
        assert!(!fail.success);
        assert!(fail.error.is_some());
    }
}