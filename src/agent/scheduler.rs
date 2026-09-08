//! Task Scheduler — Automation system for scheduled, event-triggered, and loop tasks.
//!
//! Provides cron-based scheduling, interval timers, event-driven triggers,
//! and `/loop` style iterative execution (Claude Code pattern).
//!
//! ## Usage
//!
//! ```no_run
//! use deepseek_carp::agent::scheduler::{TaskScheduler, ScheduleKind, ScheduledTask};
//! # async {
//! let mut scheduler = TaskScheduler::new();
//! // Add an interval task
//! scheduler.add_task(ScheduledTask {
//!     id: "run-tests".into(),
//!     schedule: ScheduleKind::Interval(std::time::Duration::from_secs(300)),
//!     prompt: "Run cargo test and report results".into(),
//!     ..Default::default()
//! }).await;
//! // Start background scheduler
//! scheduler.start().await;
//! // Trigger manually
//! scheduler.run_task("run-tests").await;
//! # };
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tracing;

/// Kind of schedule for a task.
#[derive(Debug, Clone)]
pub enum ScheduleKind {
    /// Cron expression (e.g., "*/5 * * * *")
    Cron(String),
    /// Fixed interval (e.g., every 5 minutes)
    Interval(Duration),
    /// Event-driven trigger (e.g., "git.push", "file.save")
    Event(String),
    /// One-shot execution
    Once,
    /// Loop until condition is met or max iterations reached
    Loop {
        max_iterations: u32,
        condition: String,
    },
}

impl std::fmt::Display for ScheduleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cron(c) => write!(f, "cron({})", c),
            Self::Interval(d) => write!(f, "interval({:.0}s)", d.as_secs()),
            Self::Event(e) => write!(f, "event({})", e),
            Self::Once => write!(f, "once"),
            Self::Loop { max_iterations, condition } => {
                write!(f, "loop(until={}, max={})", condition, max_iterations)
            }
        }
    }
}

/// Execution status of a scheduled task.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Stopped,
    Completed,
}

impl std::fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Succeeded => write!(f, "succeeded"),
            Self::Failed => write!(f, "failed"),
            Self::Stopped => write!(f, "stopped"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

/// A scheduled automation task.
#[derive(Debug, Clone)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub schedule: ScheduleKind,
    pub prompt: String,
    pub agent_config: Option<String>, // JSON config override
    pub status: ScheduleStatus,
    pub last_run: Option<Instant>,
    pub next_run: Option<Instant>,
    pub last_result: Option<String>,
    pub run_count: u32,
    pub fail_count: u32,
    pub created_at: Instant,
}

impl Default for ScheduledTask {
    fn default() -> Self {
        Self {
            id: format!("task_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..8]),
            name: String::new(),
            schedule: ScheduleKind::Once,
            prompt: String::new(),
            agent_config: None,
            status: ScheduleStatus::Pending,
            last_run: None,
            next_run: None,
            last_result: None,
            run_count: 0,
            fail_count: 0,
            created_at: Instant::now(),
        }
    }
}

/// Result of a single task execution.
#[derive(Debug, Clone)]
pub struct TaskExecutionResult {
    pub task_id: String,
    pub success: bool,
    pub output: String,
    pub duration_ms: u64,
    pub tokens_used: u32,
    pub iteration: u32, // For Loop tasks: which iteration this was
}

// Internal channel commands for the scheduler loop
enum SchedulerCommand {
    AddTask(ScheduledTask),
    RemoveTask(String),
    RunTask(String),
    TriggerEvent(String),
    ListTasks(oneshot::Sender<Vec<ScheduledTask>>),
    GetHistory(oneshot::Sender<Vec<TaskExecutionResult>>),
    Stop,
}

/// The central task scheduler.
pub struct TaskScheduler {
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
    history: Arc<RwLock<Vec<TaskExecutionResult>>>,
    event_tx: broadcast::Sender<String>,
    command_tx: mpsc::Sender<SchedulerCommand>,
    running: Arc<RwLock<bool>>,
}

impl TaskScheduler {
    /// Create a new scheduler instance.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel::<String>(64);
        let (command_tx, mut command_rx) = mpsc::channel::<SchedulerCommand>(64);

        let scheduler = Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
            event_tx,
            command_tx,
            running: Arc::new(RwLock::new(false)),
        };

        // Spawn the background scheduler loop
        let tasks_clone = scheduler.tasks.clone();
        let history_clone = scheduler.history.clone();
        let running_clone = scheduler.running.clone();

