//! Task Planning System - AI-powered task decomposition and planning
//!
//! Based on Claude Code's task planning approach, this module provides:
//! - Natural language to structured task conversion
//! - Task dependency graph
//! - Parallel task scheduling
//! - Execution state tracking

use std::collections::HashMap;
use std::time::Instant;

/// Task types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskType {
    Implementation,
    Refactoring,
    Testing,
    Documentation,
    Review,
    Debug,
    Build,
    Deploy,
    Research,
    Planning,
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
    Skipped,
}

/// Task priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Critical = 4,
    High = 3,
    Medium = 2,
    Low = 1,
}

/// A single task
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub dependencies: Vec<String>, // Task IDs that must complete before this
    pub subtasks: Vec<String>,     // Subtask IDs
    pub parent_id: Option<String>,
    pub estimated_minutes: u32,
    pub actual_minutes: Option<u32>,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    pub result: Option<TaskResult>,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, String>,
}

/// Task execution result
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub success: bool,
    pub output: String,
    pub files_created: Vec<String>,
    pub files_modified: Vec<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Task plan - a collection of tasks with dependencies
#[derive(Debug, Clone)]
pub struct TaskPlan {
    pub id: String,
    pub title: String,
    pub description: String,
    pub tasks: HashMap<String, Task>,
    pub root_tasks: Vec<String>, // Tasks with no dependencies
    pub status: PlanStatus,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    pub total_estimated_minutes: u32,
    pub progress: f32, // 0.0 to 1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStatus {
    Created,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl TaskPlan {
    pub fn new(id: &str, title: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            tasks: HashMap::new(),
            root_tasks: Vec::new(),
            status: PlanStatus::Created,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
            total_estimated_minutes: 0,
            progress: 0.0,
        }
    }

    /// Add a task to the plan
    pub fn add_task(&mut self, task: Task) -> &Task {
        let id = task.id.clone();
        
        // Update estimated time
        self.total_estimated_minutes += task.estimated_minutes;
        
        // Add to tasks map
        self.tasks.insert(id.clone(), task);
        
        // If no dependencies, add to root tasks
        if let Some(t) = self.tasks.get(&id) {
            if t.dependencies.is_empty()
                && !self.root_tasks.contains(&id) {
                    self.root_tasks.push(id.clone());
                }
        }
        
        self.tasks.get(&id).expect("Task should exist")
    }

    /// Get tasks ready to execute (all dependencies completed)
    pub fn get_ready_tasks(&self) -> Vec<&Task> {
        self.tasks.values()
            .filter(|t| {
                if t.status != TaskStatus::Pending {
                    return false;
                }
                // Check all dependencies are completed
                t.dependencies.iter().all(|dep_id| {
                    self.tasks.get(dep_id)
                        .map(|dep| dep.status == TaskStatus::Completed)
                        .unwrap_or(true) // If dep not found, assume ok
                })
            })
            .collect()
    }

    /// Update task status
    pub fn update_task_status(&mut self, task_id: &str, status: TaskStatus) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = status;
            
            match status {
                TaskStatus::InProgress => {
                    task.started_at = Some(Instant::now());
                    if self.status == PlanStatus::Created {
                        self.status = PlanStatus::InProgress;
                        self.started_at = Some(Instant::now());
                    }
                }
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Skipped => {
                    task.completed_at = Some(Instant::now());
                }
                _ => {}
            }
            
