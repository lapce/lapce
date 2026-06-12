//! Persistent background task queue — CodeWhale-style durable execution.
//!
//! ## Architecture
//!
//! ```text
//! Enqueue → Queue (VecDeque) → Worker Pool (bounded, 2-8 workers)
//!            ↓                      ↓
//!       queue.json              per-task JSON files
//!            ↓                      ↓
//!      Survives restart          Timeline entries
//! ```
//!
//! ## Features
//! - JSON-per-task persistence (survives restarts)
//! - Bounded worker pool with semaphore control
//! - CancellationToken graceful shutdown
//! - Status lifecycle: Queued → Running → Completed/Failed/Canceled/TimedOut
//! - Recovery: Running tasks reset to Queued on restart
//! - Artifact routing for large outputs

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

// ── Task Status ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskQueueStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Canceled,
    TimedOut,
}

// ── Task Record ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    /// Unique task ID.
    pub id: String,
    /// Human-readable task description.
    pub description: String,
    /// Task payload (e.g., shell command, prompt, file path).
    pub payload: String,
    /// Current status.
    pub status: TaskQueueStatus,
    /// When the task was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the task was last updated.
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Result output (if completed).
    pub result: Option<String>,
    /// Error message (if failed).
    pub error: Option<String>,
    /// Duration in seconds (if completed).
    pub duration_secs: Option<f64>,
}

impl TaskRecord {
    pub fn new(id: impl Into<String>, description: impl Into<String>, payload: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: id.into(),
            description: description.into(),
            payload: payload.into(),
            status: TaskQueueStatus::Queued,
            created_at: now,
            updated_at: now,
            result: None,
            error: None,
            duration_secs: None,
        }
    }
}

// ── Task Queue ───────────────────────────────────────────────────

/// Persistent background task queue with bounded worker pool.
pub struct TaskQueue {
    /// Queued task IDs (FIFO order).
    queue: VecDeque<String>,
    /// All tasks indexed by ID.
    tasks: HashMap<String, TaskRecord>,
    /// Persistence directory.
    data_dir: PathBuf,
    /// Worker pool semaphore.
    worker_semaphore: Arc<Semaphore>,
    /// Cancellation tokens for running tasks.
    running_cancel: HashMap<String, CancellationToken>,
    /// Max concurrent workers.
    max_workers: usize,
    /// Max queued tasks (beyond this, enqueue returns error).
    max_queue_size: usize,
    /// Whether the queue has been initialized (loaded from disk).
    initialized: bool,
}

impl TaskQueue {
    /// Create a new task queue.
    pub fn new(data_dir: impl Into<PathBuf>, max_workers: usize, max_queue_size: usize) -> Self {
        let dir = data_dir.into();
        std::fs::create_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("tasks")).ok();

