//! File-based archive for LoopEngine runs.
//!
//! Unlike the SQLite-backed `LoopStore` (which stores every run for querying),
//! the archive saves successful runs as individual JSON files in
//! `.carp/archive/`. This makes them portable, human-readable, and
//! easy to share or inspect without opening the database.
//!
//! ## Location
//!
//! `{project_root}/.carp/archive/{run_id}.json`
//!
//! Each file contains the full `LoopSummary` (round results, spec deltas,
//! timing) plus the run metadata.
//!
//! ## Retro Reports (`/retro`)
//!
//! Inspired by gstack's `/retro` skill — analyzes archived runs to produce
//! a data-driven engineering retrospective with metrics, patterns, and insights.

use crate::r#loop::{LoopSummary, LoopVerdict};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Metadata about an archived run — lightweight, list-friendly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveMeta {
    pub run_id: String,
    pub target: String,
    pub mode: String,
    pub passed: bool,
    pub total_rounds: u32,
    pub total_time_ms: u64,
    pub created_at: String,
}

/// Full archive entry — includes the complete loop summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopArchive {
    pub meta: ArchiveMeta,
    pub max_rounds: u32,
    pub final_verdict: Option<LoopVerdict>,
    pub summary: LoopSummary,
}

impl LoopArchive {
    /// Build an archive from a completed loop run summary.
    pub fn from_summary(
        run_id: String,
        target: &str,
        mode: &str,
        max_rounds: u32,
        summary: &LoopSummary,
    ) -> Self {
        let meta = ArchiveMeta {
            run_id: run_id.clone(),
            target: target.to_string(),
            mode: mode.to_string(),
            passed: summary.passed,
            total_rounds: summary.total_rounds,
            total_time_ms: summary.total_time_ms,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        Self {
            meta,
            max_rounds,
            final_verdict: summary.final_verdict.clone(),
            summary: summary.clone(),
        }
    }

    /// Get the archive directory path for a project root.
    pub fn archive_dir(project_root: &Path) -> PathBuf {
        project_root.join(".carp").join("archive")
    }

    /// Get the file path for a given run ID.
    fn path_for(project_root: &Path, run_id: &str) -> PathBuf {
        Self::archive_dir(project_root).join(format!("{}.json", run_id))
    }

    /// Save this archive to disk.
    pub fn save(&self, project_root: &Path) -> anyhow::Result<PathBuf> {
        let dir = Self::archive_dir(project_root);
        std::fs::create_dir_all(&dir)?;
        let path = Self::path_for(project_root, &self.meta.run_id);
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Load an archive from disk by run ID.
    pub fn load(project_root: &Path, run_id: &str) -> anyhow::Result<Option<Self>> {
        let path = Self::path_for(project_root, run_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let archive: LoopArchive = serde_json::from_str(&content)?;
        Ok(Some(archive))
    }

    /// List all archived runs, newest first.
    pub fn list(project_root: &Path) -> anyhow::Result<Vec<ArchiveMeta>> {
        let dir = Self::archive_dir(project_root);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if let Ok(archive) = serde_json::from_str::<LoopArchive>(&content) {
                        entries.push(archive.meta);
                    }
                }
                Err(_) => continue,
            }
        }

        // Sort by created_at descending (newest first)
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(entries)
    }

