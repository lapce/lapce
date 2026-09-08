//! Workflow DAG — YAML-defined review pipelines with feedback loops.
//!
//! Inspired by Paperclip's approach: define multi-agent workflows where each step
//! has a role, input/output, and conditional feedback (on_reject/on_fail goto).
//!
//! ## Example workflow
//!
//! ```yaml
//! name: carp-review-pipeline
//! max_iterations: 3
//! steps:
//!   - id: security-scan
//!     agent: security-scanner
//!     task: "Run deterministic security checks"
//!
//!   - id: llm-review
//!     agent: llm-reviewer
//!     task: "Multi-aspect LLM review"
//!     input_from: security-scan
//!     on_critical: abort
//!
//!   - id: apply-fixes
//!     agent: fix-applier
//!     task: "Apply HIGH+ suggestions"
//!     input_from: llm-review
//!     on_fail: goto llm-review
//!
//!   - id: verify
//!     agent: compiler
//!     task: "Run cargo check"
//!     input_from: apply-fixes
//!     on_fail: goto llm-review
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::{
    ReviewEngine, ReviewSession,
    DiffTarget,
};

// ============================================================================
// WorkflowStep — a single step in the pipeline
// ============================================================================

/// Action to take when a step fails or produces critical findings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum StepOnAction {
    /// Abort the entire workflow.
    Abort,
    /// Go to another step by ID (feedback loop).
    Goto(String),
    /// Notify but continue.
    Notify,
    /// Retry the same step (with max iterations).
    Retry,
}

/// A single workflow step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Unique ID for this step.
    pub id: String,
    /// Agent role (security-scanner, llm-reviewer, fix-applier, compiler).
    pub agent: String,
    /// Task description.
    pub task: String,
    /// Input from previous step ID (optional — depends on pipeline).
    #[serde(default)]
    pub input_from: Option<String>,
    /// Output context key for downstream steps.
    #[serde(default)]
    pub input_from_last: Option<bool>,
    /// Action on critical/fatal findings.
    #[serde(default)]
    pub on_critical: Option<StepOnAction>,
    /// Action on step failure.
    #[serde(default)]
    pub on_fail: Option<StepOnAction>,
    /// Action on rejection (for review steps).
    #[serde(default)]
    pub on_reject: Option<StepOnAction>,
}

// ============================================================================
// WorkflowDef — YAML workflow definition
// ============================================================================

/// A complete workflow definition (YAML-serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    /// Workflow name.
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Maximum iterations for the entire pipeline (anti-infinite-loop).
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// Workflow steps.
    pub steps: Vec<WorkflowStep>,
}

fn default_max_iterations() -> usize { 5 }