            self.update_progress();
            self.check_completion();
            true
        } else {
            false
        }
    }

    /// Update plan progress
    fn update_progress(&mut self) {
        let total = self.tasks.len();
        if total == 0 {
            self.progress = 0.0;
            return;
        }
        
        let completed = self.tasks.values()
            .filter(|t| {
                matches!(t.status, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Skipped)
            })
            .count();
        
        self.progress = completed as f32 / total as f32;
    }

    /// Check if plan is complete
    fn check_completion(&mut self) {
        let all_done = self.tasks.values().all(|t| {
            matches!(t.status, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Skipped)
        });
        
        if all_done && !self.tasks.is_empty() {
            self.status = PlanStatus::Completed;
            self.completed_at = Some(Instant::now());
        }
    }

    /// Get task execution order using topological sort
    pub fn get_execution_order(&self) -> Vec<&Task> {
        let mut result = Vec::new();
        let mut visited = HashMap::new();
        
        for task_id in &self.root_tasks {
            self.topological_sort(task_id, &mut visited, &mut result);
        }
        
        result.iter().filter_map(|id| self.tasks.get(id)).collect()
    }
    
    fn topological_sort(&self, task_id: &str, visited: &mut HashMap<String, bool>, result: &mut Vec<String>) {
        if visited.contains_key(task_id) {
            return;
        }
        
        visited.insert(task_id.to_string(), true);
        
        // First add dependencies
        if let Some(task) = self.tasks.get(task_id) {
            for dep_id in &task.dependencies {
                self.topological_sort(dep_id, visited, result);
            }
        }
        
        result.push(task_id.to_string());
    }

    /// Format plan summary
    pub fn summary(&self) -> String {
        let mut s = format!(
            "📋 Task Plan: {}\n\
             Description: {}\n\
             Status: {:?}\n\
             Progress: {:.1}%\n\n",
            self.title,
            self.description,
            self.status,
            self.progress * 100.0
        );

        for (id, task) in &self.tasks {
            let status_icon = match task.status {
                TaskStatus::Pending => "⏳",
                TaskStatus::InProgress => "🔄",
                TaskStatus::Completed => "✅",
                TaskStatus::Failed => "❌",
                TaskStatus::Blocked => "🚫",
                TaskStatus::Skipped => "⏭️",
            };
            
            let priority_icon = match task.priority {
                TaskPriority::Critical => "🔴",
                TaskPriority::High => "🟠",
                TaskPriority::Medium => "🟡",
                TaskPriority::Low => "⚪",
            };
            
            s.push_str(&format!(
                "{} {} [{}] {}\n   Dependencies: {}\n",
                status_icon,
                priority_icon,
                id,
                task.title,
                if task.dependencies.is_empty() {
                    "None".to_string()
                } else {
                    task.dependencies.join(", ")
                }
            ));
        }

        s
    }
}

/// Task planner - AI-powered task decomposition
pub struct TaskPlanner {
    templates: HashMap<TaskType, TaskTemplate>,
}

#[derive(Debug, Clone)]
pub struct TaskTemplate {
    pub task_type: TaskType,
    pub default_priority: TaskPriority,
    pub estimated_minutes: u32,
    pub common_steps: Vec<String>,
}

impl TaskPlanner {
    pub fn new() -> Self {
        let mut templates = HashMap::new();
        
        templates.insert(TaskType::Implementation, TaskTemplate {
            task_type: TaskType::Implementation,
            default_priority: TaskPriority::High,
            estimated_minutes: 60,
            common_steps: vec![
                "Understand requirements".to_string(),
                "Design solution".to_string(),
                "Implement code".to_string(),
                "Add tests".to_string(),
                "Review changes".to_string(),
            ],
        });
        
        templates.insert(TaskType::Testing, TaskTemplate {
            task_type: TaskType::Testing,
            default_priority: TaskPriority::High,
            estimated_minutes: 30,
            common_steps: vec![
                "Identify test cases".to_string(),
                "Write unit tests".to_string(),
                "Run tests".to_string(),
                "Fix failures".to_string(),
            ],
        });
        
        templates.insert(TaskType::Refactoring, TaskTemplate {
            task_type: TaskType::Refactoring,
            default_priority: TaskPriority::Medium,
            estimated_minutes: 45,
            common_steps: vec![
                "Identify code smells".to_string(),
                "Plan refactoring".to_string(),
                "Apply changes".to_string(),
                "Run tests".to_string(),
            ],
        });

        Self { templates }
    }

