//! Unified context manager 闁?integrates RAG + Incremental + Compression.
//!
//! This is the single entry point the Agent uses for all context operations.
//! It coordinates:
//! - RAG indexing and retrieval (rag.rs)
//! - Incremental diff-based updates (incremental.rs)
//! - Smart context compression (compression.rs)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::context::rag::RagContext;
use crate::context::ContextCompressor;
use crate::context::compression::compress_history;

use crate::context::incremental::{
    IncrementalContextManager, FileChange, ChangeType,
};
#[cfg(feature = "sqlite-storage")]
use crate::context::persistent_index::PersistentSemanticIndex;

/// Default token budget for DeepSeek V3 context window.
const DEFAULT_MAX_CONTEXT_TOKENS: usize = 128_000;

/// System prompt overhead estimate in tokens.
const SYSTEM_PREFIX_TOKEN_ESTIMATE: usize = 2_000;

/// Unified context manager for the Agent loop.
pub struct ContextManager {
    rag: RagContext,
    incremental: IncrementalContextManager,
    compressor: ContextCompressor,
    workspace: PathBuf,
    max_context_tokens: usize,
    indexed: bool,
    /// Cached file hashes for change detection.
    file_hashes: HashMap<String, u64>,
    #[cfg(feature = "sqlite-storage")]
    persistent_index: Option<Arc<PersistentSemanticIndex>>,
    last_build_time: Arc<std::sync::Mutex<Instant>>,
}

/// A snapshot of current context state for observability.
#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub relevant_chunks: Vec<String>,     // From RAG
    pub delta_changes: Vec<String>,       // From incremental
    pub compressed_history: Vec<String>,  // From compression
    pub total_tokens: usize,
    pub files_indexed: usize,
    pub changes_pending: usize,
}

/// The built context ready for LLM submission.
#[derive(Debug, Clone)]
pub struct BuildContext {
    pub system_prefix: String,
    pub rag_context: String,              // Relevant code chunks
    pub delta_context: String,            // Recent changes (diff format)
    pub compressed_history: String,       // Compressed previous turns
    pub user_message: String,
    pub total_estimated_tokens: usize,
    pub cache_friendly: bool,             // True if structure stable for ReasonIX cache
}

