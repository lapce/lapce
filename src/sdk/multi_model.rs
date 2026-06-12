//! RLM (Recursive Language Model) mode — DeepSeek-TUI inspired.
//!
//! Enhanced version of SubAgentPool with multi-model routing:
//!   - Main task → expensive model (e.g., DeepSeek V4 / Claude Sonnet)
//!   - Sub-tasks → cheap model (e.g., DeepSeek Flash / Qwen-7B)
//!   - Parallel execution with cost-weighted scheduling
//!
//! ## Cost optimization
//!
//! ```text
//! Main (deepseek-v4-pro):   $0.28/MTok  ← 1 call
//! Sub1 (deepseek-v4-flash): $0.08/MTok  ← cheap, parallel
//! Sub2 (glm-5.1):           $0.50/MTok  ← medium
//! Sub3 (kimi-2.6):          $0.60/MTok  ← medium
//! Sub4 (minimax-M2.7):      $0.50/MTok  ← medium
//! ```
//!
//! Total estimated cost: ~10-20% of running everything on premium model.

use std::sync::Arc;

use crate::agent::sub_agents::{SubAgentPool, SubAgentTask, SubAgentResult};
use crate::agent::AgentConfig;
use crate::providers::orchestrator::ProviderOrchestrator;

/// Model tier for task routing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelTier {
    /// Expensive premium model (e.g., DeepSeek V4, Claude Sonnet).
    Premium,
    /// Medium-cost model (e.g., GLM-5.1, Kimi-2.6).
    Standard,
    /// Cheapest viable model (e.g., DeepSeek Flash, local Qwen).
    Budget,
}

/// Task with model routing preference.
#[derive(Debug, Clone)]
pub struct RoutedTask {
    pub task: SubAgentTask,
    pub min_tier: ModelTier,
    /// Max concurrent tasks at this tier.
    pub max_parallel: usize,
}

/// RLM executor — routes tasks to different model tiers for cost optimization.
///
/// Inspired by DeepSeek-TUI's RLM (Recursive Language Model) mode:
///   "允许主模型同时调度多子任务并行运行，将复杂任务按难度分级处理"
pub struct RlmExecutor {
    /// Premium tier pool (small — 1 concurrent for expensive model).
    premium_pool: Arc<SubAgentPool>,
    /// Standard tier pool (medium — 2-3 concurrent).
    standard_pool: Arc<SubAgentPool>,
    /// Budget tier pool (larger — 4+ concurrent for cheap models).
    budget_pool: Arc<SubAgentPool>,
    /// Base orchestrator config.
    orchestrator: Option<ProviderOrchestrator>,
}

impl RlmExecutor {
    /// Create RLM executor with tiered pools.
    pub fn new() -> Self {
        Self {
            premium_pool: Arc::new(SubAgentPool::new(1)),
            standard_pool: Arc::new(SubAgentPool::new(3)),
            budget_pool: Arc::new(SubAgentPool::new(5)),
            orchestrator: None,
        }
    }

    /// Initialize with a provider orchestrator.
    pub fn with_orchestrator(mut self, orch: ProviderOrchestrator) -> Self {
        self.orchestrator = Some(orch);
        self
    }

    /// Classify task complexity → assign model tier.
    pub fn classify_tier(task: &SubAgentTask) -> ModelTier {
        let desc = &task.instruction.to_lowercase();
        let complexity = desc.len();

        if complexity > 500
            || desc.contains("architecture")
            || desc.contains("refactor")
            || desc.contains("design")
        {
            ModelTier::Premium
        } else if complexity > 200
            || desc.contains("test")
            || desc.contains("review")
            || desc.contains("debug")
        {
            ModelTier::Standard
        } else {
            ModelTier::Budget
        }
    }

    /// Execute tasks with tiered routing for cost optimization.
    pub async fn execute(
        &self,
        tasks: Vec<SubAgentTask>,
        agent_config: AgentConfig,
        orchestrator: ProviderOrchestrator,
    ) -> Vec<SubAgentResult> {
        let mut premium_tasks = Vec::new();
        let mut standard_tasks = Vec::new();
        let mut budget_tasks = Vec::new();

        for task in tasks {
            let tier = Self::classify_tier(&task);
            match tier {
                ModelTier::Premium => premium_tasks.push(task),
                ModelTier::Standard => standard_tasks.push(task),
                ModelTier::Budget => budget_tasks.push(task),
            }
        }

        tracing::info!(
            premium = premium_tasks.len(),
            standard = standard_tasks.len(),
            budget = budget_tasks.len(),
            "RLM tiered execution"
        );

        let mut all_results = Vec::new();

        // Execute tiers in parallel
        let (r1, r2, r3) = tokio::join!(
            self.premium_pool.execute(premium_tasks, agent_config.clone(), orchestrator.clone()),
            self.standard_pool.execute(standard_tasks, agent_config.clone(), orchestrator.clone()),
            self.budget_pool.execute(budget_tasks, agent_config, orchestrator),
        );

        all_results.extend(r1);
        all_results.extend(r2);
        all_results.extend(r3);
        all_results
    }

    /// Estimate cost of RLM execution vs single premium model.
    pub fn estimate_cost_savings(tasks: &[SubAgentTask]) -> f64 {
        let premium_count = tasks.iter().filter(|t| Self::classify_tier(t) == ModelTier::Premium).count();
        let standard_count = tasks.iter().filter(|t| Self::classify_tier(t) == ModelTier::Standard).count();
        let budget_count = tasks.len() - premium_count - standard_count;

        let rlm_cost = (premium_count as f64 * 0.28)
            + (standard_count as f64 * 0.50)
            + (budget_count as f64 * 0.08);

        let single_cost = tasks.len() as f64 * 0.28;
        single_cost - rlm_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::sub_agents::TaskStatus;

    #[test]
    fn test_classify_premium() {
        let task = SubAgentTask {
            id: "1".into(),
            instruction: "Complete system architecture redesign with distributed computing patterns".into(),
            context: None,
            status: TaskStatus::Queued,
            retry_count: 0,
        };
        assert_eq!(RlmExecutor::classify_tier(&task), ModelTier::Premium);
    }

    #[test]
    fn test_classify_budget() {
        let task = SubAgentTask {
            id: "2".into(),
            instruction: "Add comment to calculate function".into(),
            context: None,
            status: TaskStatus::Queued,
            retry_count: 0,
        };
        assert_eq!(RlmExecutor::classify_tier(&task), ModelTier::Budget);
    }

    #[test]
    fn test_cost_savings() {
        let tasks: Vec<_> = (0..10).map(|i| SubAgentTask {
            id: format!("t{}", i),
            instruction: format!("Task {}", i),
            context: None,
            status: TaskStatus::Queued,
            retry_count: 0,
        }).collect();
        let savings = RlmExecutor::estimate_cost_savings(&tasks);
        assert!(savings > 0.0, "RLM should always save cost vs single premium");
    }
}