impl WorkflowDef {
    /// Load from a YAML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read workflow at {}", path.display()))?;
        Self::from_yaml(&content)
    }

    /// Parse from YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        let def: WorkflowDef = serde_yaml::from_str(yaml)
            .with_context(|| "Failed to parse workflow YAML")?;

        // Validate: ensure all goto targets exist
        for step in &def.steps {
            if let Some(StepOnAction::Goto(ref target)) = step.on_critical {
                if !def.steps.iter().any(|s| s.id == *target) {
                    anyhow::bail!("Workflow '{}': on_critical goto '{}' not found in steps", def.name, target);
                }
            }
            if let Some(StepOnAction::Goto(ref target)) = step.on_fail {
                if !def.steps.iter().any(|s| s.id == *target) {
                    anyhow::bail!("Workflow '{}': on_fail goto '{}' not found in steps", def.name, target);
                }
            }
        }

        Ok(def)
    }

    /// Get the built-in default review workflow.
    pub fn default_review() -> Self {
        WorkflowDef {
            name: "carp-review-pipeline".into(),
            description: Some("Default multi-stage code review pipeline".into()),
            max_iterations: 5,
            steps: vec![
                WorkflowStep {
                    id: "security-scan".into(),
                    agent: "security-scanner".into(),
                    task: "Run deterministic security checks (unsafe blocks, SQL injection, etc.)".into(),
                    input_from: None,
                    input_from_last: None,
                    on_critical: Some(StepOnAction::Abort),
                    on_fail: Some(StepOnAction::Abort),
                    on_reject: None,
                },
                WorkflowStep {
                    id: "llm-review".into(),
                    agent: "llm-reviewer".into(),
                    task: "Multi-aspect LLM review: correctness, performance, style, tests".into(),
                    input_from: None,
                    input_from_last: Some(true),
                    on_critical: Some(StepOnAction::Abort),
                    on_fail: Some(StepOnAction::Goto("security-scan".into())),
                    on_reject: Some(StepOnAction::Retry),
                },
                WorkflowStep {
                    id: "apply-fixes".into(),
                    agent: "fix-applier".into(),
                    task: "Apply HIGH+ severity suggestions from review".into(),
                    input_from: Some("llm-review".into()),
                    input_from_last: None,
                    on_critical: None,
                    on_fail: Some(StepOnAction::Goto("llm-review".into())),
                    on_reject: None,
                },
                WorkflowStep {
                    id: "verify".into(),
                    agent: "compiler".into(),
                    task: "Run cargo check to verify fixes compile".into(),
                    input_from: Some("apply-fixes".into()),
                    input_from_last: None,
                    on_critical: None,
                    on_fail: Some(StepOnAction::Goto("llm-review".into())),
                    on_reject: None,
                },
            ],
        }
    }

    /// Generate a template YAML for users.
    pub fn template() -> String {
        r#"# deepseek-carp Review Workflow
# Define pipeline steps, agent roles, and feedback loops.
name: my-review-pipeline
description: "Custom review pipeline"
max_iterations: 5

steps:
  # Step 1: Deterministic security scan (fast, no LLM needed)
  - id: security-scan
    agent: security-scanner
    task: "Run deterministic security checks"
    on_critical: abort

  # Step 2: LLM-powered multi-aspect review
  - id: llm-review
    agent: llm-reviewer
    task: "Review code for correctness, performance, style, and test coverage"
    input_from: security-scan
    on_critical: abort
    on_fail: goto security-scan
    on_reject: retry

  # Step 3: Auto-apply high-severity fixes
  - id: apply-fixes
    agent: fix-applier
    task: "Apply HIGH+ suggestions from review"
    input_from: llm-review
    on_fail: goto llm-review

  # Step 4: Verify compilation
  - id: verify
    agent: compiler
    task: "Run cargo check"
    input_from: apply-fixes
    on_fail: goto llm-review
"#.to_string()
    }
}

// ============================================================================
// WorkflowEngine — executes a WorkflowDef against a target
// ============================================================================

/// State of a single workflow run.
#[derive(Debug)]
pub struct WorkflowRun {
    pub workflow: WorkflowDef,
    pub target: DiffTarget,
    pub iteration: usize,
    pub step_results: HashMap<String, StepResult>,
    pub timeline: Vec<WorkflowEvent>,
    pub final_session: Option<ReviewSession>,
}

/// Result of a single step execution.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_id: String,
    pub status: StepStatus,
    pub findings_count: usize,
    pub critical_count: usize,
    pub has_errors: bool,
}

/// Status of a step.
#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Passed,
    Failed,
    Skipped,
    Aborted,
}

/// A timed event in the workflow timeline.
#[derive(Debug, Clone)]
pub struct WorkflowEvent {
    pub step_id: String,
    pub event_type: String, // "start", "pass", "fail", "goto", "abort", "retry"
    pub message: String,
}

/// Engine that executes a WorkflowDef.
pub struct WorkflowEngine {
    /// Underlying ReviewEngine for the actual review work.
    review_engine: ReviewEngine,
}

impl WorkflowEngine {
    pub fn new(review_engine: ReviewEngine) -> Self {
        Self { review_engine }
    }

