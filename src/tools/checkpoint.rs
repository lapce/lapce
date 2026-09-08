//! Checkpoint system — SHA256 file snapshots for safe rollback.
//!
//! Ported from CarpAI's `src/checkpoint.rs`.
//! Before any destructive edit, the file is checkpointed. If the edit
//! produces bad results, the checkpoint can be restored.
//!
//! ## Usage
//!
//! ```no_run
//! use deepseek_carp::tools::checkpoint::CheckpointManager;
//!
//! let mut mgr = CheckpointManager::new(100); // Max 100 snapshots
//! mgr.save("src/main.rs").expect("unwrap failed: checkpoint.rs:13");          // Before edit
//! // ... perform edit ...
//! mgr.restore("src/main.rs").expect("unwrap failed: checkpoint.rs:15");        // Undo edit
//! ```

use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A snapshot of a file's state at a point in time.
#[derive(Debug, Clone)]
struct Snapshot {
    /// SHA256 hash of file content.
    hash: String,
    /// Backup file path on disk.
    backup_path: PathBuf,
    /// Original file path.
    original_path: PathBuf,
    /// When the snapshot was created.
    timestamp: chrono::DateTime<chrono::Utc>,
}

/// Manages file checkpoints for safe editing and rollback.
pub struct CheckpointManager {
    /// Map of original file path → list of snapshots.
    snapshots: HashMap<PathBuf, Vec<Snapshot>>,
    /// Maximum total snapshots (oldest removed when exceeded).
    max_snapshots: usize,
    /// Directory for storing backup files.
    backup_dir: PathBuf,
    /// Total snapshots tracked (across all files).
    total_count: usize,
}

impl CheckpointManager {
    /// Create a new manager with the given capacity.
    pub fn new(max_snapshots: usize) -> Self {
        let backup_dir = crate::config::paths::config_file().parent().unwrap_or(std::path::Path::new(".")).join("checkpoints");
        std::fs::create_dir_all(&backup_dir).ok();

        Self {
            snapshots: HashMap::new(),
            max_snapshots,
            backup_dir,
            total_count: 0,
        }
    }

    /// Save a checkpoint of the given file.
    /// Returns the SHA256 hash of the file content.
    pub fn save<P: AsRef<Path>>(&mut self, file_path: P) -> std::io::Result<String> {
        let path = file_path.as_ref();
        if !path.exists() {
            return Ok(String::new()); // File doesn't exist yet, nothing to backup
        }

        let content = std::fs::read(path)?;
        let hash = Self::hash_content(&content);

        // Check if we already have this exact snapshot
        if let Some(snaps) = self.snapshots.get(path) {
            if snaps.iter().any(|s| s.hash == hash) {
                return Ok(hash); // Already checkpointed
            }
        }

        // Evict oldest if at capacity
        while self.total_count >= self.max_snapshots {
            self.evict_oldest();
        }

        let backup_name = format!("{}_{}.bak", Self::sanitize_filename(path), &hash[..8]);
        let backup_path = self.backup_dir.join(&backup_name);
        std::fs::write(&backup_path, &content)?;

        let snapshot = Snapshot {
            hash: hash.clone(),
            backup_path,
            original_path: path.to_path_buf(),
            timestamp: chrono::Utc::now(),
        };

        self.snapshots.entry(path.to_path_buf()).or_default().push(snapshot.clone());
        let _summary = snapshot.summary();
        self.total_count += 1;

        tracing::debug!(file=%path.display(), hash=%hash, "Checkpoint saved");
        Ok(hash)
    }

    /// Restore the most recent checkpoint of a file.
    pub fn restore<P: AsRef<Path>>(&mut self, file_path: P) -> std::io::Result<bool> {
        let path = file_path.as_ref();
        let snapshots = match self.snapshots.get_mut(path) {
            Some(s) => s,
            None => return Ok(false),
        };

        let snapshot = match snapshots.pop() {
            Some(s) => s,
            None => return Ok(false),
        };

        let content = std::fs::read(&snapshot.backup_path)?;
        std::fs::write(path, &content)?;
        std::fs::remove_file(&snapshot.backup_path).ok();
        self.total_count -= 1;

        tracing::info!(file=%path.display(), hash=%snapshot.hash, "Checkpoint restored");
        Ok(true)
    }

    /// Rollback all files to their last checkpoint.
    pub fn rollback_all(&mut self) -> usize {
        let files: Vec<PathBuf> = self.snapshots.keys().cloned().collect();
        let mut count = 0;

        for file in files {
            if let Ok(true) = self.restore(&file) {
                count += 1;
            }
        }
        count
    }

    /// Verify that a file matches its last checkpoint.
    pub fn verify<P: AsRef<Path>>(&self, file_path: P) -> bool {
        let path = file_path.as_ref();
        let snaps = match self.snapshots.get(path) {
            Some(s) => s,
            None => return true, // No checkpoint to verify
        };

        let content = match std::fs::read(path) {
            Ok(c) => c,
            Err(_) => return false,
        };

        let current_hash = Self::hash_content(&content);
        snaps.last().is_none_or(|s| s.hash == current_hash)
    }

    fn hash_content(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("{:x}", hasher.finalize())
    }

    fn sanitize_filename(path: &Path) -> String {
        path.to_string_lossy()
            .replace(['/', '\\', ':'], "_")
            .replace("..", "_")
    }

    fn evict_oldest(&mut self) {
        let mut oldest_key: Option<PathBuf> = None;
        for (path, snaps) in &self.snapshots {
            if !snaps.is_empty() && oldest_key.is_none() {
                oldest_key = Some(path.clone());
            }
        }
        if let Some(path) = oldest_key {
            if let Some(snaps) = self.snapshots.get_mut(&path) {
                if !snaps.is_empty() {
                    let old = snaps.remove(0);
                    std::fs::remove_file(&old.backup_path).ok();
                    self.total_count -= 1;
                }
            }
        }
    }
}

impl Snapshot {
    /// Get the original file path this snapshot was taken from.
    pub fn original_path(&self) -> &PathBuf {
        &self.original_path
    }

    /// Get the timestamp when this snapshot was created.
    pub fn timestamp(&self) -> &chrono::DateTime<chrono::Utc> {
        &self.timestamp
    }

    /// Return a human-readable summary of this snapshot.
    pub fn summary(&self) -> String {
        format!("Snapshot of {} at {}", self.original_path().display(), self.timestamp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_restore() {
        let temp_dir = std::env::temp_dir().join("carp_checkpoint_test");
        std::fs::create_dir_all(&temp_dir).ok();

        let file_path = temp_dir.join("test.txt");
        std::fs::write(&file_path, "original content").unwrap();

        let mut mgr = CheckpointManager::new(10);
        let hash = mgr.save(&file_path).unwrap();
        assert!(!hash.is_empty());

        // Modify file
        std::fs::write(&file_path, "modified content").unwrap();

        // Restore
        assert!(mgr.restore(&file_path).unwrap());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "original content");

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_verify() {
        let temp_dir = std::env::temp_dir().join("carp_verify_test");
        std::fs::create_dir_all(&temp_dir).ok();
        let file = temp_dir.join("v.txt");
        std::fs::write(&file, "data").unwrap();

        let mut mgr = CheckpointManager::new(10);
        mgr.save(&file).unwrap();
        assert!(mgr.verify(&file));

        std::fs::write(&file, "changed").unwrap();
        assert!(!mgr.verify(&file));

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
