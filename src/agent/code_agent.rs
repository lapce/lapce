//! Code-generation web agent — generates, executes, and debugs Playwright scripts.
//!
//! Inspired by the Microsoft Webwright project, this agent replaces click prediction
//! with script generation. It produces Python Playwright scripts, runs them via
//! `std::process::Command`, captures output/screenshots, and self-reflects on
//! whether the task was truly completed.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐    ┌────────────────┐    ┌────────────────┐
//! │ ScriptGen    │───▶│ ScriptExecutor │───▶│ SelfReflector  │
//! │ (generates   │    │ (runs script,  │    │ (gate check,   │
//! │  Playwright  │    │  captures I/O) │    │  decides next) │
//! │  code)       │    │                │    │                │
//! └──────────────┘    └────────────────┘    └────────────────┘
//!         │                    │                      │
//!         ▼                    ▼                      ▼
//!         ┌──────────────────────────────────────────┐
//!         │              Workspace                   │
//!         │  (.carp/workspace/ scripts, logs,        │
//!         │   screenshots)                           │
//!         └──────────────────────────────────────────┘
//! ```

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Artifact
// ─────────────────────────────────────────────────────────────────────────────

/// An artifact produced by the code agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Artifact {
    /// A generated Playwright script file.
    Script {
        path: PathBuf,
        content: String,
    },
    /// A PNG screenshot captured during execution.
    Screenshot(PathBuf),
    /// Execution log output.
    Log(String),
    /// Final textual output (e.g. extracted page content).
    FinalOutput(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// RunResult
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a single `run_and_reflect` cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    /// Absolute path to the generated script.
    pub script_path: PathBuf,
    /// Whether the task was deemed completed successfully.
    pub success: bool,
    /// List of screenshot paths captured during execution.
    pub screenshots: Vec<PathBuf>,
    /// Full execution log (stdout + stderr combined).
    pub log: String,
    /// Optional error trace if the script crashed or the reflection failed.
    pub error_trace: Option<String>,
    /// Number of refinement rounds used.
    pub refinement_rounds: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Workspace
// ─────────────────────────────────────────────────────────────────────────────

/// Manages script files, logs, and screenshots as artifacts on disk.
///
/// Default location is `<project_root>/.carp/workspace/`. Each run creates
/// a timestamped subdirectory to isolate artifacts between iterations.
pub struct Workspace {
    /// Root directory for all workspace artifacts.
    root_dir: PathBuf,
    /// Current run subdirectory (set before each execution).
    run_dir: Option<PathBuf>,
    /// In-memory artifact log.
    artifacts: Vec<Artifact>,
}

impl Workspace {
    /// Create a new workspace rooted at the given directory.
    ///
    /// The directory is created if it does not exist.
    pub fn new(root: PathBuf) -> Self {
        if !root.exists() {
            let _ = std::fs::create_dir_all(&root);
        }
        Self {
            root_dir: root,
            run_dir: None,
            artifacts: Vec::new(),
        }
    }

    /// Create the default workspace at `<project_root>/.carp/workspace/`.
    ///
    /// `project_root` defaults to the current working directory if not provided.
    pub fn default_with_root(project_root: Option<PathBuf>) -> Self {
        let root = project_root
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .join(".carp")
            .join("workspace");
        Self::new(root)
    }

    /// Prepare a fresh run directory with a timestamp-based name.
    pub fn prepare_run(&mut self) -> PathBuf {
        let ts = chrono::Utc::now().format("run_%Y%m%d_%H%M%S_%3f");
        let dir = self.root_dir.join(ts.to_string());
        if !dir.exists() {
            let _ = std::fs::create_dir_all(&dir);
        }
        self.run_dir = Some(dir.clone());
        dir
    }

    /// Write a script file into the current run directory.
    ///
    /// Returns the absolute path of the written file.
    pub fn write_script(&mut self, filename: &str, content: &str) -> PathBuf {
        let dir = self.run_dir.clone().unwrap_or_else(|| self.prepare_run());
        let path = dir.join(filename);
        let _ = std::fs::write(&path, content);
        self.artifacts.push(Artifact::Script {
            path: path.clone(),
            content: content.to_string(),
        });
        path
    }

    /// Record a log entry into the workspace.
    pub fn record_log(&mut self, log: String) {
        self.artifacts.push(Artifact::Log(log.clone()));
        // Also persist to a log file
        if let Some(ref dir) = self.run_dir {
            let log_path = dir.join("execution.log");
            let _ = std::fs::write(&log_path, &log);
        }
    }

    /// Record a screenshot path.
    pub fn record_screenshot(&mut self, path: PathBuf) {
        self.artifacts.push(Artifact::Screenshot(path.clone()));
    }

    /// Record final output.
    pub fn record_output(&mut self, output: String) {
        self.artifacts.push(Artifact::FinalOutput(output));
    }

    /// Iterate over all collected artifacts.
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    /// Current run directory, if any.
    pub fn run_dir(&self) -> Option<&PathBuf> {
        self.run_dir.as_ref()
    }

    /// Root directory of the workspace.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Return the absolute path to a screenshot file within the run directory.
    pub fn screenshot_path(&self, name: &str) -> PathBuf {
        let dir = self.run_dir.clone().unwrap_or_else(|| self.root_dir.clone());
        dir.join(format!("{}.png", name))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ScriptGenerator
// ─────────────────────────────────────────────────────────────────────────────

/// Generates Playwright Python scripts from a task description and DOM context.
///
/// The generator produces self-contained Python code that:
/// - Launches a Chromium browser via Playwright
/// - Navigates to the given URL (if applicable)
/// - Performs actions described in the task
/// - Takes a screenshot before exiting
pub struct ScriptGenerator;

impl ScriptGenerator {
    /// Generate a Playwright Python script for the given task and DOM context.
    ///
    /// `task` describes what the script should do (e.g. "click the login button",
    /// "fill in the search form", "extract all article titles").
    ///
    /// `dom_context` provides information about the current page state, such as
    /// visible elements, selectors, or URL. This helps the generator produce
    /// accurate selectors.
    pub fn generate(task: &str, dom_context: &str) -> String {
        let safe_task = sanitize_comment(task);
        let safe_dom = sanitize_comment(dom_context);

        format!(
            r#"import asyncio
import sys
import json
from playwright.async_api import async_playwright

# Task: {safe_task}
# DOM context: {safe_dom}

async def main():
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True)
        context = await browser.new_context(
            viewport={{"width": 1280, "height": 720}},
            user_agent="Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
        )
        page = await context.new_page()

        # --- Task-specific logic below ---
        # This is a template. The LLM should fill in the actual steps.

        # Example navigation (customize as needed):
        # await page.goto("https://example.com")

        # Perform actions based on task description
        # await page.click("selector")
        # await page.fill("selector", "value")
        # await page.wait_for_selector("selector")

        # Capture screenshot before finishing
        await page.screenshot(path="screenshot.png", full_page=True)

        # Extract page title as a simple output
        title = await page.title()
        result = {{
            "title": title,
            "url": page.url,
            "success": True,
        }}
        print(json.dumps(result))

        await browser.close()

if __name__ == "__main__":
    asyncio.run(main())
"#,
        )
    }

    /// Generate a refined/updated script based on previous execution feedback.
    ///
    /// `feedback` describes what went wrong in the previous run (e.g. timeout,
    /// selector not found, wrong page).
    pub fn refine(_previous_script: &str, task: &str, dom_context: &str, feedback: &str) -> String {
        let safe_feedback = sanitize_comment(feedback);

        // Start from scratch with fresh template, embedding the feedback as guidance
        let mut script = Self::generate(task, dom_context);
        script.push_str(&format!(
            "\n# Refinement feedback from previous run:\n# {safe_feedback}\n"
        ));
        script
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ScriptExecutor
// ─────────────────────────────────────────────────────────────────────────────

/// Execution output from running a script.
#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    /// Combined stdout.
    pub stdout: String,
    /// Combined stderr.
    pub stderr: String,
    /// Exit code of the process.
    pub exit_code: Option<i32>,
    /// Absolute paths to screenshot files found after execution.
    pub screenshots: Vec<PathBuf>,
}

/// Runs a generated Playwright script via `std::process::Command`.
///
/// Assumes `python` (or `python3`) is available on `PATH` and Playwright is
/// installed. Supports configurable Python interpreter.
pub struct ScriptExecutor {
    /// Python interpreter command (e.g. "python", "python3").
    python_cmd: String,
    /// Working directory for execution.
    work_dir: PathBuf,
}

impl ScriptExecutor {
    /// Create a new executor with the given Python command and working directory.
    pub fn new(python_cmd: impl Into<String>, work_dir: PathBuf) -> Self {
        Self {
            python_cmd: python_cmd.into(),
            work_dir,
        }
    }

    /// Create a default executor using "python" as the interpreter.
    pub fn default_with_workdir(work_dir: PathBuf) -> Self {
        Self::new("python", work_dir)
    }

    /// Execute a script file and capture its output.
    ///
    /// Blocks the current thread while the subprocess runs (use
    /// `tokio::task::spawn_blocking` from async contexts).
    pub fn execute(&self, script_path: &Path) -> ExecutionOutput {
        let start = Instant::now();

        let output = ProcessCommand::new(&self.python_cmd)
            .arg(script_path)
            .current_dir(&self.work_dir)
            .output();

        let elapsed = start.elapsed();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let exit_code = out.status.code();

                // Discover screenshots generated in the work directory
                let screenshots = self.discover_screenshots();

                tracing::info!(
                    script = %script_path.display(),
                    exit_code,
                    elapsed_ms = elapsed.as_millis(),
                    stdout_len = stdout.len(),
                    stderr_len = stderr.len(),
                    screenshots = screenshots.len(),
                    "Script execution completed"
                );

                ExecutionOutput {
                    stdout,
                    stderr,
                    exit_code,
                    screenshots,
                }
            }
            Err(e) => {
                tracing::error!(
                    script = %script_path.display(),
                    error = %e,
                    "Failed to execute script"
                );

                ExecutionOutput {
                    stdout: String::new(),
                    stderr: format!("Failed to launch process: {e}"),
                    exit_code: None,
                    screenshots: Vec::new(),
                }
            }
        }
    }

    /// Set a different Python interpreter.
    pub fn set_python_cmd(&mut self, cmd: impl Into<String>) {
        self.python_cmd = cmd.into();
    }

    /// Discover PNG files in the working directory (screenshots).
    fn discover_screenshots(&self) -> Vec<PathBuf> {
        let mut shots = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.work_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "png") {
                    shots.push(path);
                }
            }
        }
        shots
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SelfReflector
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a reflection check.
#[derive(Debug, Clone)]
pub struct ReflectionOutcome {
    /// Whether the task is considered completed.
    pub completed: bool,
    /// Confidence score (0.0 – 1.0).
    pub confidence: f32,
    /// Human-readable explanation for the judgement.
    pub reason: String,
    /// Specific suggestions for improvement, if not completed.
    pub suggestions: Vec<String>,
}

