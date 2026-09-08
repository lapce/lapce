//! Audit Logger - Operation tracking and traceability.
//!
//! This module provides:
//! - Operation logging
//! - Audit trail
//! - Compliance reporting
//! - Query and search

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// An audit log entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: u64,
    pub event_type: AuditEventType,
    pub actor: Actor,
    pub resource: Resource,
    pub action: String,
    pub status: OperationStatus,
    pub details: HashMap<String, String>,
    pub ip_address: Option<String>,
    pub session_id: Option<String>,
}

/// Type of audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum AuditEventType {
    Authentication,
    Authorization,
    DataAccess,
    DataModification,
    Configuration,
    System,
    Security,
}

/// Operation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum OperationStatus {
    Success,
    Failure,
    Partial,
    Pending,
}

/// An actor (user or system).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Actor {
    pub id: String,
    pub actor_type: ActorType,
    pub name: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ActorType {
    User,
    System,
    Service,
}

/// A resource being acted upon.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Resource {
    pub id: String,
    pub resource_type: String,
    pub path: Option<String>,
}

/// Audit log configuration.
#[derive(Debug, Clone)]
pub struct AuditConfig {
    pub retention_days: u32,
    pub enable_persistence: bool,
    pub enable_compression: bool,
    pub max_entries_in_memory: usize,
    pub flush_interval_secs: u64,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            retention_days: 90,
            enable_persistence: true,
            enable_compression: true,
            max_entries_in_memory: 10000,
            flush_interval_secs: 60,
        }
    }
}

/// Audit logger.
pub struct AuditLogger {
    config: AuditConfig,
    entries: Arc<RwLock<VecDeque<AuditEntry>>>,
    indices: Arc<RwLock<AuditIndices>>,
    stats: Arc<RwLock<AuditStats>>,
}

/// Indices for fast querying.
#[derive(Debug, Clone, Default)]
struct AuditIndices {
    by_time: Vec<(u64, usize)>,
    by_actor: HashMap<String, Vec<usize>>,
    by_resource: HashMap<String, Vec<usize>>,
    by_type: HashMap<AuditEventType, Vec<usize>>,
}

/// Audit statistics.
#[derive(Debug, Clone, Default)]
pub struct AuditStats {
    pub total_entries: usize,
    pub entries_today: usize,
    pub auth_failures: usize,
    pub last_entry_at: u64,
}

