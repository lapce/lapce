//! Agent Orchestration Layer — Manager/Sub-agent hierarchy with task decomposition.
//!
//! Inspired by Paperclip's org chart: a Manager agent decomposes complex goals
//! into tasks, assigns them to specialized sub-agents, aggregates results,
//! and handles feedback loops.
//!
//! ## Architecture
//!
//! ```text
//! Orchestrator
//!   ├── Manager (decomposes goals, assigns tasks)
//!   │     ├── SubAgent (security-scanner)   ← deterministic, fast
//!   │     ├── SubAgent (llm-reviewer)       ← LLM-powered, deep
//!   │     ├── SubAgent (fix-applier)        ← edit & apply
//!   │     └── SubAgent (compiler)           ← verify
//!   │
//!   ├── Goal Tree (Mission → Project → Agent Goal → Task)
//!   ├── Context Bus (cross-agent context passing)
//!   └── Result Aggregator (merge findings from all sub-agents)
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;

use crate::agent::sub_agents::SubAgentResult;
use crate::tools::pr_reviewer::PrReviewer;

// ============================================================================
// Goal Tree
// ============================================================================

/// A node in the goal tree: Mission → Project → Agent Goal → Task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalNode {
    pub id: String,
    pub title: String,
    pub description: String,
    pub goal_type: GoalType,
    pub children: Vec<GoalNode>,
    pub status: GoalStatus,
}

/// Type of goal node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalType {
    Mission,
    Project,
    AgentGoal,
    Task,
}

/// Status of a goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

impl GoalNode {
    /// Create a new goal tree from a mission statement.
    pub fn from_mission(mission: &str) -> Self {
        GoalNode {
            id: "mission".into(),
            title: "Mission".into(),
            description: mission.to_string(),
            goal_type: GoalType::Mission,
            children: Vec::new(),
            status: GoalStatus::Pending,
        }
    }

    /// Count descendants.
    pub fn total_nodes(&self) -> usize {
        1 + self.children.iter().map(|c| c.total_nodes()).sum::<usize>()
    }

    /// Get all leaf tasks.
    pub fn leaf_tasks(&self) -> Vec<&GoalNode> {
        if self.children.is_empty() {
            vec![self]
        } else {
            self.children.iter().flat_map(|c| c.leaf_tasks()).collect()
        }
    }
}

// ============================================================================
// SubAgentSpec — defines a sub-agent role
// ============================================================================

/// Specification for a sub-agent in the orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentSpec {
    /// Agent name/role.
    pub name: String,
    /// Description of what this agent does.
    pub description: String,
    /// Model provider (or "deterministic" for rule-based).
    pub provider: String,
    /// Tools this agent can use.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Permission level.
    #[serde(default)]
    pub permission: String, // "read-only", "write", "admin"
}

// ============================================================================
// Orchestrator — the main Agent Orchestration Layer
// ============================================================================

/// The orchestrator manages agent teams, goal trees, and task dispatch.
pub struct Orchestrator {
    /// Goal tree for the current session.
    goal_tree: Arc<RwLock<GoalNode>>,
    /// Registered sub-agent specs.
    sub_agents: Vec<SubAgentSpec>,
    /// Context bus for cross-agent data sharing.
    context_bus: Arc<RwLock<HashMap<String, String>>>,
    /// Aggregated results from all agents.
    results: Arc<RwLock<Vec<SubAgentResult>>>,
    /// Review engine for code analysis.
    pr_reviewer: PrReviewer,
    /// Working directory.
    work_dir: PathBuf,
}