        Self {
            queue: VecDeque::new(),
            tasks: HashMap::new(),
            data_dir: dir,
            worker_semaphore: Arc::new(Semaphore::new(max_workers)),
            running_cancel: HashMap::new(),
            max_workers,
            max_queue_size,
            initialized: false,
        }
    }

    /// Load persisted tasks from disk (call once on startup).
    pub fn load_from_disk(&mut self) -> Result<usize, String> {
        let queue_path = self.data_dir.join("queue.json");
        if queue_path.exists() {
            let data = std::fs::read_to_string(&queue_path)
                .map_err(|e| format!("Failed to read queue.json: {}", e))?;
            let ids: Vec<String> = serde_json::from_str(&data)
                .map_err(|e| format!("Failed to parse queue.json: {}", e))?;
            self.queue = ids.into();
        }

        let tasks_dir = self.data_dir.join("tasks");
        if tasks_dir.exists() {
            let mut count = 0;
            for entry in std::fs::read_dir(&tasks_dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.path().extension().is_some_and(|e| e == "json") {
                    if let Ok(data) = std::fs::read_to_string(entry.path()) {
                        if let Ok(mut task) = serde_json::from_str::<TaskRecord>(&data) {
                            // Reset Running tasks to Queued on restart
                            if task.status == TaskQueueStatus::Running {
                                task.status = TaskQueueStatus::Queued;
                                task.updated_at = chrono::Utc::now();
                                // Re-save the corrected status
                                self.save_task(&task);
                            }
                            self.tasks.insert(task.id.clone(), task);
                            count += 1;
                        }
                    }
                }
            }
            self.initialized = true;
            return Ok(count);
        }

        self.initialized = true;
        Ok(0)
    }

    /// Enqueue a new task. Returns the task ID.
    pub fn enqueue(
        &mut self,
        description: impl Into<String>,
        payload: impl Into<String>,
    ) -> Result<String, String> {
        if self.queue.len() >= self.max_queue_size {
            return Err(format!("Queue full (max: {})", self.max_queue_size));
        }

        let id = format!("task_{}", uuid::Uuid::new_v4().to_string().chars().take(12).collect::<String>());
        let task = TaskRecord::new(&id, description, payload);
        self.save_task(&task);
        self.tasks.insert(id.clone(), task);
        self.queue.push_back(id.clone());
        self.save_queue();
        Ok(id)
    }

    /// Dequeue the next task for execution. Returns None if queue is empty.
    pub fn dequeue(&mut self) -> Option<TaskRecord> {
        if let Some(id) = self.queue.pop_front() {
            if let Some(mut task) = self.tasks.remove(&id) {
                task.status = TaskQueueStatus::Running;
                task.updated_at = chrono::Utc::now();
                self.save_task(&task);
                self.tasks.insert(id.clone(), task.clone());
                self.save_queue();
                return Some(task);
            }
        }
        None
    }

    /// Mark a task as completed.
    pub fn complete(&mut self, task_id: &str, result: impl Into<String>) {
        let task_snapshot = if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskQueueStatus::Completed;
            task.result = Some(result.into());
            task.updated_at = chrono::Utc::now();
            task.duration_secs = Some(
                (task.updated_at - task.created_at).num_milliseconds() as f64 / 1000.0
            );
            task.clone()
        } else {
            return;
        };
        self.save_task(&task_snapshot);
        self.running_cancel.remove(task_id);
    }

    /// Mark a task as failed.
    pub fn fail(&mut self, task_id: &str, error: impl Into<String>) {
        let task_snapshot = if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskQueueStatus::Failed;
            task.error = Some(error.into());
            task.updated_at = chrono::Utc::now();
            task.clone()
        } else {
            return;
        };
        self.save_task(&task_snapshot);
        self.running_cancel.remove(task_id);
    }

    /// Cancel a running task.
    pub fn cancel(&mut self, task_id: &str) -> bool {
        if let Some(token) = self.running_cancel.remove(task_id) {
            token.cancel();
            if let Some(task) = self.tasks.get_mut(task_id) {
                task.status = TaskQueueStatus::Canceled;
                task.updated_at = chrono::Utc::now();
                let snapshot = task.clone();
                self.save_task(&snapshot);
            }
            true
        } else {
            false
        }
    }

    /// Get a cancellation token for a task (caller uses this to check cancellation).
    pub fn cancel_token(&mut self, task_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.running_cancel.insert(task_id.to_string(), token.clone());
        token
    }

    /// Get task by ID.
    pub fn get(&self, task_id: &str) -> Option<&TaskRecord> {
        self.tasks.get(task_id)
    }

    /// List all tasks with their status.
    pub fn list(&self) -> Vec<&TaskRecord> {
        let mut all: Vec<&TaskRecord> = self.tasks.values().collect();
        all.sort_by_key(|t| t.created_at);
        all
    }

    /// Get queue summary for display.
    pub fn summary(&self) -> String {
        let queued = self.tasks.values().filter(|t| t.status == TaskQueueStatus::Queued).count();
        let running = self.tasks.values().filter(|t| t.status == TaskQueueStatus::Running).count();
        let completed = self.tasks.values().filter(|t| t.status == TaskQueueStatus::Completed).count();
        let failed = self.tasks.values().filter(|t| t.status == TaskQueueStatus::Failed).count();

        format!(
            "Task Queue: {} total ({} queued, {} running, {} completed, {} failed)",
            self.tasks.len(), queued, running, completed, failed
        )
    }

    /// Get the worker semaphore for acquiring permits.
    pub fn worker_semaphore(&self) -> Arc<Semaphore> {
        self.worker_semaphore.clone()
    }

    /// Get max workers.
    pub fn max_workers(&self) -> usize { self.max_workers }

    // ── Internal persistence ──

    fn save_task(&self, task: &TaskRecord) {
        let path = self.data_dir.join("tasks").join(format!("{}.json", task.id));
        if let Ok(json) = serde_json::to_string_pretty(task) {
            let _ = std::fs::write(&path, json);
        }
    }

    fn save_queue(&self) {
        let path = self.data_dir.join("queue.json");
        let ids: Vec<&String> = self.queue.iter().collect();
        if let Ok(json) = serde_json::to_string(&ids) {
            let _ = std::fs::write(&path, json);
        }
    }
}