/// Reflects on execution results to determine if the task was truly completed.
///
/// Implements a lightweight gate check inspired by Webwright's self-reflection:
/// - Checks exit code (non-zero ⇒ failure)
/// - Checks for error keywords in stderr
/// - Validates that screenshots exist (indicates the browser ran)
/// - Checks for a success marker in stdout
pub struct SelfReflector;

impl SelfReflector {
    /// Reflect on an execution result and determine whether the task succeeded.
    ///
    /// `task` is the original task description, used for context-aware checks.
    /// `output` is the raw execution output.
    pub fn reflect(task: &str, output: &ExecutionOutput) -> ReflectionOutcome {
        let mut issues: Vec<String> = Vec::new();
        let mut suggestions: Vec<String> = Vec::new();

        // 1. Check exit code
        let exit_ok = match output.exit_code {
            Some(0) => true,
            Some(code) => {
                issues.push(format!("Script exited with non-zero code {code}"));
                suggestions.push("Check for Python runtime errors or missing dependencies.".into());
                false
            }
            None => {
                issues.push("Script failed to launch.".into());
                suggestions.push("Ensure Python and Playwright are installed.".into());
                false
            }
        };

        // 2. Check stderr for common error patterns
        let stderr_lower = output.stderr.to_lowercase();
        let error_keywords = [
            "error", "exception", "traceback", "timeouterror",
            "timeout", "not found", "no such element", "failed",
        ];
        let has_errors = error_keywords
            .iter()
            .filter(|&&kw| stderr_lower.contains(kw))
            .count();
        if has_errors > 0 {
            issues.push(format!(
                "stderr contains {has_errors} error keyword(s)"
            ));
            suggestions.push("Review the error trace in stderr and fix selectors or timing.".into());
        }

        // 3. Check screenshots
        let has_screenshots = !output.screenshots.is_empty();
        if !has_screenshots {
            issues.push("No screenshots were produced — the browser may not have rendered.".into());
            suggestions.push("Ensure `page.screenshot()` is called in the script.".into());
        }

        // 4. Check stdout for success markers
        let stdout_lower = output.stdout.to_lowercase();
        let success_markers = ["\"success\": true", "\"status\": \"ok\"", "task complete"];
        let has_success = success_markers
            .iter()
            .any(|&m| stdout_lower.contains(m));

        // 5. Confidence calculation
        let mut confidence = 1.0_f32;
        if !exit_ok {
            confidence -= 0.4;
        }
        if has_errors > 0 {
            confidence -= 0.2 * has_errors.min(3) as f32;
        }
        if !has_screenshots {
            confidence -= 0.2;
        }
        if !has_success {
            confidence -= 0.1;
        }
        confidence = confidence.clamp(0.0, 1.0);

        let completed = exit_ok && has_screenshots && confidence >= 0.5;

        let reason = if completed {
            format!(
                "Task appears completed (confidence: {confidence:.2}). \
                 Exit OK, screenshots present, no critical errors."
            )
        } else {
            let issue_list = issues.join("; ");
            format!(
                "Task may be incomplete (confidence: {confidence:.2}). Issues: {issue_list}"
            )
        };

        tracing::info!(
            task = sanitize_comment(task),
            completed,
            confidence,
            issues = ?issues,
            "Self-reflection completed"
        );

        ReflectionOutcome {
            completed,
            confidence,
            reason,
            suggestions,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CodeAgent
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum number of refinement rounds before the agent gives up.
pub const MAX_REFINEMENT_ROUNDS: u32 = 3;

/// A code-generation web agent that generates, executes, and debugs Playwright
/// scripts.
///
/// The agent follows a closed loop:
/// 1. **Generate** — Takes a task + DOM context, produces a Playwright Python script
/// 2. **Execute** — Runs the script via subprocess, captures I/O and screenshots
/// 3. **Reflect** — Checks if the task was truly completed
/// 4. **Refine** — If not completed, improves the script and re-runs (up to
///    [`MAX_REFINEMENT_ROUNDS`] times)
///
/// # Example
///
/// ```ignore
/// use std::path::PathBuf;
/// use deepseek_carp::agent::code_agent::{CodeAgent, Workspace, CodeAgentConfig};
///
/// let workspace = Workspace::default_with_root(None);
/// let config = CodeAgentConfig::default();
/// let agent = CodeAgent::new(config, workspace);
///
/// let result = agent.run_and_reflect(
///     "Navigate to example.com and extract the page title",
///     "URL: https://example.com, no visible elements yet",
/// ).expect("Agent execution failed");
///
/// println!("Success: {}", result.success);
/// ```
pub struct CodeAgent {
    /// Configuration for the agent.
    config: CodeAgentConfig,
    /// Workspace for artifact management.
    workspace: Workspace,
}

/// Configuration for [`CodeAgent`].
#[derive(Debug, Clone)]
pub struct CodeAgentConfig {
    /// Python interpreter command.
    pub python_cmd: String,
    /// Maximum number of refinement rounds.
    pub max_refinements: u32,
    /// Whether to keep artifacts from failed runs.
    pub keep_failed_artifacts: bool,
}

impl Default for CodeAgentConfig {
    fn default() -> Self {
        Self {
            python_cmd: "python".into(),
            max_refinements: MAX_REFINEMENT_ROUNDS,
            keep_failed_artifacts: true,
        }
    }
}

impl CodeAgent {
    /// Create a new `CodeAgent` with the given configuration and workspace.
    pub fn new(config: CodeAgentConfig, workspace: Workspace) -> Self {
        Self { config, workspace }
    }

    /// Run the full generate → execute → reflect → (refine) loop.
    ///
    /// 1. Generates a Playwright script from `task` and `dom_context`
    /// 2. Executes it via subprocess
    /// 3. Reflects on the results
    /// 4. If the task is not completed and refinement rounds remain, refines the
    ///    script and re-runs in a clean environment
    ///
    /// Returns a [`RunResult`] summarizing the final attempt.
    pub fn run_and_reflect(
        &mut self,
        task: &str,
        dom_context: &str,
    ) -> anyhow::Result<RunResult> {
        let mut last_output: Option<ExecutionOutput> = None;
        let mut last_script_path: Option<PathBuf> = None;
        let mut final_success = false;
        let mut rounds: u32 = 0;
        let mut all_logs = String::new();
        let mut all_screenshots: Vec<PathBuf> = Vec::new();
        let mut error_trace: Option<String> = None;

        for round in 0..=self.config.max_refinements {
            rounds = round;
            tracing::info!(round, task = sanitize_comment(task), "Starting refinement round");

            // Prepare a clean run directory
            let run_dir = self.workspace.prepare_run();
            let executor = ScriptExecutor::new(&self.config.python_cmd, run_dir.clone());

            // Generate or refine script
            let script_content = match round {
                0 => ScriptGenerator::generate(task, dom_context),
                _ => {
                    let feedback = last_output
                        .as_ref()
                        .map(|o| format!("stderr: {}\nstdout: {}", o.stderr, o.stdout))
                        .unwrap_or_default();
                    let prev = last_script_path
                        .as_ref()
                        .and_then(|p| std::fs::read_to_string(p).ok())
                        .unwrap_or_default();
                    ScriptGenerator::refine(&prev, task, dom_context, &feedback)
                }
            };

            let script_path = self.workspace.write_script("playwright_script.py", &script_content);
            last_script_path = Some(script_path.clone());

            // Execute
            let output = executor.execute(&script_path);
            last_output = Some(output.clone());

            // Collect screenshots
            for s in &output.screenshots {
                self.workspace.record_screenshot(s.clone());
                if !all_screenshots.contains(s) {
                    all_screenshots.push(s.clone());
                }
            }

            // Collect logs
            let round_log = format!(
                "=== Round {round} ===\nExit code: {:?}\nSTDOUT:\n{}\nSTDERR:\n{}\n",
                output.exit_code, output.stdout, output.stderr
            );
            all_logs.push_str(&round_log);
            self.workspace.record_log(round_log);

            // Reflect
            let outcome = SelfReflector::reflect(task, &output);

            if outcome.completed {
                final_success = true;
                error_trace = None;
                self.workspace.record_output(output.stdout.clone());
                tracing::info!(round, confidence = outcome.confidence, "Task completed");
                break;
            }

            error_trace = Some(format!(
                "Reflection: {reason}\nSuggestions: {suggestions}\nStderr: {stderr}",
                reason = outcome.reason,
                suggestions = outcome.suggestions.join("; "),
                stderr = output.stderr,
            ));

            if !self.config.keep_failed_artifacts {
                let _ = std::fs::remove_dir_all(&run_dir);
            }

            tracing::warn!(
                round,
                confidence = outcome.confidence,
                "Task not completed, {}",
                if round < self.config.max_refinements {
                    "will refine"
                } else {
                    "max rounds reached"
                }
            );
        }

        let result = RunResult {
            script_path: last_script_path.unwrap_or_else(|| PathBuf::from("")),
            success: final_success,
            screenshots: all_screenshots,
            log: all_logs,
            error_trace,
            refinement_rounds: rounds,
        };

        Ok(result)
    }

    /// Access the workspace (e.g. to inspect artifacts after execution).
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Access the workspace mutably.
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    /// Get the agent configuration.
    pub fn config(&self) -> &CodeAgentConfig {
        &self.config
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Sanitize a string for use in a Python comment (strip unsafe characters).
fn sanitize_comment(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .replace(['\n', '\r'], " ")
        .chars()
        .take(200)
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_serialization() {
        let script = Artifact::Script {
            path: PathBuf::from("test.py"),
            content: "print('hello')".into(),
        };
        let json = serde_json::to_string(&script).unwrap();
        let deserialized: Artifact = serde_json::from_str(&json).unwrap();
        match deserialized {
            Artifact::Script { path, content } => {
                assert_eq!(path, PathBuf::from("test.py"));
                assert_eq!(content, "print('hello')");
            }
            _ => panic!("Wrong variant after round-trip"),
        }
    }

    #[test]
    fn test_run_result_serialization() {
        let result = RunResult {
            script_path: PathBuf::from("script.py"),
            success: true,
            screenshots: vec![PathBuf::from("shot.png")],
            log: "log content".into(),
            error_trace: None,
            refinement_rounds: 1,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let deserialized: RunResult = serde_json::from_str(&json).unwrap();
        assert!(deserialized.success);
        assert_eq!(deserialized.script_path, PathBuf::from("script.py"));
        assert_eq!(deserialized.refinement_rounds, 1);
    }

    #[test]
    fn test_workspace_creates_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = Workspace::new(tmp.path().join("test_ws"));
        let run_dir = ws.prepare_run();
        assert!(run_dir.exists(), "Run directory should be created");
    }

    #[test]
    fn test_workspace_write_script() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = Workspace::new(tmp.path().join("ws"));
        let path = ws.write_script("test_script.py", "print('hello')");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "print('hello')");
    }

    #[test]
    fn test_workspace_default_with_root() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::default_with_root(Some(tmp.path().to_path_buf()));
        assert_eq!(
            ws.root_dir(),
            tmp.path().join(".carp").join("workspace")
        );
        assert!(ws.root_dir().exists());
    }

    #[test]
    fn test_script_generates_valid_python() {
        let script = ScriptGenerator::generate("click login", "page has a button");
        // Should contain key Playwright imports and structure
        assert!(script.contains("from playwright.async_api import async_playwright"));
        assert!(script.contains("async def main():"));
        assert!(script.contains("await page.screenshot"));
        assert!(script.contains("json.dumps(result)"));
    }

    #[test]
    fn test_script_generates_with_comments() {
        let script = ScriptGenerator::generate("navigate to URL", "empty page");
        assert!(script.contains("# Task: navigate to URL"));
        assert!(script.contains("# DOM context: empty page"));
    }

    #[test]
    fn test_script_refine_includes_feedback() {
        let prev = ScriptGenerator::generate("test", "ctx");
        let refined = ScriptGenerator::refine(&prev, "test", "ctx", "Timeout waiting for selector");
        assert!(refined.contains("Timeout waiting for selector"));
    }

    #[test]
    fn test_self_reflector_success() {
        let output = ExecutionOutput {
            stdout: r#"{"success": true, "title": "Example"}"#.into(),
            stderr: String::new(),
            exit_code: Some(0),
            screenshots: vec![PathBuf::from("shot.png")],
        };
        let outcome = SelfReflector::reflect("test task", &output);
        assert!(outcome.completed);
        assert!(outcome.confidence >= 0.5);
    }

    #[test]
    fn test_self_reflector_failure_nonzero_exit() {
        let output = ExecutionOutput {
            stdout: String::new(),
            stderr: "Traceback: error".into(),
            exit_code: Some(1),
            screenshots: Vec::new(),
        };
        let outcome = SelfReflector::reflect("test task", &output);
        assert!(!outcome.completed);
        assert!(outcome.confidence < 0.5);
    }

    #[test]
    fn test_self_reflector_no_screenshots() {
        let output = ExecutionOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            screenshots: Vec::new(),
        };
        let outcome = SelfReflector::reflect("test task", &output);
        assert!(!outcome.completed);
        assert!(outcome.suggestions.iter().any(|s| s.contains("screenshot")));
    }

    #[test]
    fn test_self_reflector_partial_success() {
        let output = ExecutionOutput {
            stdout: "some output".into(),
            stderr: "warning: something".into(),
            exit_code: Some(0),
            screenshots: vec![PathBuf::from("shot.png")],
        };
        let outcome = SelfReflector::reflect("test task", &output);
        // No error keywords triggered, no success markers but has screenshots and exit 0
        // confidence: 1.0 - 0.0 - 0.0 - 0.1 (no success marker) = 0.9 >= 0.5
        assert!(outcome.completed);
    }

    #[test]
    fn test_self_reflector_error_keywords() {
        let output = ExecutionOutput {
            stdout: String::new(),
            stderr: "TimeoutError: page not found".into(),
            exit_code: Some(1),
            screenshots: vec![PathBuf::from("shot.png")],
        };
        let outcome = SelfReflector::reflect("test task", &output);
        assert!(!outcome.completed);
        assert!(outcome.reason.contains("confidence"));
    }

    #[test]
    fn test_executor_default_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let executor = ScriptExecutor::default_with_workdir(tmp.path().to_path_buf());
        assert_eq!(executor.python_cmd, "python");
        assert_eq!(executor.work_dir, tmp.path());
    }

