//! Audit logging system — complete operation traceability.
//!
//! Every significant action is logged with: who, when, what, result.
//! Logs are stored in memory and can be exported as structured JSON.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

// ─── Event types ────────────────────────────────────────────────

/// Classification of audit events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuditEventType {
    // Authentication & Access
    ApiKeyUsed,
    KeyRotated,
    SessionCreated,
    SessionEnded,

    // Code Operations
    FileRead,
    FileWritten,
    FileDeleted,
    BatchEditStarted,
    BatchEditCommitted,
    BatchEditRolledBack,

    // AI Operations
    PromptSubmitted,
    ResponseReceived,
    CacheHit,
    CacheMiss,
    SanitizationBlocked,
    CostAlertTriggered,
    BudgetExceeded,

    // System Events
    ConfigChanged,
    PluginLoaded,
    PluginUnloaded,
    SystemStartup,
    SystemShutdown,
    ErrorOccurred,
    RecoveryActionTaken,
}

impl AuditEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKeyUsed => "api_key_used",
            Self::KeyRotated => "key_rotated",
            Self::SessionCreated => "session_created",
            Self::SessionEnded => "session_ended",
            Self::FileRead => "file_read",
            Self::FileWritten => "file_written",
            Self::FileDeleted => "file_deleted",
            Self::BatchEditStarted => "batch_edit_started",
            Self::BatchEditCommitted => "batch_edit_committed",
            Self::BatchEditRolledBack => "batch_edit_rolled_back",
            Self::PromptSubmitted => "prompt_submitted",
            Self::ResponseReceived => "response_received",
            Self::CacheHit => "cache_hit",
            Self::CacheMiss => "cache_miss",
            Self::SanitizationBlocked => "sanitization_blocked",
            Self::CostAlertTriggered => "cost_alert_triggered",
            Self::BudgetExceeded => "budget_exceeded",
            Self::ConfigChanged => "config_changed",
            Self::PluginLoaded => "plugin_loaded",
            Self::PluginUnloaded => "plugin_unloaded",
            Self::SystemStartup => "system_startup",
            Self::SystemShutdown => "system_shutdown",
            Self::ErrorOccurred => "error_occurred",
            Self::RecoveryActionTaken => "recovery_action_taken",
        }
    }
}

// ─── Actor & Resource ───────────────────────────────────────────

/// Information about who triggered an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorInfo {
    pub actor_type: ActorType,
    pub identifier: String,
    pub ip_address: Option<String>,
    pub client_version: Option<String>,
}

impl ActorInfo {
    pub fn user(identifier: impl Into<String>) -> Self {
        Self {
            actor_type: ActorType::User,
            identifier: identifier.into(),
            ip_address: None,
            client_version: None,
        }
    }

    pub fn agent(identifier: impl Into<String>) -> Self {
        Self {
            actor_type: ActorType::Agent,
            identifier: identifier.into(),
            ip_address: None,
            client_version: None,
        }
    }

    pub fn system() -> Self {
        Self {
            actor_type: ActorType::System,
            identifier: "system".to_string(),
            ip_address: None,
            client_version: None,
        }
    }

    pub fn plugin(name: impl Into<String>) -> Self {
        Self {
            actor_type: ActorType::Plugin,
            identifier: name.into(),
            ip_address: None,
            client_version: None,
        }
    }
}

/// Type of actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorType {
    User,
    Agent,
    System,
    Plugin,
}

/// Information about what resource an event targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub resource_type: ResourceType,
    pub path: Option<String>,
    pub name: Option<String>,
}

impl ResourceInfo {
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::File,
            path: Some(path.into()),
            name: None,
        }
    }

    pub fn api_endpoint(url: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::ApiEndpoint,
            path: Some(url.into()),
            name: None,
        }
    }

    pub fn session(id: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::Session,
            path: None,
            name: Some(id.into()),
        }
    }

    pub fn configuration(key: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::Configuration,
            path: None,
            name: Some(key.into()),
        }
    }

    pub fn model(name: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::Model,
            path: None,
            name: Some(name.into()),
        }
    }

    pub fn cache(key: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::Cache,
            path: None,
            name: Some(key.into()),
        }
    }

    pub fn budget(id: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::Budget,
            path: None,
            name: Some(id.into()),
        }
    }

    pub fn transaction(id: impl Into<String>) -> Self {
        Self {
            resource_type: ResourceType::Transaction,
            path: None,
            name: Some(id.into()),
        }
    }
}