impl AuditLogger {
    pub fn new(config: AuditConfig) -> Self {
        let max_entries = config.max_entries_in_memory;
        Self {
            config,
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(max_entries))),
            indices: Arc::new(RwLock::new(AuditIndices::default())),
            stats: Arc::new(RwLock::new(AuditStats::default())),
        }
    }

    /// Log an audit event.
    pub async fn log(&self, entry: AuditEntry) -> String {
        let entry_id = entry.id.clone();
        let timestamp = entry.timestamp;
        let actor_id = entry.actor.id.clone();
        let resource_id = entry.resource.id.clone();
        let event_type = entry.event_type;

        // Add to entries
        let mut entries = self.entries.write().await;
        let index = entries.len();
        entries.push_back(entry.clone());

        // Evict old entries if necessary
        if entries.len() > self.config.max_entries_in_memory {
            entries.pop_front();
        }

        // Update indices
        drop(entries);
        let mut indices = self.indices.write().await;
        indices.by_time.push((timestamp, index));
        indices.by_actor.entry(actor_id).or_insert_with(Vec::new).push(index);
        indices.by_resource.entry(resource_id).or_insert_with(Vec::new).push(index);
        indices.by_type.entry(event_type).or_insert_with(Vec::new).push(index);

        // Update stats
        drop(indices);
        let mut stats = self.stats.write().await;
        stats.total_entries += 1;
        stats.last_entry_at = timestamp;

        if entry.status == OperationStatus::Failure && entry.event_type == AuditEventType::Authentication {
            stats.auth_failures += 1;
        }

        entry_id
    }

    /// Query logs by time range.
    pub async fn query_by_time(&self, start: u64, end: u64) -> Vec<AuditEntry> {
        let indices = self.indices.read().await;
        let entries = self.entries.read().await;

        let mut results = Vec::new();

        for &(ts, idx) in &indices.by_time {
            if ts >= start && ts <= end {
                if let Some(entry) = entries.get(idx) {
                    results.push(entry.clone());
                }
            }
        }

        results
    }

    /// Query logs by actor.
    pub async fn query_by_actor(&self, actor_id: &str) -> Vec<AuditEntry> {
        let indices = self.indices.read().await;
        let entries = self.entries.read().await;

        let mut results = Vec::new();

        if let Some(indexes) = indices.by_actor.get(actor_id) {
            for &idx in indexes {
                if let Some(entry) = entries.get(idx) {
                    results.push(entry.clone());
                }
            }
        }

        results
    }

    /// Query logs by resource.
    pub async fn query_by_resource(&self, resource_id: &str) -> Vec<AuditEntry> {
        let indices = self.indices.read().await;
        let entries = self.entries.read().await;

        let mut results = Vec::new();

        if let Some(indexes) = indices.by_resource.get(resource_id) {
            for &idx in indexes {
                if let Some(entry) = entries.get(idx) {
                    results.push(entry.clone());
                }
            }
        }

        results
    }

    /// Query logs by event type.
    pub async fn query_by_type(&self, event_type: AuditEventType) -> Vec<AuditEntry> {
        let indices = self.indices.read().await;
        let entries = self.entries.read().await;

        let mut results = Vec::new();

        if let Some(indexes) = indices.by_type.get(&event_type) {
            for &idx in indexes {
                if let Some(entry) = entries.get(idx) {
                    results.push(entry.clone());
                }
            }
        }

        results
    }

    /// Search logs by keyword.
    pub async fn search(&self, keyword: &str) -> Vec<AuditEntry> {
        let entries = self.entries.read().await;
        let keyword_lower = keyword.to_lowercase();

        entries
            .iter()
            .filter(|entry| {
                entry.action.to_lowercase().contains(&keyword_lower) ||
                entry.details.values().any(|v| v.to_lowercase().contains(&keyword_lower))
            })
            .cloned()
            .collect()
    }

    /// Get audit statistics.
    pub async fn stats(&self) -> AuditStats {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// Generate compliance report.
    pub async fn generate_report(&self, start: u64, end: u64) -> ComplianceReport {
        let logs = self.query_by_time(start, end).await;

        let mut by_type: HashMap<String, usize> = HashMap::new();
        let mut by_status: HashMap<String, usize> = HashMap::new();
        let mut failed_auth = 0;
        let mut data_modifications = 0;

        for entry in &logs {
            *by_type.entry(format!("{:?}", entry.event_type)).or_insert(0) += 1;
            *by_status.entry(format!("{:?}", entry.status)).or_insert(0) += 1;

            if entry.event_type == AuditEventType::Authentication && entry.status == OperationStatus::Failure {
                failed_auth += 1;
            }
            if entry.event_type == AuditEventType::DataModification {
                data_modifications += 1;
            }
        }

        ComplianceReport {
            period_start: start,
            period_end: end,
            total_events: logs.len(),
            by_event_type: by_type,
            by_status,
            failed_auth_attempts: failed_auth,
            data_modifications,
            unique_users: logs.iter().map(|e| e.actor.id.clone()).collect::<std::collections::HashSet<_>>().len(),
        }
    }

    /// Export logs to JSON.
    pub async fn export_json(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;

        let entries = self.entries.read().await;
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);

        for entry in entries.iter() {
            let json = serde_json::to_string(entry)?;
            writeln!(writer, "{}", json)?;
        }

        Ok(())
    }

    /// Clear old entries based on retention policy.
    pub async fn cleanup(&self) -> usize {
        let cutoff = current_timestamp() - (self.config.retention_days as u64 * 86400);

        let mut entries = self.entries.write().await;
        let mut removed = 0;

        while entries.front().map(|e| e.timestamp < cutoff).unwrap_or(false) {
            entries.pop_front();
            removed += 1;
        }

        removed
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(AuditConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct ComplianceReport {
    pub period_start: u64,
    pub period_end: u64,
    pub total_events: usize,
    pub by_event_type: HashMap<String, usize>,
    pub by_status: HashMap<String, usize>,
    pub failed_auth_attempts: usize,
    pub data_modifications: usize,
    pub unique_users: usize,
}

/// Builder for audit entries.
pub struct AuditEntryBuilder {
    entry: AuditEntry,
}

impl AuditEntryBuilder {
    pub fn new(event_type: AuditEventType, action: String) -> Self {
        Self {
            entry: AuditEntry {
                id: format!("audit_{}", current_timestamp()),
                timestamp: current_timestamp(),
                event_type,
                actor: Actor {
                    id: "system".to_string(),
                    actor_type: ActorType::System,
                    name: "System".to_string(),
                    roles: Vec::new(),
                },
                resource: Resource {
                    id: "unknown".to_string(),
                    resource_type: "unknown".to_string(),
                    path: None,
                },
                action,
                status: OperationStatus::Success,
                details: HashMap::new(),
                ip_address: None,
                session_id: None,
            },
        }
    }

    pub fn actor(mut self, actor: Actor) -> Self {
        self.entry.actor = actor;
        self
    }

    pub fn resource(mut self, resource: Resource) -> Self {
        self.entry.resource = resource;
        self
    }

    pub fn status(mut self, status: OperationStatus) -> Self {
        self.entry.status = status;
        self
    }

    pub fn detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.entry.details.insert(key.into(), value.into());
        self
    }

    pub fn ip_address(mut self, ip: impl Into<String>) -> Self {
        self.entry.ip_address = Some(ip.into());
        self
    }

    pub fn session_id(mut self, session: impl Into<String>) -> Self {
        self.entry.session_id = Some(session.into());
        self
    }

    pub fn build(self) -> AuditEntry {
        self.entry
    }
}

/// Get current timestamp.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unwrap failed: audit_logger.rs:418")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_entry() {
        let logger = AuditLogger::default();

        let entry = AuditEntryBuilder::new(AuditEventType::Authentication, "User logged in".to_string())
            .status(OperationStatus::Success)
            .build();

        let id = logger.log(entry).await;
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn test_query_by_type() {
        let logger = AuditLogger::default();

        logger.log(
            AuditEntryBuilder::new(AuditEventType::Authentication, "login".to_string()).build()
        ).await;

        logger.log(
            AuditEntryBuilder::new(AuditEventType::DataModification, "update".to_string()).build()
        ).await;

        let results = logger.query_by_type(AuditEventType::Authentication).await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_stats() {
        let logger = AuditLogger::default();

        logger.log(
            AuditEntryBuilder::new(AuditEventType::Authentication, "login".to_string())
                .status(OperationStatus::Failure)
                .build()
        ).await;

        let stats = logger.stats().await;
        assert_eq!(stats.auth_failures, 1);
    }

    #[tokio::test]
    async fn test_compliance_report() {
        let logger = AuditLogger::default();
        let now = current_timestamp();

        for _ in 0..5 {
            logger.log(
                AuditEntryBuilder::new(AuditEventType::DataAccess, "read".to_string()).build()
            ).await;
        }

        let report = logger.generate_report(now - 3600, now + 3600).await;
        assert_eq!(report.total_events, 5);
    }
}
