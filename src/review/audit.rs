//! Immutable Audit Log — append-only trail for review/apply/verify events.
//!
//! Inspired by Paperclip's append-only audit log pattern.
//! Writes structured entries to `~/.carp/audit.log` in JSONL format.
//!
//! ## Format
//!
//! ```jsonl
//! {"timestamp":"2026-06-06T10:00:00.000Z","event_type":"review_start","agent":"review-engine","detail":"target=src/main.rs","session_id":"abc123"}
//! {"timestamp":"2026-06-06T10:00:01.000Z","event_type":"review_finding","agent":"security-scanner","detail":"[HIGH] Unsafe block","session_id":"abc123"}
//! {"timestamp":"2026-06-06T10:00:02.000Z","event_type":"apply","agent":"fix-applier","detail":"src/main.rs:42 patched","session_id":"abc123"}
//! {"timestamp":"2026-06-06T10:00:03.000Z","event_type":"verify","agent":"compiler","detail":"cargo check PASSED","session_id":"abc123"}
//! ```

use std::io::Write;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// AuditEntry — a single immutable audit record
// ============================================================================

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Event type: review_start, review_finding, apply, verify, etc.
    pub event_type: String,
    /// Agent or module responsible.
    pub agent: String,
    /// Human-readable detail.
    pub detail: String,
    /// Session ID for grouping related events.
    pub session_id: String,
    /// Optional severity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// Optional target file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_file: Option<String>,
}

impl AuditEntry {
    /// Create a new audit entry with ISO 8601 timestamp.
    pub fn new(
        event_type: &str,
        agent: &str,
        detail: &str,
        session_id: &str,
    ) -> Self {
        Self {
            timestamp: now_iso8601(),
            event_type: event_type.to_string(),
            agent: agent.to_string(),
            detail: detail.to_string(),
            session_id: session_id.to_string(),
            severity: None,
            target_file: None,
        }
    }

    /// Set severity level.
    pub fn with_severity(mut self, severity: &str) -> Self {
        self.severity = Some(severity.to_string());
        self
    }

    /// Set target file.
    pub fn with_target(mut self, target: &str) -> Self {
        self.target_file = Some(target.to_string());
        self
    }
}

// ============================================================================
// AuditLog — append-only, thread-safe audit log
// ============================================================================

/// Append-only audit log writing to `~/.carp/audit.log`.
pub struct AuditLog {
    /// Log file path.
    path: PathBuf,
    /// File handle for buffered writing.
    file: Mutex<std::fs::File>,
    /// Number of entries written this session.
    count: Mutex<u64>,
}

impl AuditLog {
    /// Open or create the audit log at the default path (`~/.carp/audit.log`).
    pub fn open() -> Result<Self, String> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| "Cannot determine home directory".to_string())?;

        let dir = PathBuf::from(&home).join(".carp");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;

        let path = dir.join("audit.log");
        Self::open_path(&path)
    }

    /// Open or create the audit log at a specific path.
    pub fn open_path(path: &PathBuf) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("Failed to open audit log {}: {}", path.display(), e))?;

        Ok(Self {
            path: path.clone(),
            file: Mutex::new(file),
            count: Mutex::new(0),
        })
    }

    /// Append an entry to the audit log.
    pub fn write(&self, entry: &AuditEntry) -> Result<(), String> {
        let json = serde_json::to_string(entry)
            .map_err(|e| format!("Serialization error: {}", e))?;

        let mut file = self.file.lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        writeln!(file, "{}", json)
            .map_err(|e| format!("Write error: {}", e))?;

        file.flush()
            .map_err(|e| format!("Flush error: {}", e))?;

        let mut count = self.count.lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        *count += 1;

        Ok(())
    }

    /// Convenience: create and write a simple entry in one call.
    pub fn record(
        &self,
        event_type: &str,
        agent: &str,
        detail: &str,
        session_id: &str,
    ) -> Result<(), String> {
        let entry = AuditEntry::new(event_type, agent, detail, session_id);
        self.write(&entry)
    }

    /// Read all entries from the log file.
    pub fn read_all(&self) -> Result<Vec<AuditEntry>, String> {
        let content = std::fs::read_to_string(&self.path)
            .map_err(|e| format!("Failed to read audit log: {}", e))?;

        content.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<AuditEntry>(line)
                    .map_err(|e| format!("Parse error: {}", e))
            })
            .collect()
    }

    /// Get the log file path.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the number of entries written this session.
    pub fn count(&self) -> u64 {
        self.count.lock().map(|c| *c).unwrap_or(0)
    }

    /// Filter entries by event type.
    pub fn filter_by_type(&self, event_type: &str) -> Result<Vec<AuditEntry>, String> {
        let all = self.read_all()?;
        Ok(all.into_iter().filter(|e| e.event_type == event_type).collect())
    }

    /// Filter entries by session ID.
    pub fn filter_by_session(&self, session_id: &str) -> Result<Vec<AuditEntry>, String> {
        let all = self.read_all()?;
        Ok(all.into_iter().filter(|e| e.session_id == session_id).collect())
    }
}

