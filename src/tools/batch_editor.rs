//! Atomic Multi-file Batch Editor — transactional multi-file editing with rollback.
//!
//! When an AI agent makes changes across multiple files (e.g., renaming a function
//! that touches 15 files), if it fails halfway through the codebase is left in a
//! broken inconsistent state. This module provides **transaction semantics**:
//!
//! ```text
//! begin_txn → add_edit × N → validate → commit (atomic) | rollback
//! ```
//!
//! On commit failure, all applied edits are automatically rolled back in reverse
//! order so the workspace is never left in a partial state.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use similar::{ChangeTag, TextDiff};
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Core Types ──────────────────────────────────────────────────────────────

/// A single file edit operation within a transaction.
#[derive(Debug, Clone)]
pub struct FileEdit {
    /// Target path relative to workspace root.
    pub path: String,
    /// Original content (`None` for brand-new files).
    pub old_content: Option<String>,
    /// New content (`None` for deletions).
    pub new_content: Option<String>,
    /// What kind of edit this is.
    pub edit_type: EditType,
    /// Human-readable description, e.g. "rename Foo to Bar".
    pub description: String,
    /// Other file paths this edit depends on (must be applied first).
    pub dependencies: Vec<String>,
    /// Confidence score 0.0–1.0 from the AI model.
    pub confidence: f32,
}

impl FileEdit {
    /// Convenience constructor for a modify edit.
    pub fn modify(path: impl Into<String>, old: String, new: String, desc: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            old_content: Some(old),
            new_content: Some(new),
            edit_type: EditType::Modify,
            description: desc.into(),
            dependencies: Vec::new(),
            confidence: 1.0,
        }
    }

    /// Convenience constructor for creating a new file.
    pub fn create(path: impl Into<String>, content: String, desc: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            old_content: None,
            new_content: Some(content),
            edit_type: EditType::Create,
            description: desc.into(),
            dependencies: Vec::new(),
            confidence: 1.0,
        }
    }

    /// Convenience constructor for deleting a file.
    pub fn delete(path: impl Into<String>, original: String, desc: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            old_content: Some(original),
            new_content: None,
            edit_type: EditType::Delete,
            description: desc.into(),
            dependencies: Vec::new(),
            confidence: 1.0,
        }
    }

    /// Convenience constructor for renaming / moving a file.
    pub fn rename(
        old_path: impl Into<String>,
        new_path: impl Into<String>,
        content: String,
        desc: impl Into<String>,
    ) -> (Self, Self) {
        let old = old_path.into();
        let new = new_path.into();
        let desc = desc.into();
        let del = Self {
            path: old.clone(),
            old_content: Some(content.clone()),
            new_content: None,
            edit_type: EditType::Rename,
            description: format!("{desc} (delete source)"),
            dependencies: Vec::new(),
            confidence: 1.0,
        };
        let crt = Self {
            path: new,
            old_content: None,
            new_content: Some(content),
            edit_type: EditType::Rename,
            description: format!("{desc} (create target)"),
            dependencies: vec![old],
            confidence: 1.0,
        };
        (del, crt)
    }
}

/// Kind of file edit operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditType {
    /// Change existing file content.
    Modify,
    /// Create a brand-new file.
    Delete,
    /// Remove an existing file.
    Create,
    /// Move / rename a file (produces two edits: delete + create).
    Rename,
    /// No-op placeholder used to satisfy dependency ordering.
    Noop,
}

impl std::fmt::Display for EditType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditType::Modify => write!(f, "MODIFY"),
            EditType::Create => write!(f, "CREATE"),
            EditType::Delete => write!(f, "DELETE"),
            EditType::Rename => write!(f, "RENAME"),
            EditType::Noop => write!(f, "NOOP"),
        }
    }
}

/// Lifecycle status of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnStatus {
    /// Edits have been added but not yet validated or applied.
    Pending,
    /// Currently applying edits (mid-commit).
    Applying,
    /// All edits were applied successfully.
    Committed,
    /// Transaction was rolled back (either manually or on failure).
    RolledBack,
    /// Some edits succeeded, some failed — needs manual resolution.
    Partial,
    /// Transaction failed entirely (e.g. validation error before any writes).
    Failed,
}

impl std::fmt::Display for TxnStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TxnStatus::Pending => write!(f, "PENDING"),
            TxnStatus::Applying => write!(f, "APPLYING"),
            TxnStatus::Committed => write!(f, "COMMITTED"),
            TxnStatus::RolledBack => write!(f, "ROLLED_BACK"),
            TxnStatus::Partial => write!(f, "PARTIAL"),
            TxnStatus::Failed => write!(f, "FAILED"),
        }
    }
}

/// Metadata attached to a transaction for logging, auditing, and debugging.
#[derive(Debug, Clone)]
pub struct TxnMetadata {
    /// Human-readable summary of what task triggered this transaction.
    pub task_description: String,
    /// Which agent / subsystem initiated the edits.
    pub agent_name: String,
    /// Which LLM model generated these edits.
    pub model_used: String,
    /// Estimated risk level of this batch of changes.
    pub estimated_risk: RiskLevel,
    /// Number of distinct files affected.
    pub affected_files: usize,
    /// Total lines added across all edits.
    pub total_additions: usize,
    /// Total lines removed across all edits.
    pub total_deletions: usize,
}

impl Default for TxnMetadata {
    fn default() -> Self {
        Self {
            task_description: String::new(),
            agent_name: "default".into(),
            model_used: "unknown".into(),
            estimated_risk: RiskLevel::Low,
            affected_files: 0,
            total_additions: 0,
            total_deletions: 0,
        }
    }
}