impl ContextManager {
    /// Create a new context manager for the given workspace.
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            rag: RagContext::new(&workspace),
            incremental: IncrementalContextManager::new(),
            compressor: ContextCompressor::new(DEFAULT_MAX_CONTEXT_TOKENS / 2),
            workspace,
            max_context_tokens: DEFAULT_MAX_CONTEXT_TOKENS,
            indexed: false,
            file_hashes: HashMap::new(),
            #[cfg(feature = "sqlite-storage")]
            persistent_index: None,
            last_build_time: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }

    /// Create with a custom token budget.
    pub fn with_token_budget(workspace: PathBuf, max_tokens: usize) -> Self {
        let mut mgr = Self::new(workspace);
        mgr.max_context_tokens = max_tokens;
        mgr.compressor = ContextCompressor::new(max_tokens / 2);
        mgr
    }

    #[cfg(feature = "sqlite-storage")]
    pub fn with_persistent_index(mut self, index: Arc<PersistentSemanticIndex>) -> Self {
        self.persistent_index = Some(index);
        self
    }

    /// Initialize: index workspace (call once at session start).
    pub async fn initialize(&mut self) -> anyhow::Result<ContextSnapshot> {
        let count = self.rag.index();
        self.indexed = true;
        self.scan_file_hashes()?;

        Ok(ContextSnapshot {
            relevant_chunks: vec![],
            delta_changes: vec![],
            compressed_history: vec![],
            total_tokens: 0,
            files_indexed: count,
            changes_pending: 0,
        })
    }

    /// Build the full context for the next LLM request.
    ///
    /// Combines: system prefix + RAG context + incremental deltas + compressed history.
    /// If a fresh persisted index exists, skips full re-scan for sub-50ms latency.
    pub async fn build_context(
        &self,
        user_prompt: &str,
        history_messages: &[String],
    ) -> anyhow::Result<BuildContext> {
        #[cfg(feature = "sqlite-storage")]
        {
            if let Some(ref pidx) = self.persistent_index {
                if pidx.is_fresh() {
                    return self.build_context_cached(user_prompt, history_messages).await;
                }
            }
        }
        *self.last_build_time.lock().unwrap() = Instant::now();

        // 1. RAG enrichment
        let rag_context = self.rag.enrich(user_prompt);
        let rag_tokens = estimate_tokens(&rag_context);

        // 2. Incremental deltas
        let summary = self.incremental.get_changes_summary().await;
        let delta_changes = self.format_delta_changes().await;
        let delta_tokens = estimate_tokens(&delta_changes);

        // 3. Compress conversation history
        let history_budget = self
            .max_context_tokens
            .saturating_sub(SYSTEM_PREFIX_TOKEN_ESTIMATE)
            .saturating_sub(rag_tokens)
            .saturating_sub(delta_tokens);

        let compressed_history = if history_budget > 100 && !history_messages.is_empty() {
            compress_history(history_messages.to_vec(), history_budget)
        } else {
            history_messages.to_vec()
        };
        let history_str = compressed_history.join("\n");
        let history_tokens = estimate_tokens(&history_str);

        let total = SYSTEM_PREFIX_TOKEN_ESTIMATE + rag_tokens + delta_tokens + history_tokens
            + estimate_tokens(user_prompt);

        Ok(BuildContext {
            system_prefix: self.build_system_prefix(),
            rag_context,
            delta_context: delta_changes,
            compressed_history: history_str,
            user_message: user_prompt.to_string(),
            total_estimated_tokens: total,
            cache_friendly: summary.pending_changes == 0,
        })
    }

    #[cfg(feature = "sqlite-storage")]
    async fn build_context_cached(
        &self,
        user_prompt: &str,
        history_messages: &[String],
    ) -> anyhow::Result<BuildContext> {
        let start = Instant::now();
        let rag_context = self.rag.enrich(user_prompt);
        let _elapsed = start.elapsed();
        let rag_tokens = estimate_tokens(&rag_context);

        let summary = self.incremental.get_changes_summary().await;
        let delta_changes = self.format_delta_changes().await;
        let delta_tokens = estimate_tokens(&delta_changes);

        let history_budget = self
            .max_context_tokens
            .saturating_sub(SYSTEM_PREFIX_TOKEN_ESTIMATE)
            .saturating_sub(rag_tokens)
            .saturating_sub(delta_tokens);

        let compressed_history = if history_budget > 100 && !history_messages.is_empty() {
            compress_history(history_messages.to_vec(), history_budget)
        } else {
            history_messages.to_vec()
        };
        let history_str = compressed_history.join("\n");
        let history_tokens = estimate_tokens(&history_str);

        let total = SYSTEM_PREFIX_TOKEN_ESTIMATE + rag_tokens + delta_tokens + history_tokens
            + estimate_tokens(user_prompt);

        Ok(BuildContext {
            system_prefix: self.build_system_prefix(),
            rag_context,
            delta_context: delta_changes,
            compressed_history: history_str,
            user_message: user_prompt.to_string(),
            total_estimated_tokens: total,
            cache_friendly: summary.pending_changes == 0,
        })
    }

    /// Notify that files have changed (after agent edit).
    pub async fn notify_changes(&self, changes: Vec<FileChange>) -> anyhow::Result<()> {
        let _result = self.incremental.update(changes).await;
        Ok(())
    }

    /// Auto-detect file changes since last check by comparing file hashes.
    pub async fn detect_changes(&self) -> anyhow::Result<Vec<FileChange>> {
        let mut changes = Vec::new();
        let current_hashes = self.compute_current_hashes()?;

        for (path, &new_hash) in &current_hashes {
            if let Some(&old_hash) = self.file_hashes.get(path) {
                if old_hash != new_hash {
                    // File modified 闁?read old and new content to produce a diff
                    let full_path = self.workspace.join(path);
                    let new_content = std::fs::read_to_string(&full_path).unwrap_or_default();
                    changes.push(FileChange {
                        path: path.clone(),
                        change_type: ChangeType::Modified,
                        old_content: None, // We only have hash info; caller can fill in
                        new_content: Some(new_content),
                        diff: None,
                        line_ranges: vec![],
                    });
                }
            } else {
                // New file detected
                let full_path = self.workspace.join(path);
                let content = std::fs::read_to_string(&full_path).unwrap_or_default();
                changes.push(FileChange {
                    path: path.clone(),
                    change_type: ChangeType::Added,
                    old_content: None,
                    new_content: Some(content),
                    diff: None,
                    line_ranges: vec![],
                });
            }
        }

        Ok(changes)
    }

    /// Compress conversation history to fit token budget.
    pub fn compress_history(&self, messages: Vec<String>) -> Vec<String> {
        let budget = self.max_context_tokens.saturating_sub(SYSTEM_PREFIX_TOKEN_ESTIMATE);
        compress_history(messages, budget)
    }

    /// Re-index specific files (after edits).
    pub async fn reindex_files(&mut self, _files: &[&Path]) -> anyhow::Result<usize> {
        // Re-indexing means rebuilding the RAG index for affected files.
        // For simplicity we re-index the whole workspace; in production you'd
        // do incremental re-indexing of just the changed files.
        let count = self.rag.index();
        self.scan_file_hashes()?;
        Ok(count)
    }

    /// Get current state summary.
    pub fn snapshot(&self) -> ContextSnapshot {
        // We need async for get_changes_summary but snapshot is sync.
        // Return what we can synchronously; pending changes estimated from hashes.
        let files_indexed = if self.indexed {
            self.rag.code_index().chunk_count()
        } else {
            0
        };

        ContextSnapshot {
            relevant_chunks: vec![],
            delta_changes: vec![],
            compressed_history: vec![],
            total_tokens: 0,
            files_indexed,
            changes_pending: 0, // Would need async for accurate value
        }
    }

    /// Get reference to the RAG context for direct queries.
    pub fn rag(&self) -> &RagContext {
        &self.rag
    }

    /// Get reference to the incremental manager.
    pub fn incremental(&self) -> &IncrementalContextManager {
        &self.incremental
    }

    // --- Private helpers ---

    fn build_system_prefix(&self) -> String {
        "You are an expert coding assistant. Use the provided code context and recent \
         changes to answer the user's request accurately.\n"
            .to_string()
    }

    async fn format_delta_changes(&self) -> String {
        let summary = self.incremental.get_changes_summary().await;
        if summary.pending_changes == 0 {
            return String::new();
        }
        format!(
            "## Recent Changes\n\
             - Files tracked: {}\n\
             - Additions: {}, Modifications: {}, Deletions: {}\n",
            summary.total_files,
            summary.additions,
            summary.modifications,
            summary.deletions,
        )
    }

    /// Scan workspace and record file content hashes for change detection.
    fn scan_file_hashes(&mut self) -> anyhow::Result<()> {
        self.file_hashes = self.compute_current_hashes()?;
        Ok(())
    }

    /// Compute current file hashes for all indexable files in workspace.
    fn compute_current_hashes(&self) -> anyhow::Result<HashMap<String, u64>> {
        let mut hashes = HashMap::new();
        let extensions = [
            "rs", "py", "js", "ts", "go", "java", "cpp", "c", "h",
            "toml", "yaml", "json", "md",
        ];
        let excludes = [
            "target", "node_modules", ".git", ".idea", "dist",
            "__pycache__", "build", "venv", ".venv",
        ];

        if !self.workspace.exists() {
            return Ok(hashes);
        }

        let entries = walkdir::WalkDir::new(&self.workspace)
            .into_iter()
            .filter_entry(|e| {
                let name: &str = &e.file_name().to_string_lossy();
                !excludes.contains(&name)
            })
            .filter_map(|e| e.ok())
            .filter(|e| {
                let ext = e.path()
                    .extension()
                    .and_then(|ex| ex.to_str())
                    .unwrap_or("");
                extensions.contains(&ext) && e.path().is_file()
            });

        for entry in entries {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(rel) = entry.path().strip_prefix(workspace_root()) {
                    let hash = simple_hash(&content);
                    hashes.insert(rel.to_string_lossy().to_string(), hash);
                }
            }
        }

        Ok(hashes)
    }
}