    /// Delete an archived run by ID.
    pub fn delete(project_root: &Path, run_id: &str) -> anyhow::Result<bool> {
        let path = Self::path_for(project_root, run_id);
        if path.exists() {
            std::fs::remove_file(&path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Purge archives older than N days. Returns count of deleted entries.
    pub fn purge_older_than(project_root: &Path, days: i64) -> anyhow::Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        let cutoff_str = cutoff.to_rfc3339();
        let mut count = 0;

        let dir = Self::archive_dir(project_root);
        if !dir.is_dir() {
            return Ok(0);
        }

        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(archive) = serde_json::from_str::<LoopArchive>(&content) {
                    if archive.meta.created_at < cutoff_str {
                        std::fs::remove_file(&path)?;
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }
}

// ============================================================================
// Retro Report — data-driven engineering retrospective (gstack /retro)
// ============================================================================

/// A data-driven retrospective of archived LoopEngine runs.
///
/// Inspired by gstack's `/retro` skill — mines archive history to produce
/// metrics, patterns, and insights about development velocity, quality,
/// and friction points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetroReport {
    /// When this report was generated.
    pub generated_at: String,
    /// Time range covered.
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    /// Total runs in this period.
    pub total_runs: u32,
    /// Runs that passed.
    pub passed_runs: u32,
    /// Pass rate as percentage.
    pub pass_rate_pct: f64,
    /// Average rounds per run.
    pub avg_rounds: f64,
    /// Average time per run (ms).
    pub avg_time_ms: f64,
    /// Fastest run time.
    pub best_time_ms: u64,
    /// Slowest run time.
    pub worst_time_ms: u64,
    /// Breakdown by mode (e.g. {"review": 5, "test": 3}).
    pub by_mode: std::collections::HashMap<String, u32>,
    /// Breakdown by target file.
    pub hotspots: Vec<HotspotEntry>,
    /// Summary narrative (generated by LLM or template).
    pub summary: String,
}

/// A hotspot entry — files that appear frequently in archives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotEntry {
    pub target: String,
    pub run_count: u32,
    pub pass_count: u32,
}

impl RetroReport {
    /// Generate a retro report from all archived runs.
    ///
    /// Reads every archive JSON, computes aggregate metrics, and produces
    /// a structured retro suitable for CLI output or IDE display.
    pub fn generate(project_root: &Path) -> anyhow::Result<Self> {
        let all_archives = Self::load_all(project_root)?;
        if all_archives.is_empty() {
            return Ok(Self::empty());
        }

        let total_runs = all_archives.len() as u32;
        let passed_runs = all_archives.iter().filter(|a| a.meta.passed).count() as u32;
        let pass_rate_pct = if total_runs > 0 {
            (passed_runs as f64 / total_runs as f64) * 100.0
        } else {
            0.0
        };

        let total_rounds: u32 = all_archives.iter().map(|a| a.summary.total_rounds).sum();
        let avg_rounds = if total_runs > 0 {
            total_rounds as f64 / total_runs as f64
        } else {
            0.0
        };

        let total_time_ms: u64 = all_archives.iter().map(|a| a.summary.total_time_ms).sum();
        let avg_time_ms = if total_runs > 0 {
            total_time_ms as f64 / total_runs as f64
        } else {
            0.0
        };

        let best_time_ms = all_archives.iter()
            .map(|a| a.summary.total_time_ms)
            .min()
            .unwrap_or(0);
        let worst_time_ms = all_archives.iter()
            .map(|a| a.summary.total_time_ms)
            .max()
            .unwrap_or(0);

        // By mode breakdown
        let mut by_mode: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for a in &all_archives {
            *by_mode.entry(a.meta.mode.clone()).or_insert(0) += 1;
        }

        // Hotspot files
        let mut targets: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
        for a in &all_archives {
            let entry = targets.entry(a.meta.target.clone()).or_insert((0, 0));
            entry.0 += 1;
            if a.meta.passed { entry.1 += 1; }
        }
        let mut hotspots: Vec<HotspotEntry> = targets.into_iter()
            .map(|(target, (run_count, pass_count))| HotspotEntry { target, run_count, pass_count })
            .collect();
        hotspots.sort_by(|a, b| b.run_count.cmp(&a.run_count));

        // Time bounds
        let period_start = all_archives.last().map(|a| a.meta.created_at.clone());
        let period_end = all_archives.first().map(|a| a.meta.created_at.clone());

        // Auto-generate summary
        let summary = Self::generate_summary(
            total_runs, passed_runs, pass_rate_pct,
            avg_rounds, avg_time_ms, &hotspots,
        );

        Ok(RetroReport {
            generated_at: chrono::Utc::now().to_rfc3339(),
            period_start,
            period_end,
            total_runs,
            passed_runs,
            pass_rate_pct,
            avg_rounds,
            avg_time_ms,
            best_time_ms,
            worst_time_ms,
            by_mode,
            hotspots,
            summary,
        })
    }

    /// Load all archives from disk.
    fn load_all(project_root: &Path) -> anyhow::Result<Vec<LoopArchive>> {
        let list = LoopArchive::list(project_root)?;
        let mut archives = Vec::with_capacity(list.len());
        for meta in list {
            match LoopArchive::load(project_root, &meta.run_id) {
                Ok(Some(a)) => archives.push(a),
                _ => continue,
            }
        }
        Ok(archives)
    }

    /// Generate an empty retro report (no data available).
    fn empty() -> Self {
        Self {
            generated_at: chrono::Utc::now().to_rfc3339(),
            period_start: None,
            period_end: None,
            total_runs: 0,
            passed_runs: 0,
            pass_rate_pct: 0.0,
            avg_rounds: 0.0,
            avg_time_ms: 0.0,
            best_time_ms: 0,
            worst_time_ms: 0,
            by_mode: std::collections::HashMap::new(),
            hotspots: Vec::new(),
            summary: "No archived runs found. Run some loop executions first.".into(),
        }
    }

    /// Template-based summary generation.
    fn generate_summary(
        total: u32, passed: u32, rate: f64,
        avg_rounds: f64, avg_time_ms: f64,
        hotspots: &[HotspotEntry],
    ) -> String {
        let mut s = format!(
            "Retro: {} runs analyzed, {:.1}% pass rate ({}/{} passed).\n",
            total, rate, passed, total
        );
        s.push_str(&format!("Avg {:.1} rounds/run, {:.1}s/run.\n", avg_rounds, avg_time_ms / 1000.0));
        if !hotspots.is_empty() {
            s.push_str("\nHotspot files:\n");
            for h in hotspots.iter().take(10) {
                s.push_str(&format!("  - {}: {} runs ({} passed)\n",
                    h.target, h.run_count, h.pass_count));
            }
        }
        s
    }

    /// Format as human-readable text for CLI output.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("╔════════════════════════════════════════════╗\n");
        out.push_str("║     deepseek-carp Engineering Retrospective   ║\n");
        out.push_str("╚════════════════════════════════════════════╝\n\n");
        out.push_str(&format!("Period: {} → {}\n",
            self.period_start.as_deref().unwrap_or("N/A"),
            self.period_end.as_deref().unwrap_or("N/A"),
        ));
        out.push_str(&format!("Runs: {} | Passed: {} | Pass Rate: {:.1}%\n\n",
            self.total_runs, self.passed_runs, self.pass_rate_pct));
        out.push_str(&format!("Avg Rounds/Run: {:.1}\n", self.avg_rounds));
        out.push_str(&format!("Avg Time/Run: {:.1}s\n", self.avg_time_ms / 1000.0));
        out.push_str(&format!("Best: {:.1}s | Worst: {:.1}s\n\n",
            self.best_time_ms as f64 / 1000.0, self.worst_time_ms as f64 / 1000.0));

        if !self.by_mode.is_empty() {
            out.push_str("By Mode:\n");
            for (mode, count) in &self.by_mode {
                out.push_str(&format!("  - {}: {} runs\n", mode, count));
            }
            out.push('\n');
        }

        if !self.hotspots.is_empty() {
            out.push_str("Hotspot Files:\n");
            for h in &self.hotspots {
                out.push_str(&format!("  - [{}x] {} ({} passed)\n",
                    h.run_count, h.target, h.pass_count));
            }
            out.push('\n');
        }

        out.push_str("--- Summary ---\n");
        out.push_str(&self.summary);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#loop::{LoopConfig, LoopPhase, RoundResult};

    fn make_test_archive() -> (LoopArchive, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let phase_times = vec![
            (LoopPhase::Observe, 10u64),
            (LoopPhase::Plan, 20),
            (LoopPhase::Act, 30),
            (LoopPhase::Evaluate, 5),
        ];
        let summary = LoopSummary {
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
        };
        let archive = LoopArchive::from_summary(
            "test-run-001".into(),
            "src/main.rs",
            "review",
            5,
            &summary,
        );
        (archive, dir)
    }

    #[test]
    fn test_archive_save_and_load() {
        let (archive, dir) = make_test_archive();
        let root = dir.path();

        // Save
        let path = archive.save(root).unwrap();
        assert!(path.exists());
        assert!(path.to_string_lossy().contains("test-run-001"));

        // Load
        let loaded = LoopArchive::load(root, "test-run-001").unwrap().unwrap();
        assert_eq!(loaded.meta.mode, "review");
        assert!(loaded.meta.passed);
        assert_eq!(loaded.summary.total_rounds, 1);
    }

    #[test]
    fn test_archive_list() {
        let (archive1, dir) = make_test_archive();
        let root = dir.path();
        archive1.save(root).unwrap();

        let list = LoopArchive::list(root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].run_id, "test-run-001");
    }

    #[test]
    fn test_archive_delete() {
        let (archive, dir) = make_test_archive();
        let root = dir.path();
        archive.save(root).unwrap();

        assert!(LoopArchive::delete(root, "test-run-001").unwrap());
        assert!(LoopArchive::load(root, "test-run-001").unwrap().is_none());
    }

    #[test]
    fn test_archive_list_empty() {
        let dir = tempfile::tempdir().unwrap();
        let list = LoopArchive::list(dir.path()).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_archive_load_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = LoopArchive::load(dir.path(), "nonexistent").unwrap();
        assert!(loaded.is_none());
    }
}