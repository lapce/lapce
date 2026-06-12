//! Autonomous Planning Agent - Multi-step task decomposition and execution.
//!
//! This module provides:
//! - Task decomposition into subtasks
//! - Execution planning and scheduling
//! - Dependency management
//! - Progress tracking

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A task to be executed.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub dependencies: Vec<String>,
    pub tools: Vec<String>,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub estimated_duration_secs: u64,
    pub actual_duration_secs: Option<u64>,
}

/// Type of task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    CodeGeneration,
    CodeReview,
    Refactoring,
    TestGeneration,
    DebugAnalysis,
    Documentation,
    Build,
    Test,
    Deploy,
    Research,
    Unknown,
}

/// Task execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Planned,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// An execution plan containing decomposed tasks.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub id: String,
    pub name: String,
    pub tasks: Vec<Task>,
    pub execution_order: Vec<String>,
    pub estimated_duration_secs: u64,
}

/// A step in task execution.
#[derive(Debug, Clone)]
pub struct ExecutionStep {
    pub task_id: String,
    pub status: TaskStatus,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub result: Option<serde_json::Value>,
}

/// Autonomous planning agent.
pub struct PlanningAgent {
    plans: Arc<RwLock<HashMap<String, ExecutionPlan>>>,
    execution_history: Arc<RwLock<Vec<ExecutionStep>>>,
}