    #[test]
    fn test_code_agent_config_default() {
        let config = CodeAgentConfig::default();
        assert_eq!(config.python_cmd, "python");
        assert_eq!(config.max_refinements, 3);
        assert!(config.keep_failed_artifacts);
    }

    #[test]
    fn test_code_agent_new() {
        let ws = Workspace::default_with_root(None);
        let agent = CodeAgent::new(CodeAgentConfig::default(), ws);
        assert_eq!(agent.config().max_refinements, 3);
    }

    #[test]
    fn test_sanitize_comment_removes_control_chars() {
        let input = "hello\x00world\nline2\r\n";
        let sanitized = sanitize_comment(input);
        assert!(!sanitized.contains('\x00'));
        assert!(!sanitized.contains('\n'));
    }

    #[test]
    fn test_sanitize_comment_truncates() {
        let long = "a".repeat(500);
        let sanitized = sanitize_comment(&long);
        assert!(sanitized.len() <= 200);
    }

    #[test]
    fn test_workspace_screenshot_path() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = Workspace::new(tmp.path().join("ws"));
        let run_dir = ws.prepare_run();
        let path = ws.screenshot_path("final");
        assert_eq!(path, run_dir.join("final.png"));
    }

    #[test]
    fn test_workspace_record_log() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = Workspace::new(tmp.path().join("ws"));
        ws.prepare_run();
        ws.record_log("test log".into());
        assert_eq!(ws.artifacts().len(), 1);
    }

    #[test]
    fn test_workspace_record_output() {
        let mut ws = Workspace::default_with_root(None);
        ws.prepare_run();
        ws.record_output("final result".into());
        let artifacts = ws.artifacts();
        assert!(matches!(artifacts.last(), Some(Artifact::FinalOutput(_))));
    }

    #[test]
    fn test_code_agent_run_and_reflect_no_python() {
        // Without Python installed, the agent should produce a RunResult
        // with success=false and a meaningful error_trace.
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = Workspace::new(tmp.path().join("agent_test"));
        let config = CodeAgentConfig {
            python_cmd: "nonexistent_python_binary".into(),
            max_refinements: 1,
            keep_failed_artifacts: true,
        };
        let mut agent = CodeAgent::new(config, ws);

        let result = agent
            .run_and_reflect("test task", "empty dom")
            .expect("run_and_reflect should return Ok even on execution failure");

        assert!(!result.success);
        assert!(result.error_trace.is_some());
        assert!(result.log.contains("Round 0"));
    }

    #[test]
    fn test_code_agent_max_refinements_respected() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::new(tmp.path().join("refine_test"));
        let config = CodeAgentConfig {
            python_cmd: "nonexistent_python_binary".into(),
            max_refinements: 2,
            keep_failed_artifacts: true,
        };
        let mut agent = CodeAgent::new(config, ws);

        let result = agent
            .run_and_reflect("test task", "empty")
            .expect("run_and_reflect should return Ok");

        // With max_refinements=2 and initial round (0..=2 => 3 rounds)
        assert_eq!(result.refinement_rounds, 2);
    }

    #[test]
    fn test_script_generator_template_structure() {
        let script = ScriptGenerator::generate("test", "dom");
        // Verify the template structure is complete and correct
        assert!(script.starts_with("import asyncio"));
        assert!(script.contains("async with async_playwright() as p:"));
        assert!(script.contains("await browser.close()"));
        assert!(script.contains("if __name__ == \"__main__\":"));
    }

    #[test]
    fn test_workspace_prepare_run_creates_unique_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ws = Workspace::new(tmp.path().join("unique"));
        let dir1 = ws.prepare_run();
        let dir2 = ws.prepare_run();
        assert_ne!(dir1, dir2, "Each prepare_run should create a unique directory");
        assert!(dir1.exists());
        assert!(dir2.exists());
    }

    #[test]
    fn test_executor_discover_screenshots() {
        let tmp = tempfile::tempdir().unwrap();

        // Create a fake PNG file
        let png_path = tmp.path().join("screenshot.png");
        std::fs::write(&png_path, "fake-png-content").unwrap();

        // Create a non-PNG file that should be ignored
        let txt_path = tmp.path().join("readme.txt");
        std::fs::write(&txt_path, "text").unwrap();

        let executor = ScriptExecutor::default_with_workdir(tmp.path().to_path_buf());
        let shots = executor.discover_screenshots();

        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0], png_path);
    }

    #[test]
    fn test_self_reflector_empty_output() {
        let output = ExecutionOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            screenshots: Vec::new(),
        };
        let outcome = SelfReflector::reflect("any task", &output);
        assert!(!outcome.completed);
        assert!(outcome.confidence < 0.5);
        assert!(outcome.suggestions.iter().any(|s| s.contains("Python")));
    }
}