    /// Create a task plan from natural language
    pub fn create_plan(&self, goal: &str) -> TaskPlan {
        let mut plan = TaskPlan::new(
            &uuid_v4(),
            goal,
            &format!("Plan to achieve: {}", goal),
        );
        
        // Simple task decomposition based on keywords
        let goal_lower = goal.to_lowercase();
        
        // Add implementation task if relevant
        if goal_lower.contains("implement") || goal_lower.contains("add") || goal_lower.contains("create") {
            plan.add_task(Task {
                id: "impl-1".to_string(),
                title: "Implementation".to_string(),
                description: "Implement the required functionality".to_string(),
                task_type: TaskType::Implementation,
                status: TaskStatus::Pending,
                priority: TaskPriority::High,
                dependencies: vec![],
                subtasks: vec![],
                parent_id: None,
                estimated_minutes: 60,
                actual_minutes: None,
                created_at: Instant::now(),
                started_at: None,
                completed_at: None,
                result: None,
                tags: vec!["implementation".to_string()],
                metadata: HashMap::new(),
            });
        }
        
        // Add testing task if relevant
        if goal_lower.contains("test") || goal_lower.contains("verify") {
            plan.add_task(Task {
                id: "test-1".to_string(),
                title: "Testing".to_string(),
                description: "Test the implementation".to_string(),
                task_type: TaskType::Testing,
                status: TaskStatus::Pending,
                priority: TaskPriority::High,
                dependencies: vec!["impl-1".to_string()],
                subtasks: vec![],
                parent_id: None,
                estimated_minutes: 30,
                actual_minutes: None,
                created_at: Instant::now(),
                started_at: None,
                completed_at: None,
                result: None,
                tags: vec!["testing".to_string()],
                metadata: HashMap::new(),
            });
        }
        
        // Add documentation task if relevant
        if goal_lower.contains("doc") || goal_lower.contains("document") || goal_lower.contains("readme") {
            plan.add_task(Task {
                id: "doc-1".to_string(),
                title: "Documentation".to_string(),
                description: "Update documentation".to_string(),
                task_type: TaskType::Documentation,
                status: TaskStatus::Pending,
                priority: TaskPriority::Low,
                dependencies: vec![],
                subtasks: vec![],
                parent_id: None,
                estimated_minutes: 15,
                actual_minutes: None,
                created_at: Instant::now(),
                started_at: None,
                completed_at: None,
                result: None,
                tags: vec!["documentation".to_string()],
                metadata: HashMap::new(),
            });
        }
        
        plan
    }

    /// Create a subtask
    pub fn create_subtask(&self, parent_id: &str, title: &str, description: &str) -> Task {
        Task {
            id: uuid_v4(),
            title: title.to_string(),
            description: description.to_string(),
            task_type: TaskType::Implementation,
            status: TaskStatus::Pending,
            priority: TaskPriority::Medium,
            dependencies: vec![parent_id.to_string()],
            subtasks: vec![],
            parent_id: Some(parent_id.to_string()),
            estimated_minutes: 15,
            actual_minutes: None,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
            result: None,
            tags: vec![],
            metadata: HashMap::new(),
        }
    }
}

impl TaskPlanner {
    /// Get the task templates for each task type.
    pub fn templates(&self) -> &HashMap<TaskType, TaskTemplate> {
        &self.templates
    }
}

impl Default for TaskPlanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a simple UUID
fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        (bytes[6] & 0x0f) | 0x40, bytes[7],
        (bytes[8] & 0x3f) | 0x80, bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// Task executor
pub struct TaskExecutor {
    plan: TaskPlan,
    current_tasks: Vec<String>,
}

impl TaskExecutor {
    pub fn new(plan: TaskPlan) -> Self {
        Self {
            plan,
            current_tasks: Vec::new(),
        }
    }

    /// Get next tasks to execute
    pub fn get_next_tasks(&mut self) -> Vec<&Task> {
        self.plan.get_ready_tasks()
    }

    /// Start a task
    pub fn start_task(&mut self, task_id: &str) -> bool {
        self.plan.update_task_status(task_id, TaskStatus::InProgress);
        self.current_tasks.push(task_id.to_string());
        true
    }

    /// Complete a task
    pub fn complete_task(&mut self, task_id: &str, result: TaskResult) -> bool {
        self.plan.update_task_status(task_id, TaskStatus::Completed);
        if let Some(task) = self.plan.tasks.get_mut(task_id) {
            task.result = Some(result);
        }
        self.current_tasks.retain(|id| id != task_id);
        true
    }

    /// Fail a task
    pub fn fail_task(&mut self, task_id: &str, error_msg: String) -> bool {
        self.plan.update_task_status(task_id, TaskStatus::Failed);
        if let Some(task) = self.plan.tasks.get_mut(task_id) {
            task.result = Some(TaskResult {
                success: false,
                output: error_msg.clone(),
                files_created: vec![],
                files_modified: vec![],
                errors: vec![error_msg],
                warnings: vec![],
            });
        }
        self.current_tasks.retain(|id| id != task_id);
        true
    }

    /// Get plan status
    pub fn get_status(&self) -> (&TaskPlan, &[String]) {
        (&self.plan, &self.current_tasks)
    }

    /// Check if all tasks are done
    pub fn is_complete(&self) -> bool {
        self.plan.status == PlanStatus::Completed || 
        self.plan.status == PlanStatus::Failed ||
        self.plan.tasks.values().all(|t| {
            matches!(t.status, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Skipped)
        })
    }
}