    /// Run a workflow against a target.
    pub async fn run(
        &self,
        workflow: &WorkflowDef,
        target: &DiffTarget,
    ) -> anyhow::Result<WorkflowRun> {
        let mut run = WorkflowRun {
            workflow: workflow.clone(),
            target: target.clone(),
            iteration: 0,
            step_results: HashMap::new(),
            timeline: Vec::new(),
            final_session: None,
        };

        // Index steps by ID for fast lookup
        let step_map: HashMap<&str, &WorkflowStep> = workflow.steps.iter()
            .map(|s| (s.id.as_str(), s))
            .collect();

        // Determine execution order (topological, but defaults to declaration order)
        let order = self.execution_order(&workflow.steps);

        while run.iteration < workflow.max_iterations {
            run.iteration += 1;
            info!("Workflow iteration {}/{}", run.iteration, workflow.max_iterations);

            // Track per-iteration status
            let mut iteration_aborted = false;

            for step_id in &order {
                let step = &step_map[step_id.as_str()];
                if iteration_aborted {
                    // Mark remaining steps as skipped
                    run.step_results.insert(step_id.clone(), StepResult {
                        step_id: step_id.clone(),
                        status: StepStatus::Skipped,
                        findings_count: 0,
                        critical_count: 0,
                        has_errors: false,
                    });
                    run.timeline.push(WorkflowEvent {
                        step_id: step_id.clone(),
                        event_type: "skip".into(),
                        message: format!("Step '{}' skipped due to earlier abort", step_id),
                    });
                    continue;
                }

                run.timeline.push(WorkflowEvent {
                    step_id: step_id.clone(),
                    event_type: "start".into(),
                    message: format!("Step '{}': {}", step_id, step.task),
                });

                // Execute the step
                let mut result = self.execute_step(step, target, &run).await?;

                let is_success = result.status == StepStatus::Passed;

                // Handle failure actions
                if !is_success {
                    // Check for goto
                    if let Some(ref on_fail) = step.on_fail {
                        match on_fail {
                            StepOnAction::Goto(ref target_step) => {
                                run.timeline.push(WorkflowEvent {
                                    step_id: step_id.clone(),
                                    event_type: "goto".into(),
                                    message: format!("Step '{}' failed → goto '{}'", step_id, target_step),
                                });
                                // Find the index of target step and restart from there
                                // For now, break inner loop — outer loop handles iteration
                                break;
                            }
                            StepOnAction::Abort => {
                                iteration_aborted = true;
                                run.timeline.push(WorkflowEvent {
                                    step_id: step_id.clone(),
                                    event_type: "abort".into(),
                                    message: format!("Step '{}' failed → aborting pipeline", step_id),
                                });
                                result.status = StepStatus::Aborted;
                            }
                            StepOnAction::Retry => {
                                run.timeline.push(WorkflowEvent {
                                    step_id: step_id.clone(),
                                    event_type: "retry".into(),
                                    message: format!("Step '{}' failed → retrying", step_id),
                                });
                                // Re-run same step
                                continue;
                            }
                            StepOnAction::Notify => {
                                run.timeline.push(WorkflowEvent {
                                    step_id: step_id.clone(),
                                    event_type: "notify".into(),
                                    message: format!("Step '{}' failed, but continuing", step_id),
                                });
                            }
                        }
                    }
                }

                // Handle critical findings
                if result.critical_count > 0 {
                    if let Some(ref on_critical) = step.on_critical {
                        if on_critical == &StepOnAction::Abort {
                            iteration_aborted = true;
                            run.timeline.push(WorkflowEvent {
                                step_id: step_id.clone(),
                                event_type: "abort".into(),
                                message: format!("Critical findings in '{}' → aborting", step_id),
                            });
                        }
                    }
                }

                run.step_results.insert(step_id.clone(), result);
            }

            if iteration_aborted {
                break; // Abort terminates the entire workflow
            }

            // Check if all steps passed
            let all_passed = order.iter().all(|id| {
                run.step_results.get(id)
                    .map(|r| r.status == StepStatus::Passed)
                    .unwrap_or(false)
            });

            if all_passed {
                info!("Workflow '{}' completed successfully", workflow.name);
                break;
            }
        }

        // Generate final session summary
        run.final_session = Some(ReviewSession {
            target: target.clone(),
            report: Default::default(),
            annotations: vec![],
            apply_results: vec![],
            verify_result: None,
            elapsed_ms: 0,
        });

        Ok(run)
    }

    /// Determine execution order (topological sort based on input_from).
    fn execution_order(&self, steps: &[WorkflowStep]) -> Vec<String> {
        // Simple approach: order by declaration, ensure dependencies come first
        let mut order: Vec<String> = Vec::new();
        let mut added = std::collections::HashSet::new();

        for step in steps {
            self.add_with_deps(step, steps, &mut order, &mut added);
        }

        order
    }

