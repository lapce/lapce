//! LoopEngine run persistence — stores round results, diffs, and history.
//!
//! ## Tables
//!
//! - `loop_runs`: each `run_summary()` call (target, mode, passed, timing)
//! - `loop_rounds`: each round within a run (verdict, phase times)
//! - `diff_records`: file edits applied during a run (original ↔ modified)

use super::db::Database;
use crate::r#loop::{LoopPhase, LoopSummary, LoopVerdict, RoundResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// A persisted loop run (one invocation of `LoopEngine::run_summary`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunRecord {
    pub id: String,
    pub target: String,
    pub mode: String,
    pub max_rounds: u32,
    pub total_rounds: u32,
    pub passed: bool,
    pub total_time_ms: u64,
    pub final_verdict: Option<String>,
    pub started_at: String,
    pub finished_at: String,
}

/// A persisted single round within a loop run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRoundRecord {
    pub id: String,
    pub run_id: String,
    pub round_number: u32,
    pub verdict: String,
    pub phase_times_ms: Vec<(String, u64)>,
    pub total_time_ms: u64,
    pub details: String,
    pub created_at: String,
}

/// A file diff recorded during a loop round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffRecord {
    pub id: String,
    pub run_id: String,
    pub round_number: u32,
    pub file_path: String,
    pub original_hash: String,
    pub original: String,
    pub modified: String,
    pub description: String,
    pub status: String,
    pub created_at: String,
}

/// High-level query result — returns last N runs with summaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunHistory {
    pub runs: Vec<LoopRunRecord>,
    pub total_runs: usize,
}

/// CRUD operations for LoopEngine runs.
pub struct LoopStore {
    db: Database,
}

impl LoopStore {
    /// Open the store (creates DB and runs migrations).
    pub fn open() -> rusqlite::Result<Self> {
        Ok(Self { db: Database::open()? })
    }

    // ── Write operations ──

    /// Save a complete loop run (summary + rounds).
    /// Returns the run ID.
    pub fn save_run(
        &self,
        target: &str,
        mode: &str,
        max_rounds: u32,
        summary: &LoopSummary,
    ) -> rusqlite::Result<String> {
        let run_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let started_at = &now;
        let finished_at = &now;

        // Insert loop_run
        let final_verdict_json = summary
            .final_verdict
            .as_ref()
            .map(|v| format!("{:?}", v));

        self.db.conn().execute(
            "INSERT INTO loop_runs (id, target, mode, max_rounds, total_rounds, passed,
             total_time_ms, final_verdict, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                run_id,
                target,
                mode,
                max_rounds,
                summary.total_rounds,
                if summary.passed { 1i64 } else { 0i64 },
                summary.total_time_ms as i64,
                final_verdict_json,
                started_at,
                finished_at,
            ],
        )?;

        // Insert each round
        for round_result in &summary.results {
            self.save_round(&run_id, round_result)?;
        }