/// Type of resource being operated on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    File,
    ApiEndpoint,
    Session,
    Configuration,
    Model,
    Cache,
    Budget,
    Transaction,
}

// ─── Outcome ────────────────────────────────────────────────────

/// Result of the audited operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    Success,
    Failure,
    Blocked,
    Partial,
    Unknown,
}

impl AuditOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Blocked => "blocked",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
        }
    }
}

// ─── AuditEvent ─────────────────────────────────────────────────

/// A single audit event record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub timestamp: u64,
    pub event_type: AuditEventType,
    pub actor: ActorInfo,
    pub resource: ResourceInfo,
    pub action: String,
    pub details: serde_json::Value,
    pub outcome: AuditOutcome,
    pub duration_ms: u64,
    pub session_id: String,
    pub request_id: Option<String>,
}

impl AuditEvent {
    pub fn new(
        event_type: AuditEventType,
        actor: ActorInfo,
        resource: ResourceInfo,
        action: impl Into<String>,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            event_type,
            actor,
            resource,
            action: action.into(),
            details: serde_json::Value::Object(serde_json::Map::new()),
            outcome,
            duration_ms: 0,
            session_id: uuid::Uuid::new_v4().to_string(),
            request_id: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    pub fn with_duration_ms(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }

    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = id.into();
        self
    }

    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }
}

// ─── Query ──────────────────────────────────────────────────────

/// Filter criteria for querying audit events.
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub event_types: Vec<AuditEventType>,
    pub actors: Vec<String>,
    pub resources: Vec<String>,
    pub outcomes: Vec<AuditOutcome>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<usize>,
}

// ─── Stats ──────────────────────────────────────────────────────

/// Aggregated statistics from the audit log.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuditStats {
    pub total_events: usize,
    pub by_event_type: HashMap<String, usize>,
    pub by_outcome: HashMap<String, usize>,
    pub failures_last_24h: usize,
    pub blocked_attempts: usize,
    pub most_active_actor: Option<String>,
    pub most_accessed_resource: Option<String>,
}

// ─── AuditLog ───────────────────────────────────────────────────

/// Main audit logger — records, queries, and exports audit events.
pub struct AuditLog {
    events: Arc<RwLock<Vec<AuditEvent>>>,
    log_path: PathBuf,
    enabled: bool,
    retention_days: u32,
}

impl AuditLog {
    /// Create a new audit logger scoped to a workspace directory.
    pub fn new(workspace: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let log_path = workspace.into().join(".dscarp").join("audit.log");
        Ok(Self {
            events: Arc::new(RwLock::new(Vec::new())),
            log_path,
            enabled: true,
            retention_days: 30,
        })
    }

    /// Set whether auditing is enabled.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Set retention period in days.
    pub fn set_retention_days(&mut self, days: u32) {
        self.retention_days = days;
    }