    fn add_with_deps(
        &self,
        step: &WorkflowStep,
        all_steps: &[WorkflowStep],
        order: &mut Vec<String>,
        added: &mut std::collections::HashSet<String>,
    ) {
        if added.contains(&step.id) {
            return;
        }

        // Add dependency first
        if let Some(ref dep_id) = step.input_from {
            if let Some(dep_step) = all_steps.iter().find(|s| s.id == *dep_id) {
                self.add_with_deps(dep_step, all_steps, order, added);
            }
        }

        added.insert(step.id.clone());
        order.push(step.id.clone());
    }

    /// Execute a single workflow step.
    async fn execute_step(
        &self,
        step: &WorkflowStep,
        target: &DiffTarget,
        _run: &WorkflowRun,
    ) -> anyhow::Result<StepResult> {
        match step.agent.as_str() {
            "security-scanner" => {
                // Run SecurityScannerV2 deterministic checks
                let diff_text = target.to_diff_text();
                let findings = self.run_security_scan(&diff_text, target);
                let critical = findings.iter().filter(|f| f.contains("[CRITICAL]") || f.contains("[HIGH]")).count();
                Ok(StepResult {
                    step_id: step.id.clone(),
                    status: if critical > 0 { StepStatus::Failed } else { StepStatus::Passed },
                    findings_count: findings.len(),
                    critical_count: critical,
                    has_errors: critical > 0,
                })
            }
            "llm-reviewer" => {
                // Run PrReviewer multi-aspect analysis
                let _diff_text = target.to_diff_text();
                let _repo_root = target.working_dir();
                let report = self.review_engine.review(target, None).await?;

                let critical = report.critical_count;
                let high = report.high_count;

                Ok(StepResult {
                    step_id: step.id.clone(),
                    status: if critical > 0 { StepStatus::Failed } else { StepStatus::Passed },
                    findings_count: report.total_findings,
                    critical_count: critical,
                    has_errors: critical > 0 || high > 5,
                })
            }
            "fix-applier" => {
                // Apply HIGH+ suggestions
                // Re-run review to get fresh findings
                let report = self.review_engine.review(target, None).await?;
                // Apply logic — depends on whether review session stores results
                let apply_results = ReviewEngine::apply_high_severity_fixes(&report);
                let failed = apply_results.iter().filter(|r| {
                    matches!(r.result, crate::tools::diff::EditResult::Failed { .. })
                }).count();
                Ok(StepResult {
                    step_id: step.id.clone(),
                    status: if failed > 0 { StepStatus::Failed } else { StepStatus::Passed },
                    findings_count: apply_results.len(),
                    critical_count: 0,
                    has_errors: failed > 0,
                })
            }
            "compiler" => {
                // Run cargo check
                let verify = ReviewEngine::verify_compilation(&target.working_dir());
                Ok(StepResult {
                    step_id: step.id.clone(),
                    status: if verify.passed { StepStatus::Passed } else { StepStatus::Failed },
                    findings_count: verify.error_count,
                    critical_count: verify.error_count,
                    has_errors: !verify.passed,
                })
            }
            _ => {
                // Unknown agent: skip
                Ok(StepResult {
                    step_id: step.id.clone(),
                    status: StepStatus::Skipped,
                    findings_count: 0,
                    critical_count: 0,
                    has_errors: false,
                })
            }
        }
    }

    /// Simplified security scan that returns finding strings.
    fn run_security_scan(&self, diff_text: &str, _target: &DiffTarget) -> Vec<String> {
        let mut findings = Vec::new();

        // Basic deterministic checks
        if diff_text.contains("unsafe") && !diff_text.contains("SAFETY") {
            findings.push("[HIGH] Unsafe block without SAFETY comment".to_string());
        }
        if diff_text.contains("todo!()") || diff_text.contains("unimplemented!()") {
            findings.push("[CRITICAL] todo!() or unimplemented!() found in code".to_string());
        }
        if diff_text.contains(".unwrap()") || diff_text.contains(".expect(") {
            findings.push("[MEDIUM] Unwrap/expect without error handling".to_string());
        }

        findings
    }