        tokio::spawn(async move {
            let mut interval_tick = tokio::time::interval(Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = interval_tick.tick() => {
                        // Check all tasks for due execution
                        let mut tasks = tasks_clone.write().await;
                        let now = Instant::now();
                        for (_, task) in tasks.iter_mut() {
                            if task.status == ScheduleStatus::Pending || task.status == ScheduleStatus::Succeeded || task.status == ScheduleStatus::Failed {
                                if let Some(next) = task.next_run {
                                    if now >= next {
                                        task.status = ScheduleStatus::Running;
                                    }
                                }
                            }
                        }
                        drop(tasks);
                    }

                    Some(cmd) = command_rx.recv() => {
                        match cmd {
                            SchedulerCommand::AddTask(new_task) => {
                                let mut tasks = tasks_clone.write().await;
                                let next = Self::calculate_next_run(&new_task.schedule);
                                let mut task = new_task.clone();
                                task.next_run = next;
                                tasks.insert(task.id.clone(), task);
                                tracing::info!(task_id=%new_task.id, "Scheduled task registered");
                                drop(tasks);
                            }
                            SchedulerCommand::RemoveTask(id) => {
                                let mut tasks = tasks_clone.write().await;
                                if tasks.remove(&id).is_some() {
                                    tracing::info!(task_id=%id, "Task removed");
                                }
                            }
                            SchedulerCommand::RunTask(id) => {
                                // Mark as running — actual execution happens externally via execute_task()
                                let mut tasks = tasks_clone.write().await;
                                if let Some(task) = tasks.get_mut(&id) {
                                    task.status = ScheduleStatus::Running;
                                    task.last_run = Some(Instant::now());
                                    task.run_count += 1;
                                }
                            }
                            SchedulerCommand::TriggerEvent(event_name) => {
                                let mut tasks = tasks_clone.write().await;
                                let now = Instant::now();
                                for (_, task) in tasks.iter_mut() {
                                    if matches!(&task.schedule, ScheduleKind::Event(e) if e == &event_name)
                                        && task.status != ScheduleStatus::Running {
                                            task.status = ScheduleStatus::Running;
                                            task.last_run = Some(now);
                                            task.run_count += 1;
                                            tracing::info!(task_id=%task.id, event=%event_name, "Event-triggered task");
                                        }
                                }
                            }
                            SchedulerCommand::ListTasks(tx) => {
                                let tasks = tasks_clone.read().await;
                                let list: Vec<ScheduledTask> = tasks.values().cloned().collect();
                                let _ = tx.send(list);
                            }
                            SchedulerCommand::GetHistory(tx) => {
                                let hist = history_clone.read().await;
                                let _ = tx.send(hist.clone());
                            }
                            SchedulerCommand::Stop => {
                                *running_clone.write().await = false;
                                break;
                            }
                        }
                    }
                }
            }
        });

        scheduler
    }

    /// Register a new scheduled task.
    pub async fn add_task(&self, task: ScheduledTask) -> anyhow::Result<String> {
        let id = task.id.clone();
        self.command_tx.send(SchedulerCommand::AddTask(task)).await?;
        Ok(id)
    }

    /// Remove a task by ID.
    pub async fn remove_task(&self, id: &str) -> anyhow::Result<()> {
        self.command_tx.send(SchedulerCommand::RemoveTask(id.to_string())).await?;
        Ok(())
    }

    /// Manually trigger a task execution.
    pub async fn run_task(&self, id: &str) -> anyhow::Result<()> {
        self.command_tx.send(SchedulerCommand::RunTask(id.to_string())).await?;
        Ok(())
    }

    /// Trigger all tasks listening to an event.
    pub async fn trigger_event(&self, event_name: &str) -> anyhow::Result<u32> {
        // Send to event channel for external listeners
        let _ = self.event_tx.send(event_name.to_string());
        // Also notify internal scheduler
        self.command_tx.send(SchedulerCommand::TriggerEvent(event_name.to_string())).await?;
        Ok(1)
    }

    /// List all registered tasks.
    pub async fn list_tasks(&self) -> Vec<ScheduledTask> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(SchedulerCommand::ListTasks(tx)).await;
        rx.await.unwrap_or_default()
    }

    /// Get execution history.
    pub async fn get_history(&self) -> Vec<TaskExecutionResult> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(SchedulerCommand::GetHistory(tx)).await;
        rx.await.unwrap_or_default()
    }

    /// Record a task execution result (called after Agent finishes executing).
    pub async fn record_result(&self, result: TaskExecutionResult) {
        let mut history = self.history.write().await;
        history.push(result.clone());

        // Update task status
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(&result.task_id) {
            task.last_result = Some(result.output.clone());
            if result.success {
                task.status = ScheduleStatus::Succeeded;
                // Calculate next run for recurring tasks
                task.next_run = Self::calculate_next_run(&task.schedule);
            } else {
                task.status = ScheduleStatus::Failed;
                task.fail_count += 1;
                // Still calculate next run so failed tasks retry
                task.next_run = Self::calculate_next_run(&task.schedule);
            }
        }
    }

    /// Get a receiver for event notifications.
    pub fn subscribe_events(&self) -> broadcast::Receiver<String> {
        self.event_tx.subscribe()
    }

    /// Stop the scheduler background loop.
    pub async fn stop(&self) -> anyhow::Result<()> {
        self.command_tx.send(SchedulerCommand::Stop).await?;
        *self.running.write().await = false;
        Ok(())
    }

    /// Check if scheduler is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Get task count.
    pub async fn task_count(&self) -> usize {
        self.tasks.read().await.len()
    }

    /// Calculate when the next run should happen based on schedule kind.
    fn calculate_next_run(schedule: &ScheduleKind) -> Option<Instant> {
        let now = Instant::now();
        match schedule {
            ScheduleKind::Cron(_cron_expr) => {
                // Simplified: parse basic cron intervals
                // Full cron parsing would require tokio-cron-schedule crate
                // For now, default to 5-minute interval as fallback
                Some(now + Duration::from_secs(300))
            }
            ScheduleKind::Interval(dur) => Some(now + *dur),
            ScheduleKind::Event(_) => None, // Event-driven: no fixed time
            ScheduleKind::Once => None,
            ScheduleKind::Loop { .. } => Some(now + Duration::from_secs(10)), // Short delay between iterations
        }
    }

    /// Format all tasks as a human-readable table string.
    pub async fn format_task_table(&self) -> String {
        let tasks = self.list_tasks().await;
        if tasks.is_empty() {
            return "No scheduled tasks.".to_string();
        }

        let mut lines = vec![
            String::new(),
            format!("{:<16} {:<24} {:<20} {:<8} {:>6} {:>6} {:<12}",
                "ID", "NAME", "SCHEDULE", "STATUS", "RUNS", "FAILS", "LAST RUN"),
            "-".repeat(100),
        ];

        for t in &tasks {
            let last = t.last_run.map(|i| {
                let elapsed = i.elapsed();
                if elapsed.as_secs() < 60 {
                    format!("{}s ago", elapsed.as_secs())
                } else if elapsed.as_secs() < 3600 {
                    format!("{}m ago", elapsed.as_secs() / 60)
                } else {
                    format!("{}h ago", elapsed.as_secs() / 3600)
                }
            }).unwrap_or_else(|| "never".to_string());

            lines.push(format!(
                "{:<16} {:<24} {:<20} {:<8} {:>6} {:>6} {:<12}",
                &t.id[..t.id.len().min(16)],
                &t.name[..t.name.len().min(24)],
                &t.schedule.to_string()[..t.schedule.to_string().len().min(20)],
                t.status.to_string(),
                t.run_count,
                t.fail_count,
                last,
            ));
        }

        lines.join("\n")
    }

    /// Format execution history as table.
    pub async fn format_history_table(&self) -> String {
        let hist = self.get_history().await;
        if hist.is_empty() {
            return "No execution history.".to_string();
        }

        let recent: Vec<_> = hist.iter().rev().take(20).collect();
        let mut lines = vec![
            String::new(),
            format!("{:<16} {:<6} {:>8}ms {:>8}tok {:>4}it  {}",
                "TASK", "OK?", "DUR", "TOKENS", "ITER", "OUTPUT SUMMARY"),
            "-".repeat(80),
        ];

        for r in &recent {
            let summary: String = r.output.chars().take(60).collect();
            lines.push(format!(
                "{:<16} {:<6} {:>8}ms {:>8}tok {:>4}it  {}",
                &r.task_id[..r.task_id.len().min(16)],
                if r.success { "YES" } else { "NO" },
                r.duration_ms,
                r.tokens_used,
                r.iteration,
                summary,
            ));
        }

        lines.join("\n")
    }
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}