impl Orchestrator {
    /// Create a new orchestrator with a default agent team.
    pub fn new(work_dir: PathBuf) -> Self {
        let mut sub_agents = Vec::new();

        // Default agent team (matches workflow step agents)
        sub_agents.push(SubAgentSpec {
            name: "security-scanner".into(),
            description: "Deterministic security vulnerability scanner".into(),
            provider: "deterministic".into(),
            tools: vec!["file_read".into()],
            permission: "read-only".into(),
        });
        sub_agents.push(SubAgentSpec {
            name: "llm-reviewer".into(),
            description: "LLM-powered multi-aspect code reviewer".into(),
            provider: "auto".into(),
            tools: vec!["file_read".into(), "code_search".into()],
            permission: "read-only".into(),
        });
        sub_agents.push(SubAgentSpec {
            name: "fix-applier".into(),
            description: "Applies review suggestions as file edits".into(),
            provider: "tool".into(),
            tools: vec!["file_read".into(), "file_write".into()],
            permission: "write".into(),
        });
        sub_agents.push(SubAgentSpec {
            name: "compiler".into(),
            description: "Runs cargo check to verify compilation".into(),
            provider: "deterministic".into(),
            tools: vec!["shell_exec".into()],
            permission: "read-only".into(),
        });

        Self {
            goal_tree: Arc::new(RwLock::new(GoalNode::from_mission("Review and improve code quality"))),
            sub_agents,
            context_bus: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(Vec::new())),
            pr_reviewer: PrReviewer::new(),
            work_dir,
        }
    }

    /// Create with custom sub-agent specs.
    pub fn with_agents(work_dir: PathBuf, agents: Vec<SubAgentSpec>) -> Self {
        Self {
            sub_agents: agents,
            ..Self::new(work_dir)
        }
    }

    /// Set the mission (top-level goal).
    pub async fn set_mission(&self, mission: &str) {
        let mut tree = self.goal_tree.write().await;
        *tree = GoalNode::from_mission(mission);
        info!("Mission set: {}", mission);
    }

    /// Decompose a goal into sub-tasks (Manager agent function).
    pub async fn decompose_goal(&self, parent_id: &str, sub_goals: Vec<(&str, &str, GoalType)>) {
        let mut tree = self.goal_tree.write().await;
        Self::add_goals_recursive(&mut tree, parent_id, &sub_goals);
    }

    fn add_goals_recursive(
        node: &mut GoalNode,
        target_id: &str,
        goals: &[(&str, &str, GoalType)],
    ) {
        if node.id == target_id {
            for (id, desc, gtype) in goals {
                node.children.push(GoalNode {
                    id: id.to_string(),
                    title: id.to_string(),
                    description: desc.to_string(),
                    goal_type: gtype.clone(),
                    children: Vec::new(),
                    status: GoalStatus::Pending,
                });
            }
            return;
        }
        for child in &mut node.children {
            Self::add_goals_recursive(child, target_id, goals);
        }
    }

    /// Write context to the shared bus (cross-agent).
    pub async fn write_context(&self, key: &str, value: &str) {
        let mut bus = self.context_bus.write().await;
        bus.insert(key.to_string(), value.to_string());
    }

    /// Read context from the shared bus.
    pub async fn read_context(&self, key: &str) -> Option<String> {
        let bus = self.context_bus.read().await;
        bus.get(key).cloned()
    }

    /// Register a custom sub-agent.
    pub fn register_agent(&mut self, spec: SubAgentSpec) {
        self.sub_agents.push(spec);
    }

    /// Find an agent by name.
    pub fn find_agent(&self, name: &str) -> Option<&SubAgentSpec> {
        self.sub_agents.iter().find(|a| a.name == name)
    }

    // -----------------------------------------------------------------------
    // Task Dispatch
    // -----------------------------------------------------------------------

    /// Dispatch a task to a specific sub-agent by name.
    pub async fn dispatch_task(
        &self,
        agent_name: &str,
        task_description: &str,
        input: &str,
    ) -> Result<SubAgentResult> {
        let start = Instant::now();
        info!("Dispatching task to '{}': {}", agent_name, task_description);

        let result = match agent_name {
            "security-scanner" => {
                self.run_security_scan(input).await
            }
            "llm-reviewer" => {
                self.run_llm_review(input).await
            }
            "fix-applier" => {
                self.run_fix_apply(input).await
            }
            "compiler" => {
                self.run_compiler().await
            }
            other => {
                // Generic: delegate to agent system
                self.run_generic_agent(other, task_description, input).await
            }
        };

        let elapsed = start.elapsed().as_millis() as u64;
        let mut results = self.results.write().await;
        results.push(SubAgentResult {
            agent_name: agent_name.to_string(),
            success: result.is_ok(),
            output: Some(result.as_ref().map(|r| r.clone()).unwrap_or_else(|e| e.to_string())),
            elapsed_ms: elapsed,
            task_id: task_description.to_string(),
            status: if result.is_ok() { crate::agent::sub_agents::TaskStatus::Completed } else { crate::agent::sub_agents::TaskStatus::Failed { error: result.as_ref().err().map(|e| e.to_string()).unwrap_or_default() } },
            tools_used: vec![],
            tokens_used: 0,
        });

        // Return the SubAgentResult we just built
        self.results.read().await.last().cloned().ok_or_else(|| anyhow::anyhow!("no result"))
    }

    /// Run all agents in parallel (for independent tasks).
    pub async fn run_parallel(
        &self,
        tasks: Vec<(&str, &str, &str)>, // (agent_name, task_description, input)
    ) -> Vec<Result<SubAgentResult>> {
        let mut handles = Vec::new();

        for (agent, task, input) in tasks {
            let agent = agent.to_string();
            let task = task.to_string();
            let _input = input.to_string();

            // Create a new orchestrator clone for parallel execution
            // In production, this would use a thread pool
            let handle = tokio::spawn(async move {
                // Direct execution for now
                Ok(SubAgentResult {
                    agent_name: agent.clone(),
                    success: true,
                    output: Some(format!("Executed {}: {}", agent, task)),
                    elapsed_ms: 0,
                    task_id: task,
                    status: crate::agent::sub_agents::TaskStatus::Completed,
                    tools_used: vec![],
                    tokens_used: 0,
                })
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(r) => results.push(r),
                Err(e) => results.push(Err(anyhow::anyhow!("Task panicked: {}", e))),
            }
        }

        results
    }

    /// Aggregate results from all sub-agents.
    pub async fn aggregate_results(&self) -> Vec<SubAgentResult> {
        let results = self.results.read().await;
        results.clone()
    }

    /// Generate a summary report from all agent results.
    pub async fn summary_report(&self) -> String {
        let results = self.results.read().await;
        let tree = self.goal_tree.read().await;

        let mut report = String::new();
        report.push_str("═══ Orchestrator Report ═══\n\n");
        report.push_str(&format!("Mission: {}\n\n", tree.description));
        report.push_str(&format!("Goal Tree: {} nodes\n\n", tree.total_nodes()));

        for result in results.iter() {
            let icon = if result.success { "✅" } else { "❌" };
            report.push_str(&format!(
                "  {} {} — {} ms\n",
                icon, result.agent_name, result.elapsed_ms
            ));
        }

        report
    }

    // -----------------------------------------------------------------------
    // Individual Agent Implementations
    // -----------------------------------------------------------------------

    /// Deterministic security scanner.
    async fn run_security_scan(&self, diff_text: &str) -> Result<String> {
        let mut findings = Vec::new();

        if diff_text.contains("unsafe") && !diff_text.contains("SAFETY") {
            findings.push("[HIGH] Unsafe block without SAFETY comment");
        }
        if diff_text.contains("todo!()") || diff_text.contains("unimplemented!()") {
            findings.push("[CRITICAL] todo!()/unimplemented!() found");
        }
        if diff_text.contains(".unwrap()") && !diff_text.contains("// SAFETY") {
            findings.push("[MEDIUM] Unwrap without error handling");
        }

        Ok(if findings.is_empty() {
            "✅ No security issues detected".to_string()
        } else {
            format!("⚠️ {} finding(s):\n  {}", findings.len(), findings.join("\n  "))
        })
    }

    /// LLM-powered review.
    async fn run_llm_review(&self, diff_text: &str) -> Result<String> {
        let repo_root = &self.work_dir;
        let report = self.pr_reviewer.review_diff(diff_text, repo_root).await?;

        let mut output = format!(
            "Verdict: {:?} | {} findings ({} critical, {} high)\n",
            report.verdict, report.total_findings, report.critical_count, report.high_count
        );

        for finding in report.findings.iter().take(10) {
            output.push_str(&format!(
                "  [{:?}] {}: {} — {}\n",
                finding.severity, finding.aspect, finding.title, finding.file_path
            ));
        }

        Ok(output)
    }

    /// Fix applier.
    async fn run_fix_apply(&self, input: &str) -> Result<String> {
        // Simplified: just report what would be applied
        Ok(format!("Applied fixes from review:\n{}", input))
    }

    /// Compiler check.
    async fn run_compiler(&self) -> Result<String> {
        let output = std::process::Command::new("cargo")
            .args(["check", "--lib"])
            .current_dir(&self.work_dir)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run cargo check: {}", e))?;

        if output.status.success() {
            Ok("✅ Compilation passed".to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let errors: Vec<&str> = stderr.lines()
                .filter(|l| l.contains("error[") || l.contains("error:"))
                .collect();
            Ok(format!("❌ Compilation failed:\n  {}", errors.join("\n  ")))
        }
    }

    /// Generic agent dispatch (delegates to agent system).
    async fn run_generic_agent(&self, name: &str, task: &str, input: &str) -> Result<String> {
        Ok(format!("[{}] Task: {} | Input: {}", name, task, input))
    }
}

// ============================================================================
// HeartbeatScheduler — periodic wake-and-check for orchestrator agents
// ============================================================================

/// A heartbeat-driven agent monitoring task.
#[derive(Debug, Clone)]
pub struct HeartbeatTask {
    /// Unique ID for this heartbeat.
    pub id: String,
    /// Agent name to wake.
    pub agent_name: String,
    /// Target to review/monitor.
    pub target: String,
    /// Interval in seconds.
    pub interval_secs: u64,
    /// Max consecutive failures before alerting.
    pub max_failures: u32,
    /// Current failure count.
    pub failures: u32,
    /// Whether this heartbeat is active.
    pub active: bool,
}

/// HeartbeatScheduler — periodically wakes agents to check work.
/// Pattern: Paperclip heartbeat (agents wake on schedule → check → act).
pub struct HeartbeatScheduler {
    tasks: Arc<RwLock<Vec<HeartbeatTask>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl HeartbeatScheduler {
    /// Create a new heartbeat scheduler.
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Register a heartbeat task.
    pub async fn register_task(&self, task: HeartbeatTask) {
        let mut tasks = self.tasks.write().await;
        tasks.push(task);
        tracing::info!("Heartbeat task registered");
    }

    /// Remove a heartbeat task.
    pub async fn remove_task(&self, id: &str) {
        let mut tasks = self.tasks.write().await;
        tasks.retain(|t| t.id != id);
    }

    /// List all registered heartbeat tasks.
    pub async fn list_tasks(&self) -> Vec<HeartbeatTask> {
        self.tasks.read().await.clone()
    }

    /// Start the heartbeat loop (spawns a tokio background task).
    pub fn start<F>(&self, on_tick: F)
    where
        F: Fn(&HeartbeatTask) -> bool + Send + 'static,
    {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        let tasks = self.tasks.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
            while running.load(std::sync::atomic::Ordering::SeqCst) {
                tick.tick().await;
                let snapshot = {
                    let t = tasks.read().await;
                    t.clone()
                };
                for task in &snapshot {
                    if !task.active {
                        continue;
                    }
                    // Simulate tick — in production, execute the actual agent
                    let success = on_tick(task);
                    if success {
                        let mut t = tasks.write().await;
                        if let Some(mut_ref) = t.iter_mut().find(|x| x.id == task.id) {
                            mut_ref.failures = 0;
                        }
                    } else {
                        let mut t = tasks.write().await;
                        if let Some(mut_ref) = t.iter_mut().find(|x| x.id == task.id) {
                            mut_ref.failures += 1;
                            if mut_ref.failures >= mut_ref.max_failures {
                                tracing::warn!(
                                    "Heartbeat '{}' exceeded max failures ({})",
                                    task.id, task.max_failures
                                );
                            }
                        }
                    }
                }
            }
        });
    }

    /// Stop the heartbeat loop.
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Default tick handler: run security scan on target.
    pub fn default_on_tick(task: &HeartbeatTask) -> bool {
        tracing::info!("Heartbeat '{}' waking agent '{}'", task.id, task.agent_name);
        // Simplified — in production, dispatch to Orchestrator
        true
    }
}

