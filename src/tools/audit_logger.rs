//! Audit Logger - Operation tracking and traceability.
//!
//! This module provides:
//! - Event type classification
//! - Multi-dimensional query support
//! - Immutable audit trail
//! - Compliance reporting

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuditEventType {
    MessageSend,
    MessageReceive,
    ToolCall,
    CacheHit,
    CacheMiss,
    ConfigurationChange,
    Authentication,
    Authorization,
    Error,
    Performance,
    SystemStart,
    SystemShutdown,
}

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditEventType::MessageSend => write!(f, "MESSAGE_SEND"),
            AuditEventType::MessageReceive => write!(f, "MESSAGE_RECEIVE"),
            AuditEventType::ToolCall => write!(f, "TOOL_CALL"),
            AuditEventType::CacheHit => write!(f, "CACHE_HIT"),
            AuditEventType::CacheMiss => write!(f, "CACHE_MISS"),
            AuditEventType::ConfigurationChange => write!(f, "CONFIG_CHANGE"),
            AuditEventType::Authentication => write!(f, "AUTHENTICATION"),
            AuditEventType::Authorization => write!(f, "AUTHORIZATION"),
            AuditEventType::Error => write!(f, "ERROR"),
            AuditEventType::Performance => write!(f, "PERFORMANCE"),
            AuditEventType::SystemStart => write!(f, "SYSTEM_START"),
            AuditEventType::SystemShutdown => write!(f, "SYSTEM_SHUTDOWN"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub event_type: AuditEventType,
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub user_id: Option<String>,
    pub details: HashMap<String, String>,
    pub metadata: HashMap<String, String>,
    pub severity: AuditSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuditSeverity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl std::fmt::Display for AuditSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditSeverity::Debug => write!(f, "DEBUG"),
            AuditSeverity::Info => write!(f, "INFO"),
            AuditSeverity::Warning => write!(f, "WARNING"),
            AuditSeverity::Error => write!(f, "ERROR"),
            AuditSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditQuery {
    pub event_types: Option<HashSet<AuditEventType>>,
    pub session_ids: Option<HashSet<String>>,
    pub user_ids: Option<HashSet<String>>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub severities: Option<HashSet<AuditSeverity>>,
    pub limit: Option<usize>,
}

impl Default for AuditQuery {
    fn default() -> Self {
        Self {
            event_types: None,
            session_ids: None,
            user_ids: None,
            start_time: None,
            end_time: None,
            severities: None,
            limit: Some(100),
        }
    }
}

pub struct AuditLogger {
    events: Arc<Mutex<Vec<AuditEvent>>>,
    log_path: Option<String>,
    writer: Arc<Mutex<Option<BufWriter<File>>>>,
    max_events: usize,
}

impl AuditLogger {
    pub fn new(log_path: Option<&str>, max_events: usize) -> Self {
        let mut writer = None;
        
        if let Some(path) = log_path {
            if let Ok(file) = File::create(path) {
                writer = Some(BufWriter::new(file));
            }
        }

        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            log_path: log_path.map(|s| s.to_string()),
            writer: Arc::new(Mutex::new(writer)),
            max_events,
        }
    }

    pub fn log(&self, event_type: AuditEventType, session_id: &str, details: HashMap<String, String>) {
        let event = AuditEvent {
            id: self.generate_id(),
            event_type,
            timestamp: Utc::now(),
            session_id: session_id.to_string(),
            user_id: None,
            details,
            metadata: HashMap::new(),
            severity: self.get_severity(event_type),
        };

        let mut events = self.events.lock().expect("mutex poisoned: audit_logger.rs:146");
        if events.len() >= self.max_events {
            events.remove(0);
        }
        events.push(event.clone());

        self.write_to_file(&event);
    }

    pub fn log_with_user(&self, event_type: AuditEventType, session_id: &str, user_id: &str, details: HashMap<String, String>) {
        let event = AuditEvent {
            id: self.generate_id(),
            event_type,
            timestamp: Utc::now(),
            session_id: session_id.to_string(),
            user_id: Some(user_id.to_string()),
            details,
            metadata: HashMap::new(),
            severity: self.get_severity(event_type),
        };

        let mut events = self.events.lock().expect("mutex poisoned: audit_logger.rs:167");
        if events.len() >= self.max_events {
            events.remove(0);
        }
        events.push(event.clone());

        self.write_to_file(&event);
    }

    pub fn query(&self, query: AuditQuery) -> Vec<AuditEvent> {
        let events = self.events.lock().expect("mutex poisoned: audit_logger.rs:177");
        
        let mut results = events
            .iter()
            .filter(|e| {
                if let Some(ref types) = query.event_types {
                    if !types.contains(&e.event_type) {
                        return false;
                    }
                }
                if let Some(ref sessions) = query.session_ids {
                    if !sessions.contains(&e.session_id) {
                        return false;
                    }
                }
                if let Some(ref users) = query.user_ids {
                    if let Some(ref user_id) = e.user_id {
                        if !users.contains(user_id) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                if let Some(ref start) = query.start_time {
                    if e.timestamp < *start {
                        return false;
                    }
                }
                if let Some(ref end) = query.end_time {
                    if e.timestamp > *end {
                        return false;
                    }
                }
                if let Some(ref sevs) = query.severities {
                    if !sevs.contains(&e.severity) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect::<Vec<_>>();

        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        results
    }

    pub fn get_event_count(&self) -> usize {
        self.events.lock().expect("mutex poisoned: audit_logger.rs:231").len()
    }

    pub fn export_to_file(&self, path: &str) -> std::io::Result<()> {
        let events = self.events.lock().expect("mutex poisoned: audit_logger.rs:235");
        let content = serde_json::to_string_pretty(&*events)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn generate_summary(&self) -> AuditSummary {
        let events = self.events.lock().expect("mutex poisoned: audit_logger.rs:242");
        
        let mut event_type_counts = HashMap::new();
        let mut severity_counts = HashMap::new();
        let mut session_counts = HashMap::new();

        for event in events.iter() {
            *event_type_counts.entry(event.event_type).or_insert(0) += 1;
            *severity_counts.entry(event.severity).or_insert(0) += 1;
            *session_counts.entry(event.session_id.clone()).or_insert(0) += 1;
        }

        AuditSummary {
            total_events: events.len(),
            event_type_counts,
            severity_counts,
            unique_sessions: session_counts.len(),
            first_event: events.first().map(|e| e.timestamp),
            last_event: events.last().map(|e| e.timestamp),
        }
    }

    fn generate_id(&self) -> String {
        format!(
            "audit_{}_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0),
            rand::random::<u64>()
        )
    }

    fn get_severity(&self, event_type: AuditEventType) -> AuditSeverity {
        match event_type {
            AuditEventType::Error | AuditEventType::SystemShutdown => AuditSeverity::Error,
            AuditEventType::Authentication | AuditEventType::Authorization => AuditSeverity::Warning,
            AuditEventType::SystemStart => AuditSeverity::Info,
            _ => AuditSeverity::Debug,
        }
    }

    fn write_to_file(&self, event: &AuditEvent) {
        let mut writer = self.writer.lock().expect("mutex poisoned: audit_logger.rs:282");
        if let Some(ref mut w) = *writer {
            let line = format!("{} {}\n", event.timestamp, serde_json::to_string(event).unwrap_or_default());
            let _ = w.write_all(line.as_bytes());
            let _ = w.flush();
        }
    }

    /// Get the log file path for this audit logger.
    pub fn log_path(&self) -> Option<&str> {
        self.log_path.as_deref()
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(None, 10000)
    }
}

#[derive(Debug, Clone)]
pub struct AuditSummary {
    pub total_events: usize,
    pub event_type_counts: HashMap<AuditEventType, usize>,
    pub severity_counts: HashMap<AuditSeverity, usize>,
    pub unique_sessions: usize,
    pub first_event: Option<DateTime<Utc>>,
    pub last_event: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_event() {
        let logger = AuditLogger::default();
        let mut details = HashMap::new();
        details.insert("message".to_string(), "test".to_string());
        
        logger.log(AuditEventType::MessageSend, "session1", details);
        
        let events = logger.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, AuditEventType::MessageSend);
    }

    #[test]
    fn test_query_events() {
        let logger = AuditLogger::default();
        
        let mut details1 = HashMap::new();
        details1.insert("message".to_string(), "test1".to_string());
        logger.log(AuditEventType::MessageSend, "session1", details1);

        let mut details2 = HashMap::new();
        details2.insert("message".to_string(), "test2".to_string());
        logger.log(AuditEventType::MessageReceive, "session1", details2);

        let query = AuditQuery {
            event_types: Some(HashSet::from([AuditEventType::MessageSend])),
            ..Default::default()
        };

        let results = logger.query(query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, AuditEventType::MessageSend);
    }

    #[test]
    fn test_generate_summary() {
        let logger = AuditLogger::default();
        
        let mut details = HashMap::new();
        details.insert("message".to_string(), "test".to_string());
        
        logger.log(AuditEventType::MessageSend, "session1", details.clone());
        logger.log(AuditEventType::ToolCall, "session1", details.clone());
        logger.log(AuditEventType::MessageReceive, "session2", details);

        let summary = logger.generate_summary();
        assert_eq!(summary.total_events, 3);
        assert_eq!(summary.unique_sessions, 2);
    }
}