    /// Format workflow run results for display.
    pub fn format_workflow_result(run: &WorkflowRun) -> String {
        use std::fmt::Write;

        let mut output = String::new();

        writeln!(output, "\n═══ Workflow: {} ═══\n", run.workflow.name).ok();
        if let Some(ref desc) = run.workflow.description {
            writeln!(output, "  {}", desc).ok();
        }
        writeln!(output, "  Iterations: {}/{}\n", run.iteration, run.workflow.max_iterations).ok();

        // Print timeline
        writeln!(output, "--- Timeline ---").ok();
        for event in &run.timeline {
            let icon = match event.event_type.as_str() {
                "start" => "▶️",
                "pass" | "passed" => "✅",
                "fail" | "failed" => "❌",
                "goto" => "🔁",
                "abort" => "🛑",
                "retry" => "🔄",
                "skip" => "⏭️",
                _ => "  ",
            };
            writeln!(output, "  {} {}", icon, event.message).ok();
        }

        writeln!(output, "\n--- Step Results ---").ok();
        for (id, result) in &run.step_results {
            let status_icon = match result.status {
                StepStatus::Passed => "✅",
                StepStatus::Failed => "❌",
                StepStatus::Skipped => "⏭️",
                StepStatus::Aborted => "🛑",
                StepStatus::Running => "▶️",
                StepStatus::Pending => "⏳",
            };
            writeln!(output, "  {} {} — {} findings ({} critical)",
                status_icon, id, result.findings_count, result.critical_count).ok();
        }

        output
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_from_yaml() {
        let yaml = r#"
name: test-pipeline
max_iterations: 3
steps:
  - id: step1
    agent: security-scanner
    task: "Scan for issues"
    on_critical: abort
  - id: step2
    agent: llm-reviewer
    task: "Review code"
    input_from: step1
    on_fail: goto step1
"#;
        let wf = WorkflowDef::from_yaml(yaml).unwrap();
        assert_eq!(wf.name, "test-pipeline");
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.steps[0].id, "step1");
    }

    #[test]
    fn test_workflow_validation_invalid_goto() {
        let yaml = r#"
name: bad-pipeline
steps:
  - id: step1
    agent: test
    task: "test"
    on_fail: goto nonexistent
"#;
        let result = WorkflowDef::from_yaml(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_review_workflow() {
        let wf = WorkflowDef::default_review();
        assert_eq!(wf.name, "carp-review-pipeline");
        assert_eq!(wf.steps.len(), 4);
        assert_eq!(wf.steps[0].agent, "security-scanner");
        assert_eq!(wf.steps[3].agent, "compiler");
    }

    #[test]
    fn test_execution_order() {
        let wf = WorkflowDef::default_review();
        let engine = WorkflowEngine::new(
            ReviewEngine::new(&std::path::Path::new("."))
        );
        let order = engine.execution_order(&wf.steps);
        assert_eq!(order.len(), 4);
        // security-scan should be first
        assert_eq!(order[0], "security-scan");
        // apply-fixes depends on llm-review
        let llm_idx = order.iter().position(|s| s == "llm-review").unwrap();
        let apply_idx = order.iter().position(|s| s == "apply-fixes").unwrap();
        assert!(llm_idx < apply_idx);
    }

    #[test]
    fn test_workflow_template() {
        let template = WorkflowDef::template();
        assert!(template.contains("my-review-pipeline"));
        assert!(template.contains("security-scanner"));
        assert!(template.contains("llm-reviewer"));
        assert!(template.contains("fix-applier"));
        assert!(template.contains("compiler"));
    }

    #[test]
    fn test_run_security_scan_findings() {
        let engine = WorkflowEngine::new(
            ReviewEngine::new(&std::path::Path::new("."))
        );
        let findings = engine.run_security_scan(
            "unsafe { ... } with todo!()",
            &DiffTarget::Raw("test".into()),
        );
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.contains("SAFETY")));
        assert!(findings.iter().any(|f| f.contains("todo")));
    }

    #[test]
    fn test_step_result_defaults() {
        let result = StepResult {
            step_id: "test".into(),
            status: StepStatus::Passed,
            findings_count: 0,
            critical_count: 0,
            has_errors: false,
        };
        assert_eq!(result.status, StepStatus::Passed);
    }
}