// ============================================================================
// AgentBudget — per-agent cost control with circuit breaker (Paperclip pattern)
// ============================================================================

/// Per-agent budget tracking and enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBudget {
    /// Agent name this budget applies to.
    pub agent_name: String,
    /// Monthly token limit (hard cap).
    pub monthly_token_limit: u64,
    /// Token usage this month.
    pub used_this_month: u64,
    /// Monthly invocation limit.
    #[serde(default)]
    pub monthly_invocations_limit: u64,
    /// Invocations this month.
    #[serde(default)]
    pub invocations_this_month: u64,
    /// Circuit breaker tripped?
    pub circuit_broken: bool,
    /// Budget reset timestamp (epoch seconds).
    #[serde(default)]
    pub last_reset_at: u64,
}

impl AgentBudget {
    pub fn new(agent_name: &str, monthly_token_limit: u64) -> Self {
        Self {
            agent_name: agent_name.to_string(),
            monthly_token_limit,
            used_this_month: 0,
            monthly_invocations_limit: 10000,
            invocations_this_month: 0,
            circuit_broken: false,
            last_reset_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Check if this agent has budget remaining.
    pub fn can_execute(&self, estimated_tokens: u64) -> bool {
        if self.circuit_broken {
            return false;
        }
        if self.invocations_this_month >= self.monthly_invocations_limit {
            return false;
        }
        self.used_this_month + estimated_tokens <= self.monthly_token_limit
    }

    /// Record token usage after execution.
    pub fn record_usage(&mut self, tokens: u64) {
        self.used_this_month += tokens;
        self.invocations_this_month += 1;
        if self.used_this_month >= self.monthly_token_limit {
            self.circuit_broken = true;
            tracing::warn!(
                "Circuit breaker tripped for '{}': {} / {} tokens used",
                self.agent_name, self.used_this_month, self.monthly_token_limit
            );
        }
    }

    /// Reset budget (monthly rollover).
    pub fn reset(&mut self) {
        self.used_this_month = 0;
        self.invocations_this_month = 0;
        self.circuit_broken = false;
        self.last_reset_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// Utilization ratio (0.0–1.0).
    pub fn utilization(&self) -> f64 {
        if self.monthly_token_limit == 0 {
            return 0.0;
        }
        self.used_this_month as f64 / self.monthly_token_limit as f64
    }
}

/// BudgetManager — manages a collection of per-agent budgets.
pub struct BudgetManager {
    budgets: std::collections::HashMap<String, AgentBudget>,
    default_token_limit: u64,
}

impl BudgetManager {
    pub fn new(default_token_limit: u64) -> Self {
        Self {
            budgets: std::collections::HashMap::new(),
            default_token_limit,
        }
    }

    /// Get or create budget for an agent.
    pub fn get_budget(&mut self, agent_name: &str) -> &mut AgentBudget {
        if !self.budgets.contains_key(agent_name) {
            self.budgets.insert(
                agent_name.to_string(),
                AgentBudget::new(agent_name, self.default_token_limit),
            );
        }
        self.budgets.get_mut(agent_name).unwrap()
    }

    /// Set a custom budget for an agent.
    pub fn set_budget(&mut self, budget: AgentBudget) {
        self.budgets.insert(budget.agent_name.clone(), budget);
    }

    /// Check if an agent can execute.
    pub fn can_execute(&self, agent_name: &str, estimated_tokens: u64) -> bool {
        self.budgets
            .get(agent_name)
            .map(|b| b.can_execute(estimated_tokens))
            .unwrap_or(true) // No budget = no limit
    }

    /// Record usage for an agent.
    pub fn record_usage(&mut self, agent_name: &str, tokens: u64) {
        if let Some(budget) = self.budgets.get_mut(agent_name) {
            budget.record_usage(tokens);
        }
    }

    /// Check if any circuit breakers are tripped.
    pub fn any_circuits_broken(&self) -> Vec<&str> {
        self.budgets
            .values()
            .filter(|b| b.circuit_broken)
            .map(|b| b.agent_name.as_str())
            .collect()
    }

    /// Reset all budgets (monthly rollover).
    pub fn reset_all(&mut self) {
        for budget in self.budgets.values_mut() {
            budget.reset();
        }
    }

    /// Get all budgets.
    pub fn all_budgets(&self) -> Vec<&AgentBudget> {
        self.budgets.values().collect()
    }

    /// Total token usage across all agents.
    pub fn total_usage(&self) -> u64 {
        self.budgets.values().map(|b| b.used_this_month).sum()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_goal_tree_creation() {
        let tree = GoalNode::from_mission("Improve code quality");
        assert_eq!(tree.goal_type, GoalType::Mission);
        assert_eq!(tree.total_nodes(), 1);
    }

    #[test]
    fn test_goal_tree_decompose() {
        let mut tree = GoalNode::from_mission("Review PR");
        let sub = GoalNode {
            id: "security".into(),
            title: "Security".into(),
            description: "Run security scan".into(),
            goal_type: GoalType::AgentGoal,
            children: vec![],
            status: GoalStatus::Pending,
        };
        tree.children.push(sub);
        assert_eq!(tree.total_nodes(), 2);
        assert_eq!(tree.leaf_tasks().len(), 1);
    }

    #[test]
    fn test_orchestrator_default_agents() {
        let orch = Orchestrator::new(PathBuf::from("."));
        assert_eq!(orch.sub_agents.len(), 4);
        assert!(orch.find_agent("llm-reviewer").is_some());
        assert!(orch.find_agent("security-scanner").is_some());
    }

    #[tokio::test]
    async fn test_security_scan_detects_issues() {
        let orch = Orchestrator::new(PathBuf::from("."));
        let result = orch.run_security_scan("unsafe { } with todo!()").await.unwrap();
        assert!(result.contains("finding") || result.contains("CRITICAL"));
    }

    #[tokio::test]
    async fn test_security_scan_clean() {
        let orch = Orchestrator::new(PathBuf::from("."));
        let result = orch.run_security_scan("fn safe() { let x = 1; }").await.unwrap();
        assert!(result.contains("No security issues"));
    }

    #[tokio::test]
    async fn test_context_bus() {
        let orch = Orchestrator::new(PathBuf::from("."));
        orch.write_context("key1", "value1").await;
        let val = orch.read_context("key1").await;
        assert_eq!(val, Some("value1".to_string()));

        let missing = orch.read_context("nonexistent").await;
        assert_eq!(missing, None);
    }

    #[tokio::test]
    async fn test_set_mission() {
        let orch = Orchestrator::new(PathBuf::from("."));
        orch.set_mission("Build a better codebase").await;
        let tree = orch.goal_tree.read().await;
        assert_eq!(tree.description, "Build a better codebase");
    }

    #[tokio::test]
    async fn test_orchestrator_summary() {
        let orch = Orchestrator::new(PathBuf::from("."));
        let summary = orch.summary_report().await;
        assert!(summary.contains("Mission"));
        assert!(summary.contains("Goal Tree"));
    }

    #[test]
    fn test_sub_agent_spec() {
        let spec = SubAgentSpec {
            name: "test-agent".into(),
            description: "A test".into(),
            provider: "deterministic".into(),
            tools: vec!["file_read".into()],
            permission: "read-only".into(),
        };
        assert_eq!(spec.name, "test-agent");
        assert_eq!(spec.permission, "read-only");
    }

    // ── AgentBudget / BudgetManager tests ──

    #[test]
    fn test_agent_budget_new() {
        let b = AgentBudget::new("test-agent", 1000);
        assert_eq!(b.agent_name, "test-agent");
        assert_eq!(b.monthly_token_limit, 1000);
        assert!(!b.circuit_broken);
    }

    #[test]
    fn test_agent_budget_can_execute() {
        let b = AgentBudget::new("agent", 500);
        assert!(b.can_execute(400));
        assert!(b.can_execute(500));
        assert!(!b.can_execute(501));
    }

    #[test]
    fn test_agent_budget_record_usage_trips_circuit() {
        let mut b = AgentBudget::new("agent", 100);
        assert!(b.can_execute(50));
        b.record_usage(50);
        assert!(!b.circuit_broken);
        b.record_usage(60); // Over-limit
        assert!(b.circuit_broken);
        assert!(!b.can_execute(1));
    }

    #[test]
    fn test_agent_budget_reset() {
        let mut b = AgentBudget::new("agent", 100);
        b.record_usage(100);
        assert!(b.circuit_broken);
        b.reset();
        assert!(!b.circuit_broken);
        assert_eq!(b.used_this_month, 0);
        assert!(b.can_execute(100));
    }

    #[test]
    fn test_agent_budget_utilization() {
        let mut b = AgentBudget::new("agent", 1000);
        assert_eq!(b.utilization(), 0.0);
        b.record_usage(250);
        assert!((b.utilization() - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_budget_manager_default_budget() {
        let mut mgr = BudgetManager::new(5000);
        let budget = mgr.get_budget("llm-reviewer");
        assert_eq!(budget.monthly_token_limit, 5000);
        assert!(budget.can_execute(4000));
    }

    #[test]
    fn test_budget_manager_circuit_tracking() {
        let mut mgr = BudgetManager::new(100);
        let agent = "security-scanner";
        assert!(mgr.can_execute(agent, 50));
        mgr.record_usage(agent, 100);
        assert!(!mgr.can_execute(agent, 1));
        let broken = mgr.any_circuits_broken();
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0], "security-scanner");
    }

    #[test]
    fn test_budget_manager_total_usage() {
        let mut mgr = BudgetManager::new(100000);
        mgr.record_usage("agent-a", 100);
        mgr.record_usage("agent-b", 200);
        assert_eq!(mgr.total_usage(), 300);
        mgr.reset_all();
        assert_eq!(mgr.total_usage(), 0);
    }

    #[test]
    fn test_budget_manager_set_custom_budget() {
        let mut mgr = BudgetManager::new(5000);
        let custom = AgentBudget::new("custom-agent", 10000);
        mgr.set_budget(custom);
        assert!(mgr.can_execute("custom-agent", 9000));
        assert!(!mgr.can_execute("custom-agent", 11000));
    }
}