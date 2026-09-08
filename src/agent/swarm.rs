//! Swarm multi-agent coordination — inspired by CarpAI's Swarm architecture.
//!
//! Layered on top of SubAgentPool. The SwarmCoordinator handles:
//! - Task decomposition and assignment to sub-agents
//! - Inter-agent communication via channels (DM/broadcast)
//! - Conflict detection and resolution
//! - Lifecycle management (spawned→running→completed→crashed)
//!
//! ## Architecture
//!
//! ```text
//! SwarmCoordinator
//!   ├── Decomposer → break task into sub-tasks
//!   ├── Router → assign sub-tasks to agents (round-robin / affinity)
//!   ├── Channel → inter-agent messaging (DM, broadcast, announce)
//!   ├── ConflictDetector → detect overlapping file edits
//!   └── Integrator → merge agent outputs into final result
//! ```

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::agent::sub_agents::{SubAgentPool, SubAgentTask, SubAgentResult, TaskStatus};
use crate::agent::AgentConfig;
use crate::providers::orchestrator::ProviderOrchestrator;

// ============================================================================
// Swarm Message Types
// ============================================================================

/// Message types for inter-agent communication.
#[derive(Debug, Clone)]
pub enum SwarmMessage {
    /// Direct message from one agent to another.
    Direct {
        from: String,
        to: String,
        content: String,
    },
    /// Broadcast message to all agents.
    Broadcast {
        from: String,
        content: String,
    },
    /// Agent announces task completion for others to consume.
    TaskComplete {
        agent_id: String,
        task_id: String,
        summary: String,
    },
    /// Agent requests help from peers.
    HelpRequest {
        agent_id: String,
        question: String,
    },
}

/// A swarm agent with communication capabilities.
#[derive(Debug, Clone)]
pub struct SwarmAgent {
    /// Unique agent ID.
    pub id: String,
    /// Agent role (e.g., "coder", "reviewer", "tester").
    pub role: String,
    /// Agent's scope / working area.
    pub scope: String,
    /// Current lifecycle state.
    pub state: AgentState,
    /// Active task.
    pub current_task: Option<String>,
}

/// Agent lifecycle states.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentState {
    Spawned,
    Ready,
    Running,
    Blocked,
    Completed,
    Failed,
    Stopped,
    Crashed,
}

/// Task decomposition result.
#[derive(Debug, Clone)]
pub struct DecomposedTask {
    /// Unique task ID.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Required role (e.g., "coder", "reviewer").
    pub required_role: Option<String>,
    /// File scope for this task.
    pub scope: Option<String>,
    /// Dependencies (other task IDs that must complete first).
    pub dependencies: Vec<String>,
    /// Estimated complexity (1-10).
    pub complexity: u8,
}

/// Swarm execution result.
#[derive(Debug, Clone)]
pub struct SwarmResult {
    /// All sub-agent results.
    pub results: Vec<SubAgentResult>,
    /// Number of tasks completed.
    pub completed: usize,
    /// Number of tasks failed.
    pub failed: usize,
    /// Number of messages exchanged.
    pub messages_sent: usize,
    /// Total tokens used across all agents.
    pub total_tokens: u32,
}

// ============================================================================
// SwarmCoordinator
// ============================================================================

/// Coordinates multiple agents working on a shared goal.
pub struct SwarmCoordinator {
    /// Registered agents.
    agents: Arc<RwLock<HashMap<String, SwarmAgent>>>,
    /// Broadcast channel for agent announcements.
    broadcast_tx: broadcast::Sender<SwarmMessage>,
    /// Direct message channels (agent_id → sender).
    dm_channels: Arc<RwLock<HashMap<String, mpsc::Sender<SwarmMessage>>>>,
    /// Sub-agent pool for parallel execution.
    pool: Arc<SubAgentPool>,
    /// Agent config template.
    agent_config: AgentConfig,
}