// ── Task Executor Trait ──────────────────────────────────────────

/// Implement this trait to execute tasks in the worker pool.
#[async_trait::async_trait]
pub trait TaskExecutor: Send + Sync {
    /// Execute a task. Check `cancel_token.is_cancelled()` periodically.
    async fn execute(
        &self,
        task: &TaskRecord,
        cancel_token: CancellationToken,
    ) -> Result<String, String>;
}

// ── Worker Pool ──────────────────────────────────────────────────

/// Spawn worker tasks that dequeue and execute tasks.
pub async fn spawn_workers(
    queue: Arc<Mutex<TaskQueue>>,
    executor: Arc<dyn TaskExecutor>,
) {
    let max = { queue.lock().await.max_workers() };
    let semaphore = { queue.lock().await.worker_semaphore() };

    for _ in 0..max {
        let queue = queue.clone();
        let executor = executor.clone();
        let semaphore = semaphore.clone();

        tokio::spawn(async move {
            loop {
                let _permit = semaphore.acquire().await;
                if let Ok(permit) = semaphore.try_acquire() {
                    drop(permit);
                }

                let task = {
                    let mut q = queue.lock().await;
                    q.dequeue()
                };

                match task {
                    Some(task) => {
                        let task_id = task.id.clone();
                        let cancel_token = {
                            let mut q = queue.lock().await;
                            q.cancel_token(&task_id)
                        };

                        match executor.execute(&task, cancel_token).await {
                            Ok(result) => {
                                let mut q = queue.lock().await;
                                q.complete(&task_id, result);
                            }
                            Err(e) => {
                                let mut q = queue.lock().await;
                                q.fail(&task_id, e);
                            }
                        }
                    }
                    None => {
                        // Queue empty — release permit and wait
                        drop(semaphore.acquire().await);
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_enqueue_dequeue() {
        let dir = TempDir::new().unwrap();
        let mut queue: TaskQueue = TaskQueue::new(dir.path(), 2, 10);

        let id = queue.enqueue("Test task", "echo hello").unwrap();
        assert!(!id.is_empty());

        let dequeued = queue.dequeue().unwrap();
        assert_eq!(dequeued.description, "Test task");
        assert_eq!(dequeued.status, TaskQueueStatus::Running);
    }

    #[test]
    fn test_complete_task() {
        let dir = TempDir::new().unwrap();
        let mut queue: TaskQueue = TaskQueue::new(dir.path(), 2, 10);

        let id = queue.enqueue("Test", "payload").unwrap();
        let task = queue.dequeue().unwrap();

        queue.complete(&task.id, "OK");
        let completed = queue.get(&id).unwrap();
        assert_eq!(completed.status, TaskQueueStatus::Completed);
        assert_eq!(completed.result.as_deref(), Some("OK"));
    }

    #[test]
    fn test_queue_full() {
        let dir = TempDir::new().unwrap();
        let mut queue: TaskQueue = TaskQueue::new(dir.path(), 2, 3);

        for i in 0..3 {
            queue.enqueue(format!("task {}", i), "payload").unwrap();
        }
        assert!(queue.enqueue("overflow", "payload").is_err());
    }

    #[test]
    fn test_summary() {
        let dir = TempDir::new().unwrap();
        let mut queue: TaskQueue = TaskQueue::new(dir.path(), 2, 10);

        queue.enqueue("t1", "p1").unwrap();
        queue.enqueue("t2", "p2").unwrap();

        let summary = queue.summary();
        assert!(summary.contains("2 total"));
        assert!(summary.contains("2 queued"));
    }
}