        Ok(run_id)
    }

    /// Save a single round result as part of a run.
    fn save_round(&self, run_id: &str, round: &RoundResult) -> rusqlite::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let verdict_str = match &round.verdict {
            LoopVerdict::Passed => "Passed".into(),
            LoopVerdict::Failed { reason } => format!("Failed: {}", reason),
            LoopVerdict::Aborted { reason } => format!("Aborted: {}", reason),
        };

        // Serialize phase times as JSON array
        let phase_times_json: String = {
            let pairs: Vec<serde_json::Value> = round
                .phase_times_ms
                .iter()
                .map(|(phase, ms)| {
                    serde_json::json!({
                        "phase": format!("{:?}", phase),
                        "ms": ms,
                    })
                })
                .collect();
            serde_json::to_string(&pairs).unwrap_or_else(|_| "[]".into())
        };

        self.db.conn().execute(
            "INSERT INTO loop_rounds (id, run_id, round_number, verdict, phase_times,
             total_time_ms, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                run_id,
                round.round,
                verdict_str,
                phase_times_json,
                round.total_time_ms as i64,
                round.details,
                now,
            ],
        )?;

        Ok(())
    }

    /// Record a diff/edit action for a specific round.
    pub fn save_diff(
        &self,
        run_id: &str,
        round_number: u32,
        file_path: &str,
        original: &str,
        modified: &str,
        description: &str,
        status: &str,
    ) -> rusqlite::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let original_hash = blake3_hash(original);

        self.db.conn().execute(
            "INSERT INTO diff_records (id, run_id, round_number, file_path, original_hash,
             original, modified, description, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                run_id,
                round_number,
                file_path,
                original_hash,
                original,
                modified,
                description,
                status,
                now,
            ],
        )?;

        Ok(id)
    }

    // ── Read operations ──

    /// List recent loop runs (without rounds/diffs).
    pub fn list_runs(&self, limit: usize) -> rusqlite::Result<Vec<LoopRunRecord>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, target, mode, max_rounds, total_rounds, passed,
                    total_time_ms, final_verdict, started_at, finished_at
             FROM loop_runs
             ORDER BY finished_at DESC
             LIMIT ?1"
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(LoopRunRecord {
                id: row.get(0)?,
                target: row.get(1)?,
                mode: row.get(2)?,
                max_rounds: row.get::<_, i64>(3)? as u32,
                total_rounds: row.get::<_, i64>(4)? as u32,
                passed: row.get::<_, i64>(5)? != 0,
                total_time_ms: row.get::<_, i64>(6)? as u64,
                final_verdict: row.get(7)?,
                started_at: row.get(8)?,
                finished_at: row.get(9)?,
            })
        })?;

        rows.collect()
    }

    /// Load a single run with its rounds.
    pub fn load_run(&self, run_id: &str) -> rusqlite::Result<Option<LoopRunRecord>> {
        let result = self.db.conn().query_row(
            "SELECT id, target, mode, max_rounds, total_rounds, passed,
                    total_time_ms, final_verdict, started_at, finished_at
             FROM loop_runs WHERE id = ?1",
            params![run_id],
            |row| {
                Ok(LoopRunRecord {
                    id: row.get(0)?,
                    target: row.get(1)?,
                    mode: row.get(2)?,
                    max_rounds: row.get::<_, i64>(3)? as u32,
                    total_rounds: row.get::<_, i64>(4)? as u32,
                    passed: row.get::<_, i64>(5)? != 0,
                    total_time_ms: row.get::<_, i64>(6)? as u64,
                    final_verdict: row.get(7)?,
                    started_at: row.get(8)?,
                    finished_at: row.get(9)?,
                })
            },
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Load rounds for a specific run.
    pub fn load_rounds(&self, run_id: &str) -> rusqlite::Result<Vec<LoopRoundRecord>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, run_id, round_number, verdict, phase_times,
                    total_time_ms, details, created_at
             FROM loop_rounds WHERE run_id = ?1
             ORDER BY round_number ASC"
        )?;

        let rows = stmt.query_map(params![run_id], |row| {
            let phase_times_str: String = row.get(4)?;
            let phase_times: Vec<(String, u64)> =
                serde_json::from_str(&phase_times_str).unwrap_or_default();

            Ok(LoopRoundRecord {
                id: row.get(0)?,
                run_id: row.get(1)?,
                round_number: row.get::<_, i64>(2)? as u32,
                verdict: row.get(3)?,
                phase_times_ms: phase_times,
                total_time_ms: row.get::<_, i64>(5)? as u64,
                details: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;

        rows.collect()
    }

    /// Load diff records for a specific run.
    pub fn load_diffs(&self, run_id: &str) -> rusqlite::Result<Vec<DiffRecord>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, run_id, round_number, file_path, original_hash,
                    original, modified, description, status, created_at
             FROM diff_records WHERE run_id = ?1
             ORDER BY round_number ASC"
        )?;

        let rows = stmt.query_map(params![run_id], |row| {
            Ok(DiffRecord {
                id: row.get(0)?,
                run_id: row.get(1)?,
                round_number: row.get::<_, i64>(2)? as u32,
                file_path: row.get(3)?,
                original_hash: row.get(4)?,
                original: row.get(5)?,
                modified: row.get(6)?,
                description: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;

        rows.collect()
    }

    // ── Delete / cleanup ──

    /// Delete a run and all its rounds + diffs (cascade).
    pub fn delete_run(&self, run_id: &str) -> rusqlite::Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM loop_runs WHERE id = ?1", params![run_id])?;
        Ok(())
    }

    /// Purge runs older than N days.
    pub fn purge_old_runs(&self, days: i64) -> rusqlite::Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        let cutoff_str = cutoff.to_rfc3339();
        let count = self
            .db
            .conn()
            .execute("DELETE FROM loop_runs WHERE finished_at < ?1", params![cutoff_str])?;
        Ok(count)
    }

    /// Get total run count.
    pub fn total_run_count(&self) -> rusqlite::Result<usize> {
        self.db
            .conn()
            .query_row("SELECT COUNT(*) FROM loop_runs", [], |r| r.get(0))
    }
}

/// Compute a BLAKE3-style hash of a string for content-addressing.
fn blake3_hash(input: &str) -> String {
    // Use SHA-256 as fallback (available in std via `ring` or manual)
    // Since we don't want to add new deps, use a simple hash
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#loop::{LoopConfig, LoopPhase};

    fn make_test_summary() -> LoopSummary {
        let phase_times = vec![
            (LoopPhase::Observe, 10),
            (LoopPhase::Plan, 20),
            (LoopPhase::Act, 30),
            (LoopPhase::Evaluate, 5),
        ];

        LoopSummary {
            total_rounds: 1,
            passed: true,
            total_time_ms: 65,
            final_verdict: Some(LoopVerdict::Passed),
            results: vec![RoundResult {
                round: 1,
                verdict: LoopVerdict::Passed,
                phase_times_ms: phase_times,
                total_time_ms: 65,
                details: "All good".into(),
                spec_deltas: Vec::new(),
            }],
        }
    }

    #[test]
    fn test_save_and_load_run() {
        let store = LoopStore::open().unwrap();
        let summary = make_test_summary();

        let run_id = store.save_run("test-target", "review", 5, &summary).unwrap();
        assert!(!run_id.is_empty());

        // Load and verify
        let loaded = store.load_run(&run_id).unwrap().unwrap();
        assert_eq!(loaded.target, "test-target");
        assert!(loaded.passed);
        assert_eq!(loaded.total_rounds, 1);

        // Load rounds
        let rounds = store.load_rounds(&run_id).unwrap();
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].round_number, 1);

        // List runs
        let runs = store.list_runs(10).unwrap();
        assert!(!runs.is_empty());

        // Clean up
        store.delete_run(&run_id).unwrap();
        assert!(store.load_run(&run_id).unwrap().is_none());
    }

    #[test]
    fn test_save_and_load_diffs() {
        let store = LoopStore::open().unwrap();
        let summary = make_test_summary();
        let run_id = store.save_run("test-target", "review", 5, &summary).unwrap();

        let diff_id = store
            .save_diff(
                &run_id,
                1,
                "src/main.rs",
                "println!(\"hello\");",
                "println!(\"world\");",
                "Update greeting",
                "applied",
            )
            .unwrap();
        assert!(!diff_id.is_empty());

        let diffs = store.load_diffs(&run_id).unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file_path, "src/main.rs");
        assert_eq!(diffs[0].description, "Update greeting");

        store.delete_run(&run_id).unwrap();
    }

    #[test]
    fn test_save_complex_run() {
        let store = LoopStore::open().unwrap();

        // Create a multi-round summary
        let phase_times = vec![(LoopPhase::Observe, 5), (LoopPhase::Plan, 10)];

        let summary = LoopSummary {
            total_rounds: 2,
            passed: false,
            total_time_ms: 200,
            final_verdict: Some(LoopVerdict::Failed {
                reason: "Compile error".into(),
            }),
            results: vec![
                RoundResult {
                    round: 1,
                    verdict: LoopVerdict::Failed {
                        reason: "First attempt failed".into(),
                    },
                    phase_times_ms: phase_times.clone(),
                    total_time_ms: 100,
                    details: "".into(),
                    spec_deltas: Vec::new(),
                },
                RoundResult {
                    round: 2,
                    verdict: LoopVerdict::Passed,
                    phase_times_ms: phase_times,
                    total_time_ms: 100,
                    details: "Fixed".into(),
                    spec_deltas: Vec::new(),
                },
            ],
        };

        let run_id = store.save_run("complex-target", "test", 10, &summary).unwrap();

        let loaded = store.load_run(&run_id).unwrap().unwrap();
        assert!(!loaded.passed);
        assert_eq!(loaded.total_rounds, 2);
        assert_eq!(loaded.mode, "test");

        let rounds = store.load_rounds(&run_id).unwrap();
        assert_eq!(rounds.len(), 2);
        assert!(rounds[0].verdict.contains("Failed"));

        store.delete_run(&run_id).unwrap();
    }
}