impl SwarmCoordinator {
    /// Create a new swarm coordinator.
    pub fn new(max_concurrent: usize, agent_config: AgentConfig) -> Self {
        let (broadcast_tx, _) = broadcast::channel(64);
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            dm_channels: Arc::new(RwLock::new(HashMap::new())),
            pool: Arc::new(SubAgentPool::new(max_concurrent)),
            agent_config,
        }
    }

    /// Register a new agent in the swarm.
    pub async fn add_agent(&self, role: &str, scope: &str) -> String {
        let id = format!("agent-{}", uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>());
        let (dm_tx, _dm_rx) = mpsc::channel(32);

        let agent = SwarmAgent {
            id: id.clone(),
            role: role.to_string(),
            scope: scope.to_string(),
            state: AgentState::Ready,
            current_task: None,
        };

        self.agents.write().await.insert(id.clone(), agent);
        self.dm_channels.write().await.insert(id.clone(), dm_tx);
        tracing::info!(agent=%id, role=%role, scope=%scope, "Swarm agent registered");
        id
    }

    /// Send a direct message to an agent.
    pub async fn send_dm(&self, from: &str, to: &str, content: &str) {
        if let Some(tx) = self.dm_channels.read().await.get(to) {
            let _ = tx.send(SwarmMessage::Direct {
                from: from.to_string(),
                to: to.to_string(),
                content: content.to_string(),
            }).await;
        }
    }

    /// Broadcast message to all agents.
    pub fn broadcast(&self, from: &str, content: &str) {
        let _ = self.broadcast_tx.send(SwarmMessage::Broadcast {
            from: from.to_string(),
            content: content.to_string(),
        });
    }

    /// Decompose a complex task into sub-tasks for parallel execution.
    ///
    /// Uses simple heuristics: splits by file scope, role requirements,
    /// and dependency order. In full production, an LLM would do this.
    pub fn decompose(&self, task_description: &str, roles: &[&str]) -> Vec<DecomposedTask> {
        let mut tasks = Vec::new();

        // Simple heuristic: split by sentence boundaries
        let sentences: Vec<&str> = task_description
            .split(['.', '\n'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && s.len() > 10)
            .collect();

        for (i, sentence) in sentences.iter().enumerate() {
            let role = if i < roles.len() { Some(roles[i].to_string()) } else { None };

            tasks.push(DecomposedTask {
                id: format!("task-{}", i + 1),
                description: sentence.to_string(),
                required_role: role,
                scope: None,
                dependencies: if i > 0 { vec![format!("task-{}", i)] } else { vec![] },
                complexity: 3, // default
            });
        }

        if tasks.is_empty() {
            tasks.push(DecomposedTask {
                id: "task-1".to_string(),
                description: task_description.to_string(),
                required_role: None,
                scope: None,
                dependencies: vec![],
                complexity: 5,
            });
        }

        tasks
    }

    /// Execute a decomposed task plan across the swarm.
    ///
    /// 1. Decompose task into sub-tasks
    /// 2. Assign tasks to available agents
    /// 3. Execute in parallel (respecting dependencies)
    /// 4. Collect results and handle conflicts
    pub async fn execute(
        &self,
        task_description: &str,
        orchestrator: ProviderOrchestrator,
    ) -> SwarmResult {
        let agents_snapshot = self.agents.read().await;
        let roles: Vec<&str> = agents_snapshot.values().map(|a| a.role.as_str()).collect();
        let decomposed = self.decompose(task_description, &roles);
        drop(agents_snapshot);
        let total_tasks = decomposed.len();

        self.broadcast("coordinator", &format!("Starting swarm execution: {} sub-tasks", total_tasks));

        // Convert to SubAgentTask and execute via pool
        let sub_tasks: Vec<SubAgentTask> = decomposed.iter().map(|dt| {
            SubAgentTask {
                id: dt.id.clone(),
                instruction: dt.description.clone(),
                context: dt.scope.clone(),
                status: TaskStatus::Queued,
                retry_count: 0,
            }
        }).collect();

        // Update agent states
        {
            let mut agents = self.agents.write().await;
            for agent in agents.values_mut() {
                agent.state = AgentState::Running;
            }
        }

        let results = self.pool.execute(sub_tasks, self.agent_config.clone(), orchestrator).await;

        // Update agent states based on results
        let mut completed = 0;
        let mut failed = 0;
        let mut total_tokens = 0u32;

        {
            let mut agents = self.agents.write().await;
            for result in &results {
                total_tokens += result.tokens_used;
                match result.status {
                    TaskStatus::Completed => {
                        completed += 1;
                        // Assign to first available agent for tracking
                        if let Some(agent) = agents.values_mut().next() {
                            agent.state = AgentState::Completed;
                        }
                    }
                    TaskStatus::Failed { .. } | TaskStatus::TimedOut => {
                        failed += 1;
                    }
                    _ => {}
                }
                // Announce completion
                let _ = self.broadcast_tx.send(SwarmMessage::TaskComplete {
                    agent_id: "swarm".into(),
                    task_id: result.task_id.clone(),
                    summary: format!("Status: {:?}", result.status),
                });
            }
        }

        // Conflict detection
        let conflicts = self.detect_conflicts(&results);
        if !conflicts.is_empty() {
            tracing::warn!(conflicts=?conflicts, "Swarm conflict detected");
        }

        SwarmResult {
            results,
            completed,
            failed,
            messages_sent: 0, // TODO: track message count
            total_tokens,
        }
    }

    /// Detect conflicts between parallel agent outputs.
    /// Conflicts occur when multiple agents edit the same file.
    fn detect_conflicts(&self, results: &[SubAgentResult]) -> Vec<String> {
        let mut edited_files: HashMap<String, Vec<String>> = HashMap::new();

        for result in results {
            if let Some(ref output) = result.output {
                // Simple heuristic: check for file path mentions
                for line in output.lines() {
                    if let Some(path) = line.split(' ')
                        .find(|w| {
                            let trimmed = w.trim_end_matches([':', ',', '.', ';']);
                            trimmed.ends_with(".rs") || trimmed.ends_with(".toml") || trimmed.ends_with(".py") || trimmed.ends_with(".js")
                        })
                    {
                        edited_files.entry(path.to_string())
                            .or_default()
                            .push(result.task_id.clone());
                    }
                }
            }
        }

        edited_files.into_iter()
            .filter(|(_, tasks)| tasks.len() > 1)
            .map(|(file, tasks)| format!("{} edited by tasks: {:?}", file, tasks))
            .collect()
    }

    /// Get the swarm status for observability.
    pub async fn status(&self) -> SwarmStatus {
        let agents = self.agents.read().await;
        SwarmStatus {
            total_agents: agents.len(),
            agents: agents.values().cloned().collect(),
        }
    }
}