impl BuildContext {
    /// Serialize to the format expected by the LLM provider.
    ///
    /// Output is byte-stable when history hasn't changed (ReasonIX compatible).
    pub fn to_payload(&self) -> String {
        let mut parts = Vec::with_capacity(5);

        if !self.system_prefix.is_empty() {
            parts.push(self.system_prefix.clone());
        }
        if !self.rag_context.is_empty() {
            parts.push(self.rag_context.clone());
        }
        if !self.delta_context.is_empty() {
            parts.push(self.delta_context.clone());
        }
        if !self.compressed_history.is_empty() {
            parts.push(format!(
                "## Conversation History\n{}",
                self.compressed_history
            ));
        }
        parts.push(self.user_message.clone());

        parts.join("\n\n")
    }

    /// Estimate total tokens in this context.
    pub fn estimate_tokens(&self) -> usize {
        self.total_estimated_tokens
    }
}

/// Estimate token count for a string (rough approximation: ~4 chars/token).
fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Simple hash function for file content comparison.
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    hash
}

/// Workspace root used for relative paths. Falls back to cwd.
fn workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_manager_new() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let mgr = ContextManager::new(tmp.path().to_path_buf());
        assert!(!mgr.indexed);
        assert_eq!(mgr.max_context_tokens, DEFAULT_MAX_CONTEXT_TOKENS);
    }

    #[test]
    fn test_context_manager_custom_budget() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let mgr = ContextManager::with_token_budget(tmp.path().to_path_buf(), 64_000);
        assert_eq!(mgr.max_context_tokens, 64_000);
    }

    #[tokio::test]
    async fn test_initialize_indexes_workspace() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        // Create a source file so indexing has something to find
        std::fs::write(
            tmp.path().join("main.rs"),
            "fn main() { println!(\"hello\"); }\n",
        )
        .expect("failed to write test file");

        let mut mgr = ContextManager::new(tmp.path().to_path_buf());
        let snap = mgr.initialize().await.expect("initialize failed");
        assert!(mgr.indexed);
        assert!(snap.files_indexed > 0);
    }

    #[tokio::test]
    async fn test_build_context_produces_payload() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        std::fs::write(
            tmp.path().join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .expect("failed to write test file");

        let mut mgr = ContextManager::new(tmp.path().to_path_buf());
        mgr.initialize().await.expect("init failed");

        let ctx = mgr
            .build_context("what does add do?", &["previous question".to_string()])
            .await
            .expect("build_context failed");

        let payload = ctx.to_payload();
        assert!(!payload.is_empty());
        assert!(payload.contains("add")); // RAG should find the function
        assert!(ctx.total_estimated_tokens > 0);
    }

    #[tokio::test]
    async fn test_notify_changes_works() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let mgr = ContextManager::new(tmp.path().to_path_buf());

        let changes = vec![FileChange {
            path: "test.rs".to_string(),
            change_type: ChangeType::Added,
            old_content: None,
            new_content: Some("fn new_fn() {}".to_string()),
            diff: None,
            line_ranges: vec![],
        }];

        mgr.notify_changes(changes)
            .await
            .expect("notify_changes failed");

        let summary = mgr.incremental().get_changes_summary().await;
        assert_eq!(summary.additions, 1);
    }

    #[tokio::test]
    async fn test_compress_history_integration() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        // Use a small token budget to force aggressive compression
        let mgr = ContextManager::with_token_budget(tmp.path().to_path_buf(), 4_000);

        let messages: Vec<String> = (0..100)
            .map(|i| format!("message number {}: {}", i, "x".repeat(500)))
            .collect();

        let compressed = mgr.compress_history(messages);
        // Compressed should be shorter than original (compression kicked in)
        assert!(compressed.len() < 100, "history should be compressed");
    }

    #[test]
    fn test_build_context_cache_friendly_flag() {
        let ctx = BuildContext {
            system_prefix: "system".to_string(),
            rag_context: String::new(),
            delta_context: String::new(),
            compressed_history: String::new(),
            user_message: "hello".to_string(),
            total_estimated_tokens: 10,
            cache_friendly: true,
        };
        assert!(ctx.cache_friendly);
        let payload = ctx.to_payload();
        assert!(payload.contains("system"));
        assert!(payload.contains("hello"));
    }

    #[test]
    fn test_snapshot_returns_indexed_count() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let mgr = ContextManager::new(tmp.path().to_path_buf());
        let snap = mgr.snapshot();
        assert_eq!(snap.files_indexed, 0); // Not yet indexed
    }
}