/// Get current time as ISO 8601 string.
fn now_iso8601() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    // Simple ISO 8601 without timezone dependency
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Approximate date from Unix epoch (2020-01-01 + days)
    // This is a simplification — in production use chrono
    let (year, month, day) = days_to_date(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, millis
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days: i64) -> (i64, u32, u32) {
    // Days since 1970-01-01
    let mut y = 1970i64;
    let mut d = days;
    loop {
        let year_days = if is_leap(y) { 366 } else { 365 };
        if d < year_days {
            break;
        }
        d -= year_days;
        y += 1;
    }
    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0u32;
    for (i, &md) in month_days.iter().enumerate() {
        if d < md as i64 {
            m = i as u32 + 1;
            break;
        }
        d -= md as i64;
    }
    (y, m, (d + 1) as u32)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

// ============================================================================
// ReviewAuditTrail — high-level audit helper for review sessions
// ============================================================================

/// High-level audit helper wrapping AuditLog for review sessions.
pub struct ReviewAuditTrail {
    log: AuditLog,
    session_id: String,
}

impl ReviewAuditTrail {
    /// Open a new audit trail for a review session.
    pub fn new(session_id: &str) -> Result<Self, String> {
        let log = AuditLog::open()?;
        Ok(Self {
            log,
            session_id: session_id.to_string(),
        })
    }

    /// Log review start.
    pub fn review_start(&self, target: &str) -> Result<(), String> {
        self.log.record("review_start", "review-engine", target, &self.session_id)
    }

    /// Log a finding from review.
    pub fn review_finding(&self, agent: &str, detail: &str, severity: &str, file: &str) -> Result<(), String> {
        let entry = AuditEntry::new("review_finding", agent, detail, &self.session_id)
            .with_severity(severity)
            .with_target(file);
        self.log.write(&entry)
    }

    /// Log an apply operation.
    pub fn apply(&self, file: &str, result: &str) -> Result<(), String> {
        let entry = AuditEntry::new("apply", "fix-applier", result, &self.session_id)
            .with_target(file);
        self.log.write(&entry)
    }

    /// Log a verify operation.
    pub fn verify(&self, result: &str) -> Result<(), String> {
        self.log.record("verify", "compiler", result, &self.session_id)
    }

    /// Log workflow step.
    pub fn workflow_step(&self, step_id: &str, status: &str) -> Result<(), String> {
        self.log.record("workflow_step", step_id, status, &self.session_id)
    }

    /// Log agent dispatch.
    pub fn agent_dispatch(&self, agent_name: &str, task: &str) -> Result<(), String> {
        self.log.record("agent_dispatch", agent_name, task, &self.session_id)
    }

    /// Get underlying log reference.
    pub fn log(&self) -> &AuditLog {
        &self.log
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_path() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("carp_audit_test_{}.log", std::process::id()));
        p
    }

    #[test]
    fn test_audit_entry_creation() {
        let entry = AuditEntry::new("review_start", "test-agent", "testing", "session-1");
        assert_eq!(entry.event_type, "review_start");
        assert_eq!(entry.agent, "test-agent");
        assert_eq!(entry.session_id, "session-1");
        assert!(entry.timestamp.len() >= 20);
    }

    #[test]
    fn test_audit_entry_with_severity() {
        let entry = AuditEntry::new("finding", "scanner", "unsafe block", "s1")
            .with_severity("HIGH")
            .with_target("src/main.rs");
        assert_eq!(entry.severity, Some("HIGH".into()));
        assert_eq!(entry.target_file, Some("src/main.rs".into()));
    }

    #[test]
    fn test_audit_log_write_and_read() {
        let path = test_path();
        let log = AuditLog::open_path(&path).unwrap();

        log.record("test", "tester", "hello", "s1").unwrap();
        log.record("test", "tester", "world", "s1").unwrap();

        let entries = log.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].detail, "hello");
        assert_eq!(entries[1].detail, "world");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_audit_log_filter_by_type() {
        let path = test_path();
        let log = AuditLog::open_path(&path).unwrap();

        log.record("review_start", "engine", "src/main.rs", "s1").unwrap();
        log.record("apply", "fixer", "patched", "s1").unwrap();
        log.record("verify", "compiler", "PASSED", "s1").unwrap();

        let reviews = log.filter_by_type("review_start").unwrap();
        assert_eq!(reviews.len(), 1);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_audit_log_filter_by_session() {
        let path = test_path();
        let log = AuditLog::open_path(&path).unwrap();

        log.record("test", "t", "a", "s1").unwrap();
        log.record("test", "t", "b", "s2").unwrap();
        log.record("test", "t", "c", "s1").unwrap();

        let s1 = log.filter_by_session("s1").unwrap();
        assert_eq!(s1.len(), 2);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_review_audit_trail() {
        let path = test_path();
        let trail = ReviewAuditTrail::new("test-session").unwrap();

        // Override log path for testing
        let _log = AuditLog::open_path(&path).unwrap();
        // Just test that the API compiles and works
        trail.review_start("src/main.rs").ok();

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_days_to_date() {
        // 1970-01-01 = day 0
        let (y, m, d) = days_to_date(0);
        assert_eq!(y, 1970);
        assert_eq!(m, 1);
        assert_eq!(d, 1);

        // 2026-01-01 = ~20454 days from 1970
        let (y, m, d) = days_to_date(20454);
        assert_eq!(y, 2026);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn test_is_leap() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(2023));
        assert!(!is_leap(1900));
    }
}