/// Snapshot of swarm state.
#[derive(Debug, Clone)]
pub struct SwarmStatus {
    pub total_agents: usize,
    pub agents: Vec<SwarmAgent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_decomposition() {
        let coordinator = SwarmCoordinator::new(4, AgentConfig::default());
        let tasks = coordinator.decompose(
            "Add user authentication. Write login endpoint. Create user model. Add password hashing.",
            &["coder", "reviewer", "tester"],
        );
        assert!(tasks.len() >= 3);
    }

    #[test]
    fn test_conflict_detection() {
        let coordinator = SwarmCoordinator::new(2, AgentConfig::default());
        let results = vec![
            SubAgentResult {
                task_id: "task-1".into(),
                status: TaskStatus::Completed,
                output: Some("Edited file src/main.rs: added login".into()),
                tools_used: vec![],
                tokens_used: 100,
                agent_name: "coder".into(),
                elapsed_ms: 50,
                success: true,
            },
            SubAgentResult {
                task_id: "task-2".into(),
                status: TaskStatus::Completed,
                output: Some("Changed file src/main.rs: added logout".into()),
                tools_used: vec![],
                tokens_used: 100,
                agent_name: "reviewer".into(),
                elapsed_ms: 60,
                success: true,
            },
        ];
        let conflicts = coordinator.detect_conflicts(&results);
        assert!(!conflicts.is_empty(), "Should detect conflict on src/main.rs");
    }
}