impl PlanningAgent {
    pub fn new() -> Self {
        Self {
            plans: Arc::new(RwLock::new(HashMap::new())),
            execution_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Decompose a complex task into subtasks.
    pub async fn decompose(&self, task: &str) -> ExecutionPlan {
        let task_type = self.classify_task(task);
        let subtasks = self.generate_subtasks(task, &task_type);

        // Build dependency graph and execution order
        let execution_order = self.topological_sort(&subtasks);

        // Calculate estimated duration
        let estimated_duration: u64 = subtasks.iter()
            .map(|t| t.estimated_duration_secs)
            .sum();

        let plan = ExecutionPlan {
            id: format!("plan_{}", current_timestamp()),
            name: task.chars().take(50).collect(),
            tasks: subtasks,
            execution_order,
            estimated_duration_secs: estimated_duration,
        };

        self.plans.write().await.insert(plan.id.clone(), plan.clone());
        plan
    }

    /// Classify the type of task.
    fn classify_task(&self, task: &str) -> TaskType {
        let task_lower = task.to_lowercase();

        if task_lower.contains("generate") || task_lower.contains("write") || task_lower.contains("create") {
            if task_lower.contains("test") {
                TaskType::TestGeneration
            } else if task_lower.contains("doc") || task_lower.contains("comment") {
                TaskType::Documentation
            } else {
                TaskType::CodeGeneration
            }
        } else if task_lower.contains("refactor") || task_lower.contains("extract") || task_lower.contains("rename") {
            TaskType::Refactoring
        } else if task_lower.contains("review") || task_lower.contains("analyze") || task_lower.contains("check") {
            TaskType::CodeReview
        } else if task_lower.contains("debug") || task_lower.contains("fix") || task_lower.contains("error") {
            TaskType::DebugAnalysis
        } else if task_lower.contains("build") || task_lower.contains("compile") {
            TaskType::Build
        } else if task_lower.contains("test") || task_lower.contains("spec") {
            TaskType::Test
        } else if task_lower.contains("deploy") || task_lower.contains("release") {
            TaskType::Deploy
        } else {
            TaskType::Unknown
        }
    }

    /// Generate subtasks based on task type.
    fn generate_subtasks(&self, task: &str, task_type: &TaskType) -> Vec<Task> {
        match task_type {
            TaskType::CodeGeneration => self.generate_code_subtasks(task),
            TaskType::TestGeneration => self.generate_test_subtasks(task),
            TaskType::Refactoring => self.generate_refactor_subtasks(task),
            TaskType::CodeReview => self.generate_review_subtasks(task),
            TaskType::DebugAnalysis => self.generate_debug_subtasks(task),
            _ => vec![Task {
                id: "task_1".to_string(),
                name: task.chars().take(30).collect(),
                description: task.to_string(),
                task_type: *task_type,
                status: TaskStatus::Pending,
                dependencies: vec![],
                tools: vec!["code_generator".to_string()],
                input: serde_json::json!({"task": task}),
                output: None,
                error: None,
                estimated_duration_secs: 60,
                actual_duration_secs: None,
            }],
        }
    }

    fn generate_code_subtasks(&self, task: &str) -> Vec<Task> {
        vec![
            Task {
                id: "code_1".to_string(),
                name: "Analyze requirements".to_string(),
                description: format!("Analyze: {}", task),
                task_type: TaskType::CodeGeneration,
                status: TaskStatus::Pending,
                dependencies: vec![],
                tools: vec!["analyzer".to_string()],
                input: serde_json::json!({"task": task}),
                output: None,
                error: None,
                estimated_duration_secs: 30,
                actual_duration_secs: None,
            },
            Task {
                id: "code_2".to_string(),
                name: "Generate code".to_string(),
                description: "Generate code implementation".to_string(),
                task_type: TaskType::CodeGeneration,
                status: TaskStatus::Pending,
                dependencies: vec!["code_1".to_string()],
                tools: vec!["code_generator".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 60,
                actual_duration_secs: None,
            },
            Task {
                id: "code_3".to_string(),
                name: "Verify syntax".to_string(),
                description: "Verify generated code syntax".to_string(),
                task_type: TaskType::CodeGeneration,
                status: TaskStatus::Pending,
                dependencies: vec!["code_2".to_string()],
                tools: vec!["syntax_checker".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 15,
                actual_duration_secs: None,
            },
        ]
    }

    fn generate_test_subtasks(&self, task: &str) -> Vec<Task> {
        vec![
            Task {
                id: "test_1".to_string(),
                name: "Analyze code for testability".to_string(),
                description: format!("Analyze code to generate tests for: {}", task),
                task_type: TaskType::TestGeneration,
                status: TaskStatus::Pending,
                dependencies: vec![],
                tools: vec!["analyzer".to_string()],
                input: serde_json::json!({"task": task}),
                output: None,
                error: None,
                estimated_duration_secs: 30,
                actual_duration_secs: None,
            },
            Task {
                id: "test_2".to_string(),
                name: "Generate unit tests".to_string(),
                description: "Generate unit test cases".to_string(),
                task_type: TaskType::TestGeneration,
                status: TaskStatus::Pending,
                dependencies: vec!["test_1".to_string()],
                tools: vec!["test_generator".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 45,
                actual_duration_secs: None,
            },
            Task {
                id: "test_3".to_string(),
                name: "Run tests".to_string(),
                description: "Execute generated tests".to_string(),
                task_type: TaskType::TestGeneration,
                status: TaskStatus::Pending,
                dependencies: vec!["test_2".to_string()],
                tools: vec!["test_runner".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 30,
                actual_duration_secs: None,
            },
        ]
    }

    fn generate_refactor_subtasks(&self, task: &str) -> Vec<Task> {
        vec![
            Task {
                id: "refactor_1".to_string(),
                name: "Analyze code structure".to_string(),
                description: format!("Analyze for refactoring: {}", task),
                task_type: TaskType::Refactoring,
                status: TaskStatus::Pending,
                dependencies: vec![],
                tools: vec!["code_analyzer".to_string()],
                input: serde_json::json!({"task": task}),
                output: None,
                error: None,
                estimated_duration_secs: 30,
                actual_duration_secs: None,
            },
            Task {
                id: "refactor_2".to_string(),
                name: "Generate refactoring plan".to_string(),
                description: "Create refactoring plan with impact analysis".to_string(),
                task_type: TaskType::Refactoring,
                status: TaskStatus::Pending,
                dependencies: vec!["refactor_1".to_string()],
                tools: vec!["refactor_planner".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 20,
                actual_duration_secs: None,
            },
            Task {
                id: "refactor_3".to_string(),
                name: "Apply refactoring".to_string(),
                description: "Execute refactoring changes".to_string(),
                task_type: TaskType::Refactoring,
                status: TaskStatus::Pending,
                dependencies: vec!["refactor_2".to_string()],
                tools: vec!["code_editor".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 60,
                actual_duration_secs: None,
            },
            Task {
                id: "refactor_4".to_string(),
                name: "Verify refactoring".to_string(),
                description: "Verify refactoring didn't break code".to_string(),
                task_type: TaskType::Refactoring,
                status: TaskStatus::Pending,
                dependencies: vec!["refactor_3".to_string()],
                tools: vec!["test_runner".to_string(), "linter".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 45,
                actual_duration_secs: None,
            },
        ]
    }

    fn generate_review_subtasks(&self, task: &str) -> Vec<Task> {
        vec![
            Task {
                id: "review_1".to_string(),
                name: "Analyze code".to_string(),
                description: format!("Analyze code for review: {}", task),
                task_type: TaskType::CodeReview,
                status: TaskStatus::Pending,
                dependencies: vec![],
                tools: vec!["code_analyzer".to_string()],
                input: serde_json::json!({"task": task}),
                output: None,
                error: None,
                estimated_duration_secs: 40,
                actual_duration_secs: None,
            },
            Task {
                id: "review_2".to_string(),
                name: "Check code quality".to_string(),
                description: "Run linters and style checks".to_string(),
                task_type: TaskType::CodeReview,
                status: TaskStatus::Pending,
                dependencies: vec!["review_1".to_string()],
                tools: vec!["linter".to_string(), "style_checker".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 20,
                actual_duration_secs: None,
            },
            Task {
                id: "review_3".to_string(),
                name: "Generate review report".to_string(),
                description: "Generate comprehensive review report".to_string(),
                task_type: TaskType::CodeReview,
                status: TaskStatus::Pending,
                dependencies: vec!["review_2".to_string()],
                tools: vec!["report_generator".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 15,
                actual_duration_secs: None,
            },
        ]
    }

    fn generate_debug_subtasks(&self, task: &str) -> Vec<Task> {
        vec![
            Task {
                id: "debug_1".to_string(),
                name: "Analyze error".to_string(),
                description: format!("Analyze error: {}", task),
                task_type: TaskType::DebugAnalysis,
                status: TaskStatus::Pending,
                dependencies: vec![],
                tools: vec!["error_analyzer".to_string()],
                input: serde_json::json!({"task": task}),
                output: None,
                error: None,
                estimated_duration_secs: 30,
                actual_duration_secs: None,
            },
            Task {
                id: "debug_2".to_string(),
                name: "Identify root cause".to_string(),
                description: "Perform root cause analysis".to_string(),
                task_type: TaskType::DebugAnalysis,
                status: TaskStatus::Pending,
                dependencies: vec!["debug_1".to_string()],
                tools: vec!["debug_engine".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 45,
                actual_duration_secs: None,
            },
            Task {
                id: "debug_3".to_string(),
                name: "Generate fix".to_string(),
                description: "Generate fix suggestion".to_string(),
                task_type: TaskType::DebugAnalysis,
                status: TaskStatus::Pending,
                dependencies: vec!["debug_2".to_string()],
                tools: vec!["fix_generator".to_string()],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 30,
                actual_duration_secs: None,
            },
        ]
    }

    /// Topological sort for execution order.
    fn topological_sort(&self, tasks: &[Task]) -> Vec<String> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj_list: HashMap<&str, Vec<&str>> = HashMap::new();

        // Initialize
        for task in tasks {
            in_degree.insert(task.id.as_str(), task.dependencies.len());
            adj_list.entry(task.id.as_str()).or_default();
            for dep in &task.dependencies {
                adj_list.entry(dep.as_str()).or_default().push(task.id.as_str());
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<&str> = VecDeque::new();
        for (id, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(id);
            }
        }

        let mut result = Vec::new();
        while let Some(node) = queue.pop_front() {
            result.push(node.to_string());

            if let Some(neighbors) = adj_list.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        result
    }

    /// Execute a plan step by step.
    pub async fn execute(&self, plan_id: &str) -> Result<Vec<ExecutionStep>, String> {
        let plan = self.plans.read().await.get(plan_id).cloned()
            .ok_or_else(|| format!("Plan {} not found", plan_id))?;

        let mut steps = Vec::new();

        for task_id in &plan.execution_order {
            let step = ExecutionStep {
                task_id: task_id.clone(),
                status: TaskStatus::Running,
                started_at: Some(current_timestamp()),
                completed_at: None,
                result: None,
            };

            steps.push(step);

            // Record in history
            self.execution_history.write().await.push(ExecutionStep {
                task_id: task_id.clone(),
                status: TaskStatus::Running,
                started_at: Some(current_timestamp()),
                completed_at: None,
                result: None,
            });
        }

        Ok(steps)
    }

    /// Get plan status.
    pub async fn get_plan(&self, plan_id: &str) -> Option<ExecutionPlan> {
        self.plans.read().await.get(plan_id).cloned()
    }

    /// Get execution history.
    pub async fn get_history(&self) -> Vec<ExecutionStep> {
        self.execution_history.read().await.clone()
    }
}

impl Default for PlanningAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: planning_agent.rs:512")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_decompose_code_task() {
        let agent = PlanningAgent::new();
        let plan = agent.decompose("Generate a function to calculate fibonacci").await;

        assert!(!plan.tasks.is_empty());
        assert!(!plan.execution_order.is_empty());
    }

    #[tokio::test]
    async fn test_topological_sort() {
        let agent = PlanningAgent::new();
        let tasks = vec![
            Task {
                id: "a".to_string(),
                name: "Task A".to_string(),
                description: "".to_string(),
                task_type: TaskType::Unknown,
                status: TaskStatus::Pending,
                dependencies: vec![],
                tools: vec![],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 10,
                actual_duration_secs: None,
            },
            Task {
                id: "b".to_string(),
                name: "Task B".to_string(),
                description: "".to_string(),
                task_type: TaskType::Unknown,
                status: TaskStatus::Pending,
                dependencies: vec!["a".to_string()],
                tools: vec![],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 10,
                actual_duration_secs: None,
            },
            Task {
                id: "c".to_string(),
                name: "Task C".to_string(),
                description: "".to_string(),
                task_type: TaskType::Unknown,
                status: TaskStatus::Pending,
                dependencies: vec!["a".to_string()],
                tools: vec![],
                input: serde_json::json!({}),
                output: None,
                error: None,
                estimated_duration_secs: 10,
                actual_duration_secs: None,
            },
        ];

        let order = agent.topological_sort(&tasks);
        assert_eq!(order[0], "a");
        assert!(order.contains(&"b".to_string()));
        assert!(order.contains(&"c".to_string()));
    }
}
