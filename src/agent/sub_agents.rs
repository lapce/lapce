//! SubAgent pool — parallel task execution with semaphore control.
//!
//! Ported from CarpAI's `src/sub_agents.rs` and Swarm architecture.
//! Allows spawning multiple sub-agents to execute independent tasks in parallel,
//! sharing the same provider pool and tool registry.
//!
//! ## Architecture
//!
//! ```text
//! Main Agent
//!   └── SubAgentPool { max_concurrent: 4 }
//!         ├── SubAgent 1: "Refactor auth module"
//!         ├── SubAgent 2: "Write tests for utils"
//!         ├── SubAgent 3: "Update documentation"
//!         └── SubAgent 4: "Fix clippy warnings"
//! ```

use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::agent::{Agent, AgentConfig};
use crate::config::DeepSeekConfig;
use crate::providers::orchestrator::ProviderOrchestrator;

/// A single sub-agent task.
#[derive(Debug, Clone)]
pub struct SubAgentTask {
    /// Unique task ID.
    pub id: String,
    /// Human-readable description of what to do.
    pub instruction: String,
    /// Optional file context (e.g., "src/auth/").
    pub context: Option<String>,
    /// Task status.
    pub status: TaskStatus,
    /// Number of retry attempts made.
    pub retry_count: u32,
}

/// Status of a sub-agent task.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed { error: String },
    /// Timed out before completing.
    TimedOut,
}

/// Result from executing a sub-agent task.
#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub output: Option<String>,
    pub tools_used: Vec<String>,
    pub tokens_used: u32,
    /// Agent name that produced this result (used by Orchestrator).
    pub agent_name: String,
    /// Whether the task succeeded (used by Orchestrator summary).
    pub success: bool,
    /// Wall-clock elapsed time in milliseconds (used by Orchestrator summary).
    pub elapsed_ms: u64,
}

/// A pool of sub-agents for parallel task execution.
///
/// Uses a semaphore to limit concurrent agent execution.
/// Each sub-agent gets its own conversation history but shares
/// the provider orchestrator.
pub struct SubAgentPool {
    /// Maximum concurrent sub-agents.
    semaphore: Arc<Semaphore>,
    /// Timeout per task in seconds.
    timeout_secs: u64,
    /// Maximum retry attempts per failed task.
    max_retries: u32,
}

impl SubAgentPool {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            timeout_secs: 300,
            max_retries: 2,
        }
    }

    /// Execute multiple sub-agent tasks in parallel.
    pub async fn execute(
        &self,
        tasks: Vec<SubAgentTask>,
        agent_config: AgentConfig,
        orchestrator: ProviderOrchestrator,
    ) -> Vec<SubAgentResult> {
        let orchestrator = Arc::new(orchestrator);
        let mut handles: Vec<JoinHandle<SubAgentResult>> = Vec::new();

        for task in tasks {
            let sem = Arc::clone(&self.semaphore);
            let orch = Arc::clone(&orchestrator);
            let cfg = agent_config.clone();
            let timeout = self.timeout_secs;
            let max_retries = self.max_retries;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("Semaphore closed");
                Self::execute_single(task, cfg, orch, timeout, max_retries).await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(SubAgentResult {
                    task_id: "unknown".into(),
                    status: TaskStatus::Failed { error: format!("Join error: {}", e) },
                    output: None,
                    tools_used: vec![],
                    tokens_used: 0,
                    agent_name: String::new(),
                    success: false,
                    elapsed_ms: 0,
                }),
            }
        }

        results
    }

    async fn execute_single(
        mut task: SubAgentTask,
        agent_config: AgentConfig,
        orchestrator: Arc<ProviderOrchestrator>,
        timeout_secs: u64,
        max_retries: u32,
    ) -> SubAgentResult {
        task.status = TaskStatus::Running;

        while task.retry_count <= max_retries {
            let mut agent = match Agent::new(
                &DeepSeekConfig::default(),
                agent_config.clone(),
                (*orchestrator).clone(),
            ) {
                Ok(a) => a,
                Err(e) => {
                    return SubAgentResult {
                        task_id: task.id.clone(),
                        status: TaskStatus::Failed { error: e.to_string() },
                        output: None,
                        tools_used: vec![],
                        tokens_used: 0,
                        agent_name: String::new(),
                        success: false,
                        elapsed_ms: 0,
                    };
                }
            };

            let prompt = if let Some(ref ctx) = task.context {
                format!("Context: {}\n\nTask: {}", ctx, task.instruction)
            } else {
                task.instruction.clone()
            };

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                agent.process(&prompt),
            ).await;

            match result {
                Ok(Ok(agent_result)) => {
                    return SubAgentResult {
                        task_id: task.id.clone(),
                        status: TaskStatus::Completed,
                        output: Some(agent_result.content),
                        tools_used: agent_result.tools_used,
                        tokens_used: agent_result.total_tokens,
                        agent_name: String::new(),
                        success: true,
                        elapsed_ms: 0,
                    };
                }
                Ok(Err(e)) => {
                    task.retry_count += 1;
                    if task.retry_count > max_retries {
                        return SubAgentResult {
                            task_id: task.id,
                            status: TaskStatus::Failed { error: e.to_string() },
                            output: None,
                            tools_used: vec![],
                            tokens_used: 0,
                            agent_name: String::new(),
                            success: false,
                            elapsed_ms: 0,
                        };
                    }
                    tracing::warn!(task=%task.id, attempt=task.retry_count, error=%e, "SubAgent retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(_) => {
                    task.status = TaskStatus::TimedOut;
                    task.retry_count += 1;
                    if task.retry_count > max_retries {
                        return SubAgentResult {
                            task_id: task.id,
                            status: TaskStatus::TimedOut,
                            output: None,
                            tools_used: vec![],
                            tokens_used: 0,
                            agent_name: String::new(),
                            success: false,
                            elapsed_ms: 0,
                        };
                    }
                }
            }
        }

        SubAgentResult {
            task_id: task.id,
            status: TaskStatus::Failed { error: "Max retries exceeded".into() },
            output: None,
            tools_used: vec![],
            tokens_used: 0,
            agent_name: String::new(),
            success: false,
            elapsed_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_agent_pool_creation() {
        let _pool = SubAgentPool::new(4);
    }

    #[test]
    fn test_task_status_transitions() {
        let task = SubAgentTask {
            id: "test-1".into(),
            instruction: "Hello".into(),
            context: None,
            status: TaskStatus::Queued,
            retry_count: 0,
        };
        assert_eq!(task.status, TaskStatus::Queued);
    }
}