    /// Record an audit event.
    pub async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let mut events = self.events.write().await;
        events.push(event);
        Ok(())
    }

    /// Record synchronously (for non-async contexts).
    pub fn record_sync(&self, event: AuditEvent) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        // Use block_in_place when inside async context; otherwise this is fine for sync callers
        let mut events = self.events.blocking_write();
        events.push(event);
        Ok(())
    }

    /// Convenience: record a simple success event.
    pub fn record_success_sync(
        &self,
        event_type: AuditEventType,
        actor: &ActorInfo,
        resource: &ResourceInfo,
        action: &str,
    ) {
        let event = AuditEvent::new(event_type, actor.clone(), resource.clone(), action, AuditOutcome::Success);
        let _ = self.record_sync(event);
    }

    /// Convenience: record a failure event.
    pub fn record_failure_sync(
        &self,
        event_type: AuditEventType,
        actor: &ActorInfo,
        resource: &ResourceInfo,
        action: &str,
        error: &str,
    ) {
        let event = AuditEvent::new(event_type, actor.clone(), resource.clone(), action, AuditOutcome::Failure)
            .with_details(serde_json::json!({ "error": error }));
        let _ = self.record_sync(event);
    }

    /// Convenience: record a blocked event.
    pub fn record_blocked_sync(
        &self,
        event_type: AuditEventType,
        actor: &ActorInfo,
        resource: &ResourceInfo,
        action: &str,
        reason: &str,
    ) {
        let event = AuditEvent::new(event_type, actor.clone(), resource.clone(), action, AuditOutcome::Blocked)
            .with_details(serde_json::json!({ "reason": reason }));
        let _ = self.record_sync(event);
    }

    /// Query events by filters.
    pub async fn query(&self, filters: &AuditQuery) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        let mut results: Vec<AuditEvent> = events
            .iter()
            .filter(|e| {
                if !filters.event_types.is_empty() && !filters.event_types.contains(&e.event_type) {
                    return false;
                }
                if !filters.actors.is_empty() && !filters.actors.contains(&e.actor.identifier) {
                    return false;
                }
                if !filters.resources.is_empty() {
                    let res_match = e.resource.path.as_ref().is_some_and(|p| filters.resources.iter().any(|r| p.contains(r)))
                        || e.resource.name.as_ref().is_some_and(|n| filters.resources.iter().any(|r| n.contains(r)));
                    if !res_match {
                        return false;
                    }
                }
                if !filters.outcomes.is_empty() && !filters.outcomes.contains(&e.outcome) {
                    return false;
                }
                if let Some(since) = filters.since {
                    if e.timestamp < since {
                        return false;
                    }
                }
                if let Some(until) = filters.until {
                    if e.timestamp > until {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        if let Some(limit) = filters.limit {
            results.truncate(limit);
        }

        results
    }

    /// Synchronous query (blocking).
    pub fn query_sync(&self, filters: &AuditQuery) -> Vec<AuditEvent> {
        let events = self.events.blocking_read();
        let mut results: Vec<AuditEvent> = events
            .iter()
            .filter(|e| {
                if !filters.event_types.is_empty() && !filters.event_types.contains(&e.event_type) {
                    return false;
                }
                if !filters.actors.is_empty() && !filters.actors.contains(&e.actor.identifier) {
                    return false;
                }
                if !filters.outcomes.is_empty() && !filters.outcomes.contains(&e.outcome) {
                    return false;
                }
                if let Some(since) = filters.since {
                    if e.timestamp < since {
                        return false;
                    }
                }
                if let Some(until) = filters.until {
                    if e.timestamp > until {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        if let Some(limit) = filters.limit {
            results.truncate(limit);
        }

        results
    }

    /// Get recent N events.
    pub async fn recent(&self, n: usize) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        let start = if events.len() > n { events.len() - n } else { 0 };
        events[start..].to_vec()
    }

    /// Synchronous recent.
    pub fn recent_sync(&self, n: usize) -> Vec<AuditEvent> {
        let events = self.events.blocking_read();
        let start = if events.len() > n { events.len() - n } else { 0 };
        events[start..].to_vec()
    }

    /// Export all events as JSON string.
    pub async fn export_json(&self) -> String {
        let events = self.events.read().await;
        serde_json::to_string_pretty(&*events).unwrap_or_else(|_| "[]".to_string())
    }

    /// Synchronous export.
    pub fn export_json_sync(&self) -> String {
        let events = self.events.blocking_read();
        serde_json::to_string_pretty(&*events).unwrap_or_else(|_| "[]".to_string())
    }

    /// Calculate statistics across all recorded events.
    pub async fn stats(&self) -> AuditStats {
        let events = self.events.read().await;
        self.compute_stats(&events)
    }

    /// Synchronous stats.
    pub fn stats_sync(&self) -> AuditStats {
        let events = self.events.blocking_read();
        self.compute_stats(&events)
    }

    fn compute_stats(&self, events: &[AuditEvent]) -> AuditStats {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut by_event_type: HashMap<String, usize> = HashMap::new();
        let mut by_outcome: HashMap<String, usize> = HashMap::new();
        let mut actor_counts: HashMap<String, usize> = HashMap::new();
        let mut resource_accesses: HashMap<String, usize> = HashMap::new();
        let mut failures_last_24h = 0usize;
        let mut blocked_attempts = 0usize;

        for e in events {
            *by_event_type.entry(e.event_type.as_str().to_string()).or_insert(0) += 1;
            *by_outcome.entry(e.outcome.as_str().to_string()).or_insert(0) += 1;
            *actor_counts.entry(e.actor.identifier.clone()).or_insert(0) += 1;

            let res_key = e.resource
                .path
                .clone()
                .or_else(|| e.resource.name.clone())
                .unwrap_or_else(|| "unknown".to_string());
            *resource_accesses.entry(res_key).or_insert(0) += 1;

            if e.outcome == AuditOutcome::Failure && now_secs.saturating_sub(e.timestamp) < 86400 {
                failures_last_24h += 1;
            }
            if e.outcome == AuditOutcome::Blocked {
                blocked_attempts += 1;
            }
        }

        let most_active_actor = actor_counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(k, _)| k);
        let most_accessed_resource = resource_accesses
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(k, _)| k);

        AuditStats {
            total_events: events.len(),
            by_event_type,
            by_outcome,
            failures_last_24h,
            blocked_attempts,
            most_active_actor,
            most_accessed_resource,
        }
    }

    /// Prune old events beyond the retention period. Returns count removed.
    pub async fn prune(&self) -> anyhow::Result<usize> {
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub((self.retention_days as u64) * 86400))
            .unwrap_or(0);

        let mut events = self.events.write().await;
        let before = events.len();
        events.retain(|e| e.timestamp >= cutoff);
        Ok(before - events.len())
    }

    /// Synchronous prune.
    pub fn prune_sync(&self) -> anyhow::Result<usize> {
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub((self.retention_days as u64) * 86400))
            .unwrap_or(0);

        let mut events = self.events.blocking_write();
        let before = events.len();
        events.retain(|e| e.timestamp >= cutoff);
        Ok(before - events.len())
    }

    /// Get the number of currently stored events.
    pub async fn len(&self) -> usize {
        self.events.read().await.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.events.read().await.is_empty()
    }

    /// Clear all events.
    pub async fn clear(&self) {
        self.events.write().await.clear();
    }

    /// Synchronous clear.
    pub fn clear_sync(&self) {
        self.events.blocking_write().clear();
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_audit_log() -> AuditLog {
        AuditLog::new("d:\\studying\\deepseek-carp\\target\\tmp_audit_test").expect("create audit log")
    }

    #[tokio::test]
    async fn test_audit_log_record_and_query() {
        let log = make_audit_log();
        let actor = ActorInfo::user("test_user");
        let resource = ResourceInfo::file("/tmp/test.rs");

        let event = AuditEvent::new(
            AuditEventType::FileWritten,
            actor,
            resource,
            "wrote test file",
            AuditOutcome::Success,
        );

        log.record(event).await.expect("record event");

        let all = log.query(&AuditQuery::default()).await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].action, "wrote test file");
    }

    #[test]
    fn test_audit_stats_calculation() {
        let log = make_audit_log();
        let actor = ActorInfo::user("alice");
        let resource = ResourceInfo::file("/tmp/a.rs");

        log.record_success_sync(AuditEventType::FileRead, &actor, &resource, "read a.rs");
        log.record_failure_sync(AuditEventType::FileRead, &actor, &resource, "read b.rs", "not found");
        log.record_blocked_sync(AuditEventType::PromptSubmitted, &actor, &resource, "bad prompt", "injection");

        let stats = log.stats_sync();
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.blocked_attempts, 1);
        assert_eq!(stats.most_active_actor.as_deref(), Some("alice"));
    }

    #[test]
    fn test_prune_old_events() {
        let mut log = make_audit_log();
        log.set_retention_days(0); // everything is old

        let actor = ActorInfo::system();
        let resource = ResourceInfo::configuration("test");
        log.record_success_sync(AuditEventType::ConfigChanged, &actor, &resource, "change config");

        let pruned = log.prune_sync().expect("prune");
        assert_eq!(pruned, 1);
    }

    #[test]
    fn test_export_json_valid() {
        let log = make_audit_log();
        let actor = ActorInfo::agent("test-agent");
        let resource = ResourceInfo::model("gpt-4");
        log.record_success_sync(AuditEventType::PromptSubmitted, &actor, &resource, "send prompt");

        let json = log.export_json_sync();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().expect("array").len(), 1);
    }

    #[test]
    fn test_convenience_methods() {
        let log = make_audit_log();
        let actor = ActorInfo::user("bob");
        let resource = ResourceInfo::session("sess-123");

        log.record_success_sync(AuditEventType::SessionCreated, &actor, &resource, "created session");
        log.record_failure_sync(AuditEventType::FileRead, &actor, &resource, "read failed", "ENOENT");
        log.record_blocked_sync(AuditEventType::SanitizationBlocked, &actor, &resource, "block bad input", "xss pattern");

        assert_eq!(log.stats_sync().total_events, 3);
    }

    #[test]
    fn test_actor_info_serialization() {
        let actor = ActorInfo::user("alice");
        let json = serde_json::to_string(&actor).expect("serialize");
        let deserialized: ActorInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.actor_type, ActorType::User);
        assert_eq!(deserialized.identifier, "alice");
    }

    #[test]
    fn test_query_with_filters() {
        let log = make_audit_log();
        let actor = ActorInfo::user("query_user");
        let res_file = ResourceInfo::file("/tmp/x.rs");
        let res_api = ResourceInfo::api_endpoint("/v1/chat");

        log.record_success_sync(AuditEventType::FileRead, &actor, &res_file, "read file");
        log.record_success_sync(AuditEventType::PromptSubmitted, &actor, &res_api, "call api");

        // Query only FileRead events
        let q = AuditQuery {
            event_types: vec![AuditEventType::FileRead],
            ..Default::default()
        };
        let results = log.query_sync(&q);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, AuditEventType::FileRead);
    }

    #[test]
    fn test_recent_events() {
        let log = make_audit_log();
        let actor = ActorInfo::system();
        let resource = ResourceInfo::cache("key1");

        for i in 0..10 {
            log.record_success_sync(AuditEventType::CacheHit, &actor, &resource, &format!("hit {}", i));
        }

        let recent = log.recent_sync(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_resource_info_constructors() {
        let f = ResourceInfo::file("/path/to/file.rs");
        assert_eq!(f.resource_type, ResourceType::File);
        assert_eq!(f.path.as_deref(), Some("/path/to/file.rs"));

        let s = ResourceInfo::session("sess-abc");
        assert_eq!(s.resource_type, ResourceType::Session);
        assert_eq!(s.name.as_deref(), Some("sess-abc"));

        let m = ResourceInfo::model("deepseek-coder");
        assert_eq!(m.resource_type, ResourceType::Model);
    }

    #[test]
    fn test_event_builder() {
        let actor = ActorInfo::plugin("my-plugin");
        let resource = ResourceInfo::transaction("txn-001");
        let event = AuditEvent::new(AuditEventType::BatchEditCommitted, actor, resource, "commit batch", AuditOutcome::Success)
            .with_details(serde_json::json!({ "files_edited": 5 }))
            .with_duration_ms(142)
            .with_session_id("sess-xyz")
            .with_request_id("req-abc");

        assert_eq!(event.duration_ms, 142);
        assert_eq!(event.session_id, "sess-xyz");
        assert_eq!(event.request_id.as_deref(), Some("req-abc"));
        assert!(event.details.get("files_edited").is_some());
    }

    #[test]
    fn test_clear_events() {
        let log = make_audit_log();
        let actor = ActorInfo::system();
        let resource = ResourceInfo::configuration("x");
        log.record_success_sync(AuditEventType::ConfigChanged, &actor, &resource, "change");
        assert_eq!(log.stats_sync().total_events, 1);

        log.clear_sync();
        assert_eq!(log.stats_sync().total_events, 0);
    }

    #[test]
    fn test_disabled_logging() {
        let mut log = make_audit_log();
        log.set_enabled(false);
        let actor = ActorInfo::user("ghost");
        let resource = ResourceInfo::file("/dev/null");
        log.record_success_sync(AuditEventType::FileWritten, &actor, &resource, "should not appear");
        assert_eq!(log.stats_sync().total_events, 0);
    }

    #[test]
    fn test_event_type_as_str_coverage() {
        use AuditEventType::*;
        let variants = [
            ApiKeyUsed, KeyRotated, SessionCreated, SessionEnded,
            FileRead, FileWritten, FileDeleted, BatchEditStarted, BatchEditCommitted, BatchEditRolledBack,
            PromptSubmitted, ResponseReceived, CacheHit, CacheMiss, SanitizationBlocked,
            CostAlertTriggered, BudgetExceeded, ConfigChanged, PluginLoaded, PluginUnloaded,
            SystemStartup, SystemShutdown, ErrorOccurred, RecoveryActionTaken,
        ];
        for v in &variants {
            assert!(!v.as_str().is_empty());
        }
    }
}