/// Risk classification for a transaction's impact on the codebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum RiskLevel {
    /// Single-file, simple change.
    Low,
    /// Multiple files, well-understood pattern.
    Medium,
    /// Cross-cutting concern touching many modules.
    High,
    /// Core-type refactor renaming across 10+ files.
    Critical,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "LOW"),
            RiskLevel::Medium => write!(f, "MEDIUM"),
            RiskLevel::High => write!(f, "HIGH"),
            RiskLevel::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// A transaction that groups multiple [`FileEdit`] operations atomically.
#[derive(Debug, Clone)]
pub struct BatchTransaction {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// Ordered list of edits (after dependency sorting at commit time).
    pub edits: Vec<FileEdit>,
    /// Current lifecycle status.
    pub status: TxnStatus,
    /// Indices into `edits` that were successfully applied.
    pub applied_edits: Vec<usize>,
    /// Indices of edits that failed, with error messages.
    pub failed_edits: Vec<(usize, String)>,
    /// When this transaction was created.
    pub created_at: Instant,
    /// When the transaction was committed (if applicable).
    pub committed_at: Option<Instant>,
    /// Snapshot of original file contents keyed by relative path — used for rollback.
    pub rollback_data: HashMap<String, String>,
    /// Audit metadata.
    pub metadata: TxnMetadata,
}

impl BatchTransaction {
    /// Returns true if this transaction can still be modified (edits added / committed).
    pub fn is_mutable(&self) -> bool {
        matches!(self.status, TxnStatus::Pending)
    }

    /// Returns the elapsed duration since creation.
    pub fn elapsed_ms(&self) -> u64 {
        self.created_at.elapsed().as_millis() as u64
    }
}

/// Result returned after attempting to commit a transaction.
#[derive(Debug, Clone)]
pub struct TxnResult {
    /// Whether every edit was applied successfully.
    pub success: bool,
    /// Number of edits that were applied.
    pub applied_count: usize,
    /// Number of edits that failed.
    pub failed_count: usize,
    /// Error messages from failed edits.
    pub errors: Vec<String>,
    /// Whether an automatic rollback was performed.
    pub rollback_was_needed: bool,
    /// Wall-clock duration of the commit in milliseconds.
    pub duration_ms: u64,
}

/// Aggregate statistics about editing activity since the editor was created.
#[derive(Debug, Clone, Default)]
pub struct EditorStats {
    /// Total number of transactions created.
    pub total_txns: usize,
    /// Transactions that committed successfully.
    pub committed: usize,
    /// Transactions that were rolled back.
    pub rolled_back: usize,
    /// Total individual edits across all transactions.
    pub total_edits: usize,
    /// Total number of distinct files touched.
    pub total_files_touched: usize,
    /// Mean edits per transaction.
    pub avg_edits_per_txn: f64,
}

// ── BatchEditor ─────────────────────────────────────────────────────────────

/// Atomic multi-file batch editor with transaction semantics and rollback.
///
/// # Example
///
/// ```ignore
/// let mut editor = BatchEditor::new("/path/to/workspace");
/// editor.begin_txn(TxnMetadata { ..Default::default() });
/// editor.add_edit(FileEdit::modify("src/lib.rs", old, new, "fix bug"))?;
/// let result = editor.commit_txn().await?;
/// ```
pub struct BatchEditor {
    /// Root of the workspace being edited.
    workspace: PathBuf,
    /// All transactions ever created (in order).
    transactions: Vec<BatchTransaction>,
    /// Index into `transactions` for the currently active txn, if any.
    active_txn: Option<usize>,
    /// If true, `commit_txn` only validates and previews — no filesystem writes.
    dry_run: bool,
    /// If true, snapshot each file's original content before the first write.
    auto_backup: bool,
    /// Directory where backup snapshots are stored.
    backup_dir: PathBuf,
}

impl BatchEditor {
    /// Create a new batch editor rooted at `workspace`.
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let ws = workspace.into();
        Self {
            backup_dir: ws.join(".carp_backup"),
            workspace: ws,
            transactions: Vec::new(),
            active_txn: None,
            dry_run: false,
            auto_backup: true,
        }
    }

    /// Enable or disable dry-run mode.
    pub fn with_dry_run(mut self, dry: bool) -> Self {
        self.dry_run = dry;
        self
    }

    /// Enable or disable automatic backup before writes.
    pub fn with_auto_backup(mut self, backup: bool) -> Self {
        self.auto_backup = backup;
        self
    }

    /// Set a custom backup directory.
    pub fn with_backup_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.backup_dir = dir.into();
        self
    }

    // ── Transaction lifecycle ───────────────────────────────────────────

    /// Begin a new transaction and return its UUID.
    ///
    /// Any previously active transaction that is still `Pending` is kept
    /// open; you can switch back via [`rollback_by_id`](Self::rollback_by_id)
    /// logic, but typically you should commit or rollback before beginning a new one.
    pub fn begin_txn(&mut self, metadata: TxnMetadata) -> String {
        let id = Uuid::new_v4().to_string();
        let txn = BatchTransaction {
            id: id.clone(),
            edits: Vec::new(),
            status: TxnStatus::Pending,
            applied_edits: Vec::new(),
            failed_edits: Vec::new(),
            created_at: Instant::now(),
            committed_at: None,
            rollback_data: HashMap::new(),
            metadata,
        };
        self.active_txn = Some(self.transactions.len());
        self.transactions.push(txn);
        info!(txn_id = %id, "BatchEditor: transaction started");
        id
    }

    /// Add an edit to the **active** transaction.
    ///
    /// # Errors
    /// Returns an error if there is no active transaction or it is no longer mutable.
    pub fn add_edit(&mut self, edit: FileEdit) -> anyhow::Result<()> {
        let idx = self.active_txn.ok_or_else(|| anyhow::anyhow!("No active transaction. Call begin_txn() first."))?;
        let txn = &mut self.transactions[idx];
        if !txn.is_mutable() {
            anyhow::bail!("Transaction {} is not mutable (status={})", txn.id, txn.status);
        }
        debug!(
            txn_id = %txn.id,
            path = %edit.path,
            edit_type = %edit.edit_type,
            "BatchEditor: edit added"
        );
        txn.edits.push(edit);
        Ok(())
    }

    /// Validate the active transaction without applying anything.
    ///
    /// Checks:
    /// - dependency graph has no cycles
    /// - no two edits target the same file (conflict detection)
    /// - all dependency references point to edits present in this transaction
    ///
    /// Returns a list of warnings (non-fatal) or errors via `Err`.
    pub fn validate_txn(&self) -> anyhow::Result<Vec<String>> {
        let idx = self.active_txn.ok_or_else(|| anyhow::anyhow!("No active transaction"))?;
        let txn = &self.transactions[idx];
        let mut warnings = Vec::new();

        // 1. Dependency cycle detection
        let mut temp = HashSet::new();
        let mut perm = HashSet::new();
        let path_to_idx: HashMap<&str, usize> = txn.edits.iter().enumerate().map(|(i, e)| (e.path.as_str(), i)).collect();

        fn visit(
            idx: usize,
            edits: &[FileEdit],
            path_to_idx: &HashMap<&str, usize>,
            temp: &mut HashSet<usize>,
            perm: &mut HashSet<usize>,
            cycle_path: &mut Vec<usize>,
        ) -> anyhow::Result<()> {
            if perm.contains(&idx) { return Ok(()); }
            if temp.contains(&idx) {
                cycle_path.push(idx);
                anyhow::bail!("Dependency cycle detected involving edit #{} ({})", idx, edits[idx].path);
            }
            temp.insert(idx);
            for dep in &edits[idx].dependencies {
                if let Some(&dep_idx) = path_to_idx.get(dep.as_str()) {
                    visit(dep_idx, edits, path_to_idx, temp, perm, cycle_path)?;
                } else {
                    // Dependency points outside this txn — not necessarily an error,
                    // but worth warning about.
                }
            }
            temp.remove(&idx);
            perm.insert(idx);
            Ok(())
        }

        let mut _cycle_path = Vec::new();
        for i in 0..txn.edits.len() {
            visit(i, &txn.edits, &path_to_idx, &mut temp, &mut perm, &mut _cycle_path)?;
        }

        // 2. Conflict detection — same file targeted by multiple non-Noop edits
        let conflicts = self.check_conflicts(&txn.edits);
        warnings.extend(conflicts);

        // 3. Orphan dependency check
        for (i, edit) in txn.edits.iter().enumerate() {
            for dep in &edit.dependencies {
                if !path_to_idx.contains_key(dep.as_str()) {
                    warnings.push(format!(
                        "Edit #{} ({}) depends on '{}' which is not in this transaction",
                        i, edit.path, dep
                    ));
                }
            }
        }

        Ok(warnings)
    }

    /// Check for conflicting edits where two (or more) non-Noop edits target the same file.
    fn check_conflicts(&self, edits: &[FileEdit]) -> Vec<String> {
        let mut seen: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, edit) in edits.iter().enumerate() {
            if edit.edit_type != EditType::Noop {
                seen.entry(edit.path.as_str()).or_default().push(i);
            }
        }
        let mut conflicts = Vec::new();
        for indices in seen.values() {
            if indices.len() > 1 {
                let indices_str: Vec<String> = indices.iter().map(|i| format!("#{i}")).collect();
                conflicts.push(format!(
                    "Multiple edits target the same file: edits {}",
                    indices_str.join(", ")
                ));
            }
        }
        conflicts
    }

    /// Topologically sort edits in-place based on their dependency ordering.
    ///
    /// Uses Kahn's algorithm (BFS-based). Edits with no dependencies come first.
    fn sort_by_dependency(&self, edits: &mut [FileEdit]) {
        Self::sort_by_dependency_static(edits);
    }

    /// Static version of [`Self::sort_by_dependency`] — usable without `&self`.
    fn sort_by_dependency_static(edits: &mut [FileEdit]) {
        let n = edits.len();
        if n <= 1 {
            return;
        }

        let path_to_idx: HashMap<String, usize> = edits.iter().enumerate().map(|(i, e)| (e.path.clone(), i)).collect();
        let mut in_degree: Vec<usize> = vec![0; n];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

        for (i, edit) in edits.iter().enumerate() {
            for dep in &edit.dependencies {
                if let Some(&dep_idx) = path_to_idx.get(dep) {
                    adj[dep_idx].push(i);
                    in_degree[i] += 1;
                }
            }
        }

        // Collect zero-in-degree nodes
        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut sorted_order = Vec::with_capacity(n);

        while let Some(u) = queue.pop() {
            sorted_order.push(u);
            for &v in &adj[u] {
                in_degree[v] -= 1;
                if in_degree[v] == 0 {
                    queue.push(v);
                }
            }
        }

        // If we couldn't sort all nodes there's a cycle — leave as-is but log
        if sorted_order.len() != n {
            warn!(
                total = n,
                sorted = sorted_order.len(),
                "sort_by_dependency: cycle detected, preserving original order"
            );
            return;
        }

        // Apply the sort by rearranging the slice according to sorted_order
        // We do this by building a new Vec and copying back
        let sorted_edits: Vec<FileEdit> = sorted_order.iter().map(|&i| edits[i].clone()).collect();
        edits.clone_from_slice(&sorted_edits);
    }

    /// Commit the active transaction: validate → sort → apply atomically.
    ///
    /// ## Algorithm
    ///
    /// ```text
    /// 1. Validate (cycle check, conflict check)
    /// 2. Sort edits by dependency order
    /// 3. For each edit:
    ///    a. Backup original content (if auto_backup)
    ///    b. Apply edit to filesystem
    ///    c. Record as applied
    ///    d. On failure → break
    /// 4. If any failures → rollback all applied edits (reverse order)
    /// 5. Return TxnResult
    /// ```
    pub async fn commit_txn(&mut self) -> anyhow::Result<TxnResult> {
        let start = Instant::now();
        let idx = self.active_txn.ok_or_else(|| anyhow::anyhow!("No active transaction"))?;

        if self.dry_run {
            let txn_id = self.transactions[idx].id.clone();
            let edit_count = self.transactions[idx].edits.len();
            info!(txn_id = %txn_id, "BatchEditor: dry-run — skipping commit");
            return Ok(TxnResult {
                success: true,
                applied_count: edit_count,
                failed_count: 0,
                errors: Vec::new(),
                rollback_was_needed: false,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Step 1: Validate
        self.validate_txn()?; // fatal on cycles

        // Step 2: Topological sort (operates on a local copy to avoid borrow conflicts)
        {
            let edits = &mut self.transactions[idx].edits;
            Self::sort_by_dependency_static(edits);
        }
        self.transactions[idx].status = TxnStatus::Applying;

        // Step 3: Apply loop — collect results without holding a mutable txn borrow
        let mut applied_indices = Vec::new();
        let mut failed = Vec::new();
        let mut errors = Vec::new();

        let edit_count = self.transactions[idx].edits.len();
        for edit_idx in 0..edit_count {
            // Take a snapshot of what we need from the edit
            let (edit_copy, needs_backup) = {
                let txn = &self.transactions[idx];
                let e = &txn.edits[edit_idx];
                (e.clone(), self.auto_backup && e.edit_type != EditType::Create && e.edit_type != EditType::Noop)
            };

            // Perform backup if needed (before write)
            if needs_backup {
                let full_path = self.workspace.join(&edit_copy.path);
                if let Ok(existing) = tokio::fs::read_to_string(&full_path).await {
                    self.transactions[idx].rollback_data.insert(edit_copy.path.clone(), existing);
                }
            }

            // Apply the edit
            match Self::apply_single_edit_static(&self.workspace, &edit_copy, edit_idx).await {
                Ok(()) => {
                    applied_indices.push(edit_idx);
                    debug!(edit_idx, path = %edit_copy.path, "BatchEditor: edit applied");
                }
                Err(e) => {
                    let msg = format!("edit #{} ({}): {}", edit_idx, edit_copy.path, e);
                    warn!(error = %msg, "BatchEditor: edit failed during commit");
                    failed.push((edit_idx, msg.clone()));
                    errors.push(msg);
                    break;
                }
            }
        }

        // Step 4: Rollback on partial failure
        let rollback_needed = !failed.is_empty() && !applied_indices.is_empty();
        if rollback_needed {
            let txn_id = self.transactions[idx].id.clone();
            info!(
                txn_id = %txn_id,
                applied = applied_indices.len(),
                failed = failed.len(),
                "BatchEditor: rolling back partial commit"
            );

            // Collect rollback info first (what we need from txn)
            let rollback_plan: Vec<(String, Option<String>, EditType)> = {
                let txn = &self.transactions[idx];
                applied_indices.iter().rev().map(|&ai| {
                    let edit = &txn.edits[ai];
                    (edit.path.clone(), txn.rollback_data.get(&edit.path).cloned(), edit.edit_type)
                }).collect()
            };

            // Now execute rollback without holding txn borrow
            for (path, original, edit_type) in &rollback_plan {
                if let Some(orig) = original {
                    if let Err(e) = Self::restore_backup_static(&self.workspace, path, orig).await {
                        warn!(path = %path, error = %e, "BatchEditor: restore failed during rollback");
                    }
                } else if *edit_type == EditType::Create || *edit_type == EditType::Rename {
                    let full_path = self.workspace.join(path);
                    if full_path.exists() {
                        let _ = tokio::fs::remove_file(&full_path).await;
                    }
                }
            }

            self.transactions[idx].status = TxnStatus::Partial;
            self.transactions[idx].failed_edits = failed.clone();
            self.transactions[idx].applied_edits.clear();
        } else if failed.is_empty() {
            // Full success
            self.transactions[idx].status = TxnStatus::Committed;
            self.transactions[idx].committed_at = Some(Instant::now());
            self.transactions[idx].applied_edits = applied_indices.clone();
            let txn_id = &self.transactions[idx].id;
            info!(txn_id = %txn_id, count = applied_indices.len(), "BatchEditor: transaction committed");
        } else {
            // All edits failed
            self.transactions[idx].status = TxnStatus::Failed;
            self.transactions[idx].failed_edits = failed.clone();
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(TxnResult {
            success: failed.is_empty(),
            applied_count: applied_indices.len(),
            failed_count: failed.len(),
            errors,
            rollback_was_needed: rollback_needed,
            duration_ms,
        })
    }

    /// Rollback the **active** transaction: restore all files to their pre-txn state.
    ///
    /// After rollback the transaction status becomes `RolledBack`. You can begin
    /// a new transaction immediately.
    pub async fn rollback_txn(&mut self) -> anyhow::Result<()> {
        let idx = self.active_txn.ok_or_else(|| anyhow::anyhow!("No active transaction to rollback"))?;
        self.rollback_txn_at(idx).await
    }

    /// Rollback a specific transaction by its UUID.
    ///
    /// Returns `Ok(true)` if the transaction was found and rolled back,
    /// `Ok(false)` if no transaction with that ID exists.
    pub async fn rollback_by_id(&mut self, id: &str) -> anyhow::Result<bool> {
        let pos = self.transactions.iter().position(|t| t.id == id);
        match pos {
            Some(idx) => {
                self.rollback_txn_at(idx).await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Internal: rollback the transaction at index `idx`.
    async fn rollback_txn_at(&mut self, idx: usize) -> anyhow::Result<()> {
        let txn = &self.transactions[idx];
        info!(txn_id = %txn.id, status = %txn.status, "BatchEditor: rolling back");

        // Restore each backed-up file
        let mut restore_errors = 0;
        for (path, original) in &txn.rollback_data {
            if let Err(e) = self.restore_backup(path, original).await {
                warn!(path, error = %e, "BatchEditor: restore failed during rollback");
                restore_errors += 1;
            }
        }

        // For files that were created in this txn (no backup because they didn't exist before),
        // remove them if they still exist
        for edit in &txn.edits {
            if edit.edit_type == EditType::Create && !txn.rollback_data.contains_key(&edit.path) {
                let full_path = self.workspace.join(&edit.path);
                if full_path.exists() {
                    if let Err(e) = tokio::fs::remove_file(&full_path).await {
                        warn!(path = %edit.path, error = %e, "BatchEditor: failed to delete created file during rollback");
                        restore_errors += 1;
                    }
                }
            }
        }

        // Update status
        self.transactions[idx].status = TxnStatus::RolledBack;
        if self.active_txn == Some(idx) {
            self.active_txn = None;
        }

        if restore_errors > 0 {
            anyhow::bail!("Rollback completed with {} restore errors", restore_errors);
        }
        Ok(())
    }

    /// List all transactions (most recent last).
    pub fn list_transactions(&self) -> Vec<&BatchTransaction> {
        self.transactions.iter().collect()
    }

    /// Generate a unified-diff preview of what the active transaction would change.
    ///
    /// Returns a human-readable string showing every edit as a diff hunk.
    pub fn preview_txn(&self) -> String {
        let idx = match self.active_txn {
            Some(i) => i,
            None => return "(no active transaction)".to_string(),
        };
        let txn = &self.transactions[idx];

        if txn.edits.is_empty() {
            return "(empty transaction — no edits)".to_string();
        }

        let mut out = String::new();
        out.push_str(&format!("Transaction: {}\n", txn.id));
        out.push_str(&format!("Status:     {}\n", txn.status));
        out.push_str(&format!("Edits:      {}\n\n", txn.edits.len()));

        for (i, edit) in txn.edits.iter().enumerate() {
            out.push_str(&format!("[{}] {}  {}  (confidence: {:.2})\n",
                i, edit.edit_type, edit.path, edit.confidence));
            if !edit.description.is_empty() {
                out.push_str(&format!("    {}\n", edit.description));
            }
            if !edit.dependencies.is_empty() {
                out.push_str(&format!("    depends on: {}\n", edit.dependencies.join(", ")));
            }

            // Generate unified diff between old and new content
            let old = edit.old_content.as_deref().unwrap_or("");
            let new = edit.new_content.as_deref().unwrap_or("");
            let diff = TextDiff::from_lines(old, new);

            let has_changes = diff.iter_all_changes().any(|c| c.tag() != ChangeTag::Equal);
            if has_changes {
                out.push_str("    --- diff:\n");
                for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
                    for line in format!("{}", hunk).lines() {
                        out.push_str("      ");
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            } else if edit.edit_type == EditType::Create {
                out.push_str("    +++ new file\n");
                for line in new.lines().take(10) {
                    out.push_str("      +");
                    out.push_str(line);
                    out.push('\n');
                }
                if new.lines().count() > 10 {
                    out.push_str("      ... (truncated)\n");
                }
            } else if edit.edit_type == EditType::Delete {
                out.push_str("    --- deleted file\n");
                for line in old.lines().take(10) {
                    out.push_str("      -");
                    out.push_str(line);
                    out.push('\n');
                }
                if old.lines().count() > 10 {
                    out.push_str("      ... (truncated)\n");
                }
            }
            out.push('\n');
        }

        out
    }

    /// Compute aggregate statistics about all editing activity.
    pub fn stats(&self) -> EditorStats {
        let total_txns = self.transactions.len();
        let committed = self.transactions.iter().filter(|t| t.status == TxnStatus::Committed).count();
        let rolled_back = self.transactions.iter().filter(|t| t.status == TxnStatus::RolledBack).count();
        let total_edits: usize = self.transactions.iter().map(|t| t.edits.len()).sum();

        let mut files: HashSet<String> = HashSet::new();
        for txn in &self.transactions {
            for edit in &txn.edits {
                files.insert(edit.path.clone());
            }
        }

        EditorStats {
            total_txns,
            committed,
            rolled_back,
            total_edits,
            total_files_touched: files.len(),
            avg_edits_per_txn: if total_txns > 0 { total_edits as f64 / total_txns as f64 } else { 0.0 },
        }
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Apply a single edit to the filesystem.
    ///
    /// Before writing, reads and stores the original content (for rollback)
    /// when `auto_backup` is enabled.
    async fn apply_single_edit(&mut self, edit: &FileEdit, idx: usize) -> anyhow::Result<()> {
        let full_path = self.workspace.join(&edit.path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        match edit.edit_type {
            EditType::Modify | EditType::Rename => {
                // Backup original content before overwriting
                if self.auto_backup {
                    if let Ok(existing) = tokio::fs::read_to_string(&full_path).await {
                        if let Some(active) = self.active_txn {
                            self.transactions[active].rollback_data.insert(edit.path.clone(), existing);
                        }
                    }
                }

                let content = edit.new_content.as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Modify/Rename edit #{} ({}) has no new_content", idx, edit.path))?;
                tokio::fs::write(&full_path, content.as_bytes()).await?;
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: file written (modify)");
            }

            EditType::Create => {
                // No backup needed for truly new files (they don't exist yet)
                let content = edit.new_content.as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Create edit #{} ({}) has no new_content", idx, edit.path))?;
                tokio::fs::write(&full_path, content.as_bytes()).await?;
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: file created");
            }

            EditType::Delete => {
                // Backup before deleting
                if self.auto_backup {
                    if let Ok(existing) = tokio::fs::read_to_string(&full_path).await {
                        if let Some(active) = self.active_txn {
                            self.transactions[active].rollback_data.insert(edit.path.clone(), existing);
                        }
                    }
                }

                tokio::fs::remove_file(&full_path).await?;
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: file deleted");
            }

            EditType::Noop => {
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: noop — skipped");
            }
        }

        Ok(())
    }

    /// Restore a single file to its original content during rollback.
    async fn restore_backup(&self, path: &str, original: &str) -> anyhow::Result<()> {
        Self::restore_backup_static(&self.workspace, path, original).await
    }

    /// Static version of [`Self::apply_single_edit`] — takes workspace explicitly.
    async fn apply_single_edit_static(
        workspace: &PathBuf,
        edit: &FileEdit,
        idx: usize,
    ) -> anyhow::Result<()> {
        let full_path = workspace.join(&edit.path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        match edit.edit_type {
            EditType::Modify | EditType::Rename => {
                let content = edit.new_content.as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Modify/Rename edit #{} ({}) has no new_content", idx, edit.path))?;
                tokio::fs::write(&full_path, content.as_bytes()).await?;
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: file written (modify)");
            }

            EditType::Create => {
                let content = edit.new_content.as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Create edit #{} ({}) has no new_content", idx, edit.path))?;
                tokio::fs::write(&full_path, content.as_bytes()).await?;
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: file created");
            }

            EditType::Delete => {
                tokio::fs::remove_file(&full_path).await?;
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: file deleted");
            }

            EditType::Noop => {
                debug!(path = %edit.path, edit_idx = idx, "BatchEditor: noop — skipped");
            }
        }

        Ok(())
    }

    /// Static version of [`Self::restore_backup`] — takes workspace explicitly.
    async fn restore_backup_static(
        workspace: &PathBuf,
        path: &str,
        original: &str,
    ) -> anyhow::Result<()> {
        let full_path = workspace.join(path);

        // Ensure parent directory exists (in case a delete removed a directory)
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&full_path, original.as_bytes()).await?;
        debug!(path, "BatchEditor: restored from backup");
        Ok(())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a temporary workspace and return `(editor, temp_dir)`.
    async fn make_editor() -> (BatchEditor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let editor = BatchEditor::new(dir.path())
            .with_auto_backup(true)
            .with_dry_run(false);
        (editor, dir)
    }

    #[tokio::test]
    async fn test_simple_modify_txn() {
        let (mut editor, dir) = make_editor().await;

        // Write an initial file
        let p = dir.path().join("hello.rs");
        tokio::fs::write(&p, "fn old() {}\n").await.unwrap();

        let meta = TxnMetadata {
            task_description: "simple test".into(),
            ..Default::default()
        };

        let txn_id = editor.begin_txn(meta);
        editor.add_edit(FileEdit::modify(
            "hello.rs",
            "fn old() {}\n".into(),
            "fn new() {}\n".into(),
            "rename old to new",
        )).unwrap();

        let result = editor.commit_txn().await.unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 1);
        assert_eq!(result.failed_count, 0);
        assert!(!result.rollback_was_needed);

        // Verify file content changed
        let content = tokio::fs::read_to_string(p).await.unwrap();
        assert_eq!(content, "fn new() {}\n");

        // Verify transaction status
        let txns = editor.list_transactions();
        assert_eq!(txns[0].status, TxnStatus::Committed);
        assert_eq!(txns[0].id, txn_id);
    }

    #[tokio::test]
    async fn test_create_and_delete_txn() {
        let (mut editor, dir) = make_editor().await;

        // Write initial file to later delete
        let p = dir.path().join("temp.txt");
        tokio::fs::write(&p, "temporary content\n").await.unwrap();

        let meta = TxnMetadata::default();
        editor.begin_txn(meta);

        // Add a create and a delete
        editor.add_edit(FileEdit::create(
            "new_file.rs",
            "// new file\n".into(),
            "create new_file.rs",
        )).unwrap();

        editor.add_edit(FileEdit::delete(
            "temp.txt",
            "temporary content\n".into(),
            "remove temp.txt",
        )).unwrap();

        let result = editor.commit_txn().await.unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 2);

        // Verify new file exists
        assert!(dir.path().join("new_file.rs").exists());
        // Verify deleted file is gone
        assert!(!dir.path().join("temp.txt").exists());
    }

    #[tokio::test]
    async fn test_rollback_on_failure() {
        let (mut editor, dir) = make_editor().await;

        // Write initial file
        let p1 = dir.path().join("file_a.rs");
        tokio::fs::write(&p1, "original A\n").await.unwrap();
        let p2 = dir.path().join("file_b.rs");
        tokio::fs::write(&p2, "original B\n").await.unwrap();

        let meta = TxnMetadata::default();
        editor.begin_txn(meta);

        // First edit: valid modification
        editor.add_edit(FileEdit::modify(
            "file_a.rs",
            "original A\n".into(),
            "modified A\n".into(),
            "change A",
        )).unwrap();

        // Second edit: targets a nonexistent sub-directory (will fail on write)
        // We simulate failure by using a path that can't be created easily
        editor.add_edit(FileEdit {
            path: "nonexistent_dir\0illegal/file.rs".into(), // null byte in path → will fail
            old_content: Some("old".into()),
            new_content: Some("new".into()),
            edit_type: EditType::Modify,
            description: "this will fail".into(),
            dependencies: Vec::new(),
            confidence: 1.0,
        }).unwrap();

        let result = editor.commit_txn().await.unwrap();
        assert!(!result.success);
        assert!(result.rollback_was_needed);
        assert_eq!(result.failed_count, 1);

        // Verify file_a.rs was rolled back to original
        let content = tokio::fs::read_to_string(p1).await.unwrap();
        assert_eq!(content, "original A\n");

        // Transaction should be Partial
        let txns = editor.list_transactions();
        assert_eq!(txns[0].status, TxnStatus::Partial);
    }

    #[tokio::test]
    async fn test_dependency_ordering() {
        let (mut editor, dir) = make_editor().await;

        let meta = TxnMetadata::default();
        editor.begin_txn(meta);

        // Add edits with dependencies: C depends on B, B depends on A
        editor.add_edit(FileEdit {
            path: "c.rs".into(),
            old_content: Some("old_c".into()),
            new_content: Some("new_c".into()),
            edit_type: EditType::Modify,
            description: "edit C".into(),
            dependencies: vec!["b.rs".into()],
            confidence: 1.0,
        }).unwrap();

        editor.add_edit(FileEdit {
            path: "a.rs".into(),
            old_content: Some("old_a".into()),
            new_content: Some("new_a".into()),
            edit_type: EditType::Modify,
            description: "edit A".into(),
            dependencies: Vec::new(),
            confidence: 1.0,
        }).unwrap();

        editor.add_edit(FileEdit {
            path: "b.rs".into(),
            old_content: Some("old_b".into()),
            new_content: Some("new_b".into()),
            edit_type: EditType::Modify,
            description: "edit B".into(),
            dependencies: vec!["a.rs".into()],
            confidence: 1.0,
        }).unwrap();

        // Create the actual files first
        for name in ["a.rs", "b.rs", "c.rs"] {
            tokio::fs::write(dir.path().join(name), format!("old_{name}\n")).await.unwrap();
        }

        let result = editor.commit_txn().await.unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 3);

        // After commit, edits should have been applied in dependency order
        let txns = editor.list_transactions();
        let applied_paths: Vec<&str> = txns[0].applied_edits.iter()
            .map(|&i| txns[0].edits[i].path.as_str())
            .collect();
        // a.rs should come before b.rs, which comes before c.rs
        let pos_a = applied_paths.iter().position(|&p| p == "a.rs").unwrap();
        let pos_b = applied_paths.iter().position(|&p| p == "b.rs").unwrap();
        let pos_c = applied_paths.iter().position(|&p| p == "c.rs").unwrap();
        assert!(pos_a < pos_b, "a.rs must be applied before b.rs");
        assert!(pos_b < pos_c, "b.rs must be applied before c.rs");
    }

    #[test]
    fn test_conflict_detection() {
        let editor = BatchEditor::new("/tmp/workspace");

        let edits = vec![
            FileEdit::modify("lib.rs", "old".into(), "new1".into(), "first change"),
            FileEdit::modify("lib.rs", "old".into(), "new2".into(), "second change"),
            FileEdit::create("main.rs", "content".into(), "new main"),
        ];

        let conflicts = editor.check_conflicts(&edits);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("lib.rs"));
    }

    #[tokio::test]
    async fn test_dry_run_no_changes() {
        let (_editor, dir) = make_editor().await;

        let p = dir.path().join("unchanged.rs");
        tokio::fs::write(&p, "original\n").await.unwrap();

        let mut dry_editor = BatchEditor::new(dir.path()).with_dry_run(true);
        dry_editor.begin_txn(TxnMetadata::default());
        dry_editor.add_edit(FileEdit::modify(
            "unchanged.rs",
            "original\n".into(),
            "modified\n".into(),
            "would change",
        )).unwrap();

        let result = dry_editor.commit_txn().await.unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 1); // counts the edit even though not written

        // File must NOT have changed
        let content = tokio::fs::read_to_string(p).await.unwrap();
        assert_eq!(content, "original\n");
    }

    #[test]
    fn test_preview_diff() {
        let mut editor = BatchEditor::new("/tmp/test_workspace");
        editor.begin_txn(TxnMetadata::default());

        editor.add_edit(FileEdit::modify(
            "src/lib.rs",
            "fn hello() {\n    println!(\"old\");\n}\n".into(),
            "fn hello() {\n    println!(\"new\");\n}\n".into(),
            "update print message",
        )).unwrap();

        let preview = editor.preview_txn();
        assert!(preview.contains("Transaction:"));
        assert!(preview.contains("MODIFY"));
        assert!(preview.contains("src/lib.rs"));
        assert!(preview.contains("update print message"));
        // Diff should show both old and new
        assert!(preview.contains("old") || preview.contains("-"));
        assert!(preview.contains("new") || preview.contains("+"));
    }

    #[tokio::test]
    async fn test_multi_file_rename() {
        let (mut editor, dir) = make_editor().await;

        // Setup: create original files
        let foo_content = "struct Foo { x: i32 }\nfn use_foo() { let _ = Foo { x: 1 }; }\n";
        tokio::fs::write(dir.path().join("foo.rs"), foo_content).await.unwrap();
        tokio::fs::write(dir.path().join("main.rs"), "mod foo;\nfn main() { foo::use_foo(); }\n").await.unwrap();

        let meta = TxnMetadata {
            task_description: "rename Foo to Bar across codebase".into(),
            estimated_risk: RiskLevel::High,
            affected_files: 2,
            ..Default::default()
        };
        editor.begin_txn(meta);

        // Use the rename helper which produces (delete_old, create_new) pair
        let (del, crt) = FileEdit::rename(
            "foo.rs",
            "bar.rs",
            "struct Bar { x: i32 }\nfn use_bar() { let _ = Bar { x: 1 }; }\n".into(),
            "rename Foo→Bar",
        );
        editor.add_edit(del).unwrap();
        editor.add_edit(crt).unwrap();

        // Also update the reference in main.rs
        editor.add_edit(FileEdit::modify(
            "main.rs",
            "mod foo;\nfn main() { foo::use_foo(); }\n".into(),
            "mod bar;\nfn main() { bar::use_bar(); }\n".into(),
            "update main.rs references",
        )).unwrap();

        let result = editor.commit_txn().await.unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 3);

        // Verify: foo.rs gone, bar.rs exists, main.rs updated
        assert!(!dir.path().join("foo.rs").exists());
        assert!(dir.path().join("bar.rs").exists());
        let main_content = tokio::fs::read_to_string(dir.path().join("main.rs")).await.unwrap();
        assert!(main_content.contains("mod bar"));
        assert!(main_content.contains("bar::use_bar"));

        // Stats should reflect the activity
        let stats = editor.stats();
        assert_eq!(stats.total_txns, 1);
        assert_eq!(stats.committed, 1);
        assert_eq!(stats.total_edits, 3);
    }

    #[tokio::test]
    async fn test_rollback_by_id() {
        let (mut editor, dir) = make_editor().await;

        let p = dir.path().join("rollback_test.rs");
        tokio::fs::write(&p, "before txn\n").await.unwrap();

        let id = editor.begin_txn(TxnMetadata::default());
        editor.add_edit(FileEdit::modify(
            "rollback_test.rs",
            "before txn\n".into(),
            "after txn\n".into(),
            "test change",
        )).unwrap();

        editor.commit_txn().await.unwrap();

        // Content should be changed
        assert_eq!(tokio::fs::read_to_string(&p).await.unwrap(), "after txn\n");

        // Rollback by ID
        let found = editor.rollback_by_id(&id).await.unwrap();
        assert!(found);

        // Content should be restored
        assert_eq!(tokio::fs::read_to_string(&p).await.unwrap(), "before txn\n");

        // Unknown ID returns false
        assert!(!editor.rollback_by_id("nonexistent-id").await.unwrap());
    }

    #[tokio::test]
    async fn test_validate_detects_cycles() {
        let mut editor = BatchEditor::new("/tmp/workspace");
        editor.begin_txn(TxnMetadata::default());

        // A depends on B, B depends on A → cycle
        let mut edit_a = FileEdit::modify("a.rs", "old".into(), "new".into(), "A");
        edit_a.dependencies = vec!["b.rs".into()];
        let mut edit_b = FileEdit::modify("b.rs", "old".into(), "new".into(), "B");
        edit_b.dependencies = vec!["a.rs".into()];

        editor.add_edit(edit_a).unwrap();
        editor.add_edit(edit_b).unwrap();

        let result = editor.validate_txn();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle"));
    }

    #[tokio::test]
    async fn test_stats_aggregation() {
        let (mut editor, dir) = make_editor().await;

        // First transaction
        editor.begin_txn(TxnMetadata::default());
        editor.add_edit(FileEdit::create("x.rs", "// x".into(), "create x")).unwrap();
        editor.commit_txn().await.unwrap();

        // Second transaction (rolled back)
        tokio::fs::write(dir.path().join("y.rs"), "old y\n").await.unwrap();
        editor.begin_txn(TxnMetadata::default());
        editor.add_edit(FileEdit::modify("y.rs", "old y\n".into(), "new y\n".into(), "change y")).unwrap();
        editor.commit_txn().await.unwrap(); // succeeds
        editor.rollback_txn().await.unwrap(); // then rollback

        let s = editor.stats();
        assert_eq!(s.total_txns, 2);
        assert_eq!(s.committed, 1);
        assert_eq!(s.rolled_back, 1);
        assert_eq!(s.total_edits, 2);
        assert!((s.avg_edits_per_txn - 1.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_no_active_txn_errors() {
        let mut editor = BatchEditor::new("/tmp/workspace");

        assert!(editor.add_edit(FileEdit::create("x.rs", "".into(), "")).is_err());
        assert!(editor.validate_txn().is_err());
    }

    #[test]
    fn test_edit_type_display() {
        assert_eq!(EditType::Modify.to_string(), "MODIFY");
        assert_eq!(EditType::Create.to_string(), "CREATE");
        assert_eq!(EditType::Delete.to_string(), "DELETE");
        assert_eq!(EditType::Rename.to_string(), "RENAME");
        assert_eq!(EditType::Noop.to_string(), "NOOP");
    }

    #[test]
    fn test_txn_status_display() {
        assert_eq!(TxnStatus::Pending.to_string(), "PENDING");
        assert_eq!(TxnStatus::Committed.to_string(), "COMMITTED");
        assert_eq!(TxnStatus::RolledBack.to_string(), "ROLLED_BACK");
        assert_eq!(TxnStatus::Partial.to_string(), "PARTIAL");
        assert_eq!(TxnStatus::Failed.to_string(), "FAILED");
    }

    #[test]
    fn test_risk_level_ord() {
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[tokio::test]
    async fn test_empty_transaction_commit() {
        let (mut editor, _dir) = make_editor().await;
        editor.begin_txn(TxnMetadata::default());

        let result = editor.commit_txn().await.unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 0);
    }

    #[tokio::test]
    async fn test_noop_edit_skipped() {
        let (mut editor, dir) = make_editor().await;
        let p = dir.path().join("noop_test.rs");
        tokio::fs::write(&p, "unchanged\n").await.unwrap();

        editor.begin_txn(TxnMetadata::default());
        editor.add_edit(FileEdit {
            path: "noop_test.rs".into(),
            old_content: Some("unchanged\n".into()),
            new_content: Some("unchanged\n".into()),
            edit_type: EditType::Noop,
            description: "should skip".into(),
            dependencies: Vec::new(),
            confidence: 1.0,
        }).unwrap();

        let result = editor.commit_txn().await.unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 1);

        // File unchanged
        assert_eq!(tokio::fs::read_to_string(p).await.unwrap(), "unchanged\n");
    }
}
