//! Structured logging — JSON-formatted output compatible with OpenTelemetry/ELK/Datadog.
//!
//! Replaces ad-hoc tracing! calls with structured, machine-parseable logs.
//! Each log entry includes: timestamp, level, module, message, fields, trace_id, span_id.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ─── LogLevel ───────────────────────────────────────────────────

/// Log level following standard severity ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, serde::Serialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }

    /// Parse from a string (case-insensitive).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "TRACE" => Some(Self::Trace),
            "DEBUG" => Some(Self::Debug),
            "INFO" => Some(Self::Info),
            "WARN" => Some(Self::Warn),
            "WARNING" => Some(Self::Warn),
            "ERROR" => Some(Self::Error),
            "ERR" => Some(Self::Error),
            "FATAL" => Some(Self::Fatal),
            _ => None,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ─── LogEntry ──────────────────────────────────────────────────

/// A single structured log entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, serde_json::Value>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

impl LogEntry {
    fn now_iso8601() -> String {
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    pub fn new(
        level: LogLevel,
        target: impl Into<String>,
        message: impl Into<String>,
        fields: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            timestamp: Self::now_iso8601(),
            level: level.as_str().to_string(),
            target: target.into(),
            message: message.into(),
            fields,
            trace_id: None,
            span_id: None,
            file: None,
            line: None,
        }
    }

    pub fn with_trace(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    pub fn with_span(mut self, span_id: impl Into<String>) -> Self {
        self.span_id = Some(span_id.into());
        self
    }

    pub fn with_source(mut self, file: impl Into<String>, line: u32) -> Self {
        self.file = Some(file.into());
        self.line = Some(line);
        self
    }
}

// ─── LogFormat ─────────────────────────────────────────────────

/// Output format for log entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Text,
    OpenTelemetry,
}

impl LogFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
            Self::OpenTelemetry => "otlp",
        }
    }
}

// ─── StructuredLogger ──────────────────────────────────────────

/// The main structured logger.
pub struct StructuredLogger {
    entries: Arc<RwLock<Vec<LogEntry>>>,
    min_level: LogLevel,
    output_format: LogFormat,
    include_source: bool,
}

impl StructuredLogger {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
            min_level: LogLevel::Trace,
            output_format: LogFormat::Json,
            include_source: false,
        }
    }

    pub fn with_min_level(mut self, level: LogLevel) -> Self {
        self.min_level = level;
        self
    }

    pub fn with_format(mut self, fmt: LogFormat) -> Self {
        self.output_format = fmt;
        self
    }

    pub fn with_source_location(mut self, include: bool) -> Self {
        self.include_source = include;
        self
    }

    /// Get current minimum log level.
    pub fn min_level(&self) -> LogLevel {
        self.min_level
    }

    /// Get current output format.
    pub fn format(&self) -> LogFormat {
        self.output_format
    }

    /// Log a message at the given level.
    pub async fn log(
        &self,
        level: LogLevel,
        target: &str,
        message: &str,
        fields: HashMap<String, serde_json::Value>,
    ) {
        if level < self.min_level {
            return;
        }

        let entry = LogEntry::new(level, target, message, fields);

        if self.include_source {
            // Source location would be filled by macros in production; here we leave it None
            // since we can't capture file!() / line!() at this level without macros.
        }

        let mut entries = self.entries.write().await;
        entries.push(entry);
    }

    /// Synchronous log (for non-async contexts).
    pub fn log_sync(
        &self,
        level: LogLevel,
        target: &str,
        message: &str,
        fields: HashMap<String, serde_json::Value>,
    ) {
        if level < self.min_level {
            return;
        }

        let entry = LogEntry::new(level, target, message, fields);
        let mut entries = self.entries.blocking_write();
        entries.push(entry);
    }

    // ── Convenience methods ─────────────────────────────────────

    pub fn trace(&self, target: &str, msg: &str) {
        self.log_sync(LogLevel::Trace, target, msg, HashMap::new());
    }

    pub fn debug(&self, target: &str, msg: &str) {
        self.log_sync(LogLevel::Debug, target, msg, HashMap::new());
    }

    pub fn info(&self, target: &str, msg: &str) {
        self.log_sync(LogLevel::Info, target, msg, HashMap::new());
    }

    pub fn warn(&self, target: &str, msg: &str) {
        self.log_sync(LogLevel::Warn, target, msg, HashMap::new());
    }

    pub fn error(&self, target: &str, msg: &str) {
        self.log_sync(LogLevel::Error, target, msg, HashMap::new());
    }

    pub fn fatal(&self, target: &str, msg: &str) {
        self.log_sync(LogLevel::Fatal, target, msg, HashMap::new());
    }

    // ── Field builder pattern ───────────────────────────────────

    /// Start building a log entry with extra fields.
    pub fn with_fields(&self, level: LogLevel, target: &str, msg: &str) -> FieldBuilder<'_> {
        FieldBuilder {
            logger: self,
            level,
            target: target.to_string(),
            message: msg.to_string(),
            fields: HashMap::new(),
        }
    }

    // ── Query / export ──────────────────────────────────────────

    /// Get all logged entries (for testing/export).
    pub async fn entries(&self) -> Vec<LogEntry> {
        self.entries.read().await.clone()
    }

    /// Synchronous entries accessor.
    pub fn entries_sync(&self) -> Vec<LogEntry> {
        self.entries.blocking_read().clone()
    }

    /// Export all entries as a single JSON array string.
    pub async fn export_json(&self) -> String {
        let entries = self.entries.read().await;
        serde_json::to_string_pretty(&*entries).unwrap_or_else(|_| "[]".to_string())
    }

    /// Synchronous JSON export.
    pub fn export_json_sync(&self) -> String {
        let entries = self.entries.blocking_read();
        serde_json::to_string_pretty(&*entries).unwrap_or_else(|_| "[]".to_string())
    }

    /// Export in OpenTelemetry OTLP-compatible JSON format.
    pub async fn export_otlp(&self) -> String {
        let entries = self.entries.read().await;
        Self::format_otlp(&entries)
    }

    /// Synchronous OTLP export.
    pub fn export_otlp_sync(&self) -> String {
        let entries = self.entries.blocking_read();
        Self::format_otlp(&entries)
    }

    fn format_otlp(entries: &[LogEntry]) -> String {
        let resource_logs: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                let mut body_fields = serde_json::Map::new();
                body_fields.insert("message".into(), serde_json::json!(e.message));
                for (k, v) in &e.fields {
                    body_fields.insert(k.clone(), v.clone());
                }

                serde_json::json!({
                    "traceId": e.trace_id.as_deref().unwrap_or(""),
                    "spanId": e.span_id.as_deref().unwrap_or(""),
                    "timeUnixNano": 0,
                    "severityNumber": severity_to_otlp_number(&e.level),
                    "severityText": e.level,
                    "body": { "stringValue": e.message },
                    "attributes": e.fields,
                })
            })
            .collect();

        let otlp = serde_json::json!({
            "resourceLogs": [{
                "resource": {},
                "scopeLogs": [{
                    "scope": {"name": "deepseek-carp"},
                    "logRecords": resource_logs,
                }],
            }],
        });

        serde_json::to_string_pretty(&otlp).unwrap_or_else(|_| "{}".to_string())
    }

    /// Clear all stored entries.
    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }

    /// Synchronous clear.
    pub fn clear_sync(&self) {
        self.entries.blocking_write().clear();
    }

    /// Count total entries.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Check if no entries exist.
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    /// Count entries grouped by log level.
    pub async fn counts_by_level(&self) -> HashMap<String, usize> {
        let entries = self.entries.read().await;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for e in entries.iter() {
            *counts.entry(e.level.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Synchronous counts_by_level.
    pub fn counts_by_level_sync(&self) -> HashMap<String, usize> {
        let entries = self.entries.blocking_read();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for e in entries.iter() {
            *counts.entry(e.level.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Format a single entry according to the configured output format.
    pub fn format_entry(&self, entry: &LogEntry) -> String {
        match self.output_format {
            LogFormat::Json => serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string()),
            LogFormat::Text => format!(
                "[{}] {} {}: {}",
                entry.timestamp, entry.level, entry.target, entry.message
            ),
            LogFormat::OpenTelemetry => {
                // Single-entry OTLP wrapper
                let otlp = serde_json::json!({
                    "resourceLogs": [{
                        "resource": {},
                        "scopeLogs": [{
                            "scope": {"name": "deepseek-carp"},
                            "logRecords": [{
                                "traceId": entry.trace_id.as_deref().unwrap_or(""),
                                "spanId": entry.span_id.as_deref().unwrap_or(""),
                                "severityText": entry.level,
                                "body": {"stringValue": entry.message},
                                "attributes": entry.fields,
                            }],
                        }],
                    }],
                });
                serde_json::to_string(&otlp).unwrap_or_else(|_| "{}".to_string())
            }
        }
    }
}

impl Default for StructuredLogger {
    fn default() -> Self {
        Self::new()
    }
}

fn severity_to_otlp_number(level: &str) -> i32 {
    match level {
        "TRACE" | "DEBUG" => 1,
        "INFO" => 9,
        "WARN" => 13,
        "ERROR" => 17,
        "FATAL" | _ => 21,
    }
}

// ─── FieldBuilder ───────────────────────────────────────────────

/// Builder for adding fields to a log entry before sending.
pub struct FieldBuilder<'a> {
    logger: &'a StructuredLogger,
    level: LogLevel,
    target: String,
    message: String,
    fields: HashMap<String, serde_json::Value>,
}

impl<'a> FieldBuilder<'a> {
    /// Add a field to the log entry.
    pub fn field(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.fields.insert(key.to_string(), value.into());
        self
    }

    /// Finalize and emit the log entry.
    pub fn send(self) {
        self.logger.log_sync(self.level, &self.target, &self.message, self.fields);
    }
}

// ─── Global singleton ───────────────────────────────────────────

/// Global logger state protected by a OnceLock.
static GLOBAL_LOGGER_INIT: std::sync::OnceLock<StructuredLogger> = std::sync::OnceLock::new();

/// Access the global logger instance.
pub fn logger() -> &'static StructuredLogger {
    GLOBAL_LOGGER_INIT.get_or_init(StructuredLogger::new)
}

// ─── Macros ─────────────────────────────────────────────────────

/// Log an INFO-level message through the global structured logger.
#[macro_export]
macro_rules! log_info {
    ($target:expr, $msg:expr $(,)?) => {
        $crate::logging::structured::logger().info($target, $msg)
    };
    ($target:expr, $msg:expr, $($key:ident = $value:expr),* $(,)?) => {{
        let mut f = std::collections::HashMap::new();
        $( f.insert(stringify!($key).into(), serde_json::json!($value)); )*
        $crate::logging::structured::logger().log_sync(
            $crate::logging::structured::LogLevel::Info,
            $target,
            $msg,
            f,
        );
    }};
}

/// Log a WARN-level message through the global structured logger.
#[macro_export]
macro_rules! log_warn {
    ($target:expr, $msg:expr $(,)?) => {
        $crate::logging::structured::logger().warn($target, $msg)
    };
    ($target:expr, $msg:expr, $($key:ident = $value:expr),* $(,)?) => {{
        let mut f = std::collections::HashMap::new();
        $( f.insert(stringify!($key).into(), serde_json::json!($value)); )*
        $crate::logging::structured::logger().log_sync(
            $crate::logging::structured::LogLevel::Warn,
            $target,
            $msg,
            f,
        );
    }};
}

/// Log an ERROR-level message through the global structured logger.
#[macro_export]
macro_rules! log_error {
    ($target:expr, $msg:expr $(,)?) => {
        $crate::logging::structured::logger().error($target, $msg)
    };
    ($target:expr, $msg:expr, $($key:ident = $value:expr),* $(,)?) => {{
        let mut f = std::collections::HashMap::new();
        $( f.insert(stringify!($key).into(), serde_json::json!($value)); )*
        $crate::logging::structured::logger().log_sync(
            $crate::logging::structured::LogLevel::Error,
            $target,
            $msg,
            f,
        );
    }};
}

/// Log a DEBUG-level message through the global structured logger.
#[macro_export]
macro_rules! log_debug {
    ($target:expr, $msg:expr $(,)?) => {
        $crate::logging::structured::logger().debug($target, $msg)
    };
    ($target:expr, $msg:expr, $($key:ident = $value:expr),* $(,)?) => {{
        let mut f = std::collections::HashMap::new();
        $( f.insert(stringify!($key).into(), serde_json::json!($value)); )*
        $crate::logging::structured::logger().log_sync(
            $crate::logging::structured::LogLevel::Debug,
            $target,
            $msg,
            f,
        );
    }};
}

/// Log a TRACE-level message through the global structured logger.
#[macro_export]
macro_rules! log_trace {
    ($target:expr, $msg:expr $(,)?) => {
        $crate::logging::structured::logger().trace($target, $msg)
    };
    ($target:expr, $msg:expr, $($key:ident = $value:expr),* $(,)?) => {{
        let mut f = std::collections::HashMap::new();
        $( f.insert(stringify!($key).into(), serde_json::json!($value)); )*
        $crate::logging::structured::logger().log_sync(
            $crate::logging::structured::LogLevel::Trace,
            $target,
            $msg,
            f,
        );
    }};
}

/// Log a FATAL-level message through the global structured logger.
#[macro_export]
macro_rules! log_fatal {
    ($target:expr, $msg:expr $(,)?) => {
        $crate::logging::structured::logger().fatal($target, $msg)
    };
    ($target:expr, $msg:expr, $($key:ident = $value:expr),* $(,)?) => {{
        let mut f = std::collections::HashMap::new();
        $( f.insert(stringify!($key).into(), serde_json::json!($value)); )*
        $crate::logging::structured::logger().log_sync(
            $crate::logging::structured::LogLevel::Fatal,
            $target,
            $msg,
            f,
        );
    }};
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_logger() -> StructuredLogger {
        StructuredLogger::new()
    }

    #[test]
    fn test_basic_logging() {
        let log = fresh_logger();
        log.info("test_module", "hello world");
        let entries = log.entries_sync();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "hello world");
        assert_eq!(entries[0].level, "INFO");
        assert_eq!(entries[0].target, "test_module");
    }

    #[test]
    fn test_json_output_valid() {
        let log = fresh_logger();
        log.error("my_mod", "something went wrong");
        let json = log.export_json_sync();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed.is_array());
        let arr = parsed.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["message"], "something went wrong");
        assert_eq!(arr[0]["level"], "ERROR");
    }

    #[test]
    fn test_field_builder() {
        let log = fresh_logger();
        log.with_fields(LogLevel::Info, "mod_a", "user login")
            .field("user_id", 42)
            .field("ip", "10.0.0.1")
            .field("success", true)
            .send();

        let entries = log.entries_sync();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].fields.get("user_id").unwrap(), &42);
        assert_eq!(entries[0].fields.get("ip").unwrap(), "10.0.0.1");
        assert_eq!(entries[0].fields.get("success").unwrap(), true);
    }

    #[test]
    fn test_level_filtering() {
        let log = StructuredLogger::new().with_min_level(LogLevel::Warn);
        log.info("m", "should be filtered");
        log.debug("m", "also filtered");
        log.warn("m", "should appear");
        log.error("m", "should also appear");

        let entries = log.entries_sync();
        assert_eq!(entries.len(), 2); // only warn + error
        assert!(entries.iter().all(|e| e.level == "WARN" || e.level == "ERROR"));
    }

    #[test]
    fn test_counts_by_level() {
        let log = fresh_logger();
        log.info("m", "a");
        log.info("m", "b");
        log.error("m", "c");
        log.warn("m", "d");

        let counts = log.counts_by_level_sync();
        assert_eq!(*counts.get("INFO").unwrap_or(&0), 2);
        assert_eq!(*counts.get("ERROR").unwrap_or(&0), 1);
        assert_eq!(*counts.get("WARN").unwrap_or(&0), 1);
    }

    #[test]
    fn test_otlp_export_format() {
        let log = fresh_logger();
        log.with_fields(LogLevel::Info, "svc", "request processed")
            .field("latency_ms", 12)
            .field("status_code", 200)
            .send();

        let otlp = log.export_otlp_sync();
        let parsed: serde_json::Value = serde_json::from_str(&otlp).expect("valid OTLP JSON");
        assert!(parsed["resourceLogs"].is_array());
    }

    #[test]
    fn test_global_logger_singleton() {
        let l1 = logger() as *const StructuredLogger;
        let l2 = logger() as *const StructuredLogger;
        assert_eq!(l1, l2, "global logger should be singleton");
    }

    #[test]
    fn test_clear_entries() {
        let log = fresh_logger();
        log.info("m", "entry 1");
        log.info("m", "entry 2");
        assert_eq!(log.entries_sync().len(), 2);

        log.clear_sync();
        assert_eq!(log.entries_sync().len(), 0);
        assert!(log.entries_sync().is_empty());
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
        assert!(LogLevel::Error < LogLevel::Fatal);
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str("info"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str("ERROR"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("WARN"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("WARNING"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("unknown"), None);
    }

    #[test]
    fn test_log_entry_with_context() {
        let entry = LogEntry::new(LogLevel::Error, "my_mod", "oops", HashMap::new())
            .with_trace("trace-abc")
            .with_span("span-xyz")
            .with_source("src/main.rs", 42);

        assert_eq!(entry.trace_id.as_deref(), Some("trace-abc"));
        assert_eq!(entry.span_id.as_deref(), Some("span-xyz"));
        assert_eq!(entry.file.as_deref(), Some("src/main.rs"));
        assert_eq!(entry.line, Some(42));
    }

    #[test]
    fn test_text_format() {
        let log = StructuredLogger::new().with_format(LogFormat::Text);
        log.info("mod_test", "text msg");
        let entries = log.entries_sync();
        let formatted = log.format_entry(&entries[0]);
        assert!(formatted.contains("["));
        assert!(formatted.contains("INFO"));
        assert!(formatted.contains("mod_test"));
        assert!(formatted.contains("text msg"));
    }

    #[test]
    fn test_timestamp_format() {
        let log = fresh_logger();
        log.info("t", "ts test");
        let entries = log.entries_sync();
        // ISO-8601 should contain 'T' separator and end with Z
        assert!(entries[0].timestamp.contains('T'));
        assert!(entries[0].timestamp.ends_with('Z'));
    }

    #[tokio::test]
    async fn test_async_logging() {
        let log = fresh_logger();
        log.log(LogLevel::Info, "async_mod", "async msg", HashMap::new()).await;
        let entries = log.entries().await;
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_async_clear_and_len() {
        let log = fresh_logger();
        log.info("m", "x");
        assert_eq!(log.len().await, 1);
        log.clear().await;
        assert!(log.is_empty().await);
    }

    #[test]
    fn test_all_convenience_methods() {
        let log = fresh_logger();
        log.trace("m", "trace");
        log.debug("m", "debug");
        log.info("m", "info");
        log.warn("m", "warn");
        log.error("m", "error");
        log.fatal("m", "fatal");

        let entries = log.entries_sync();
        assert_eq!(entries.len(), 6);

        let levels: Vec<&str> = entries.iter().map(|e| e.level.as_str()).collect();
        assert!(levels.contains(&"TRACE"));
        assert!(levels.contains(&"DEBUG"));
        assert!(levels.contains(&"INFO"));
        assert!(levels.contains(&"WARN"));
        assert!(levels.contains(&"ERROR"));
        assert!(levels.contains(&"FATAL"));
    }

    #[test]
    fn test_builder_multiple_fields_chain() {
        let log = fresh_logger();
        log.with_fields(LogLevel::Debug, "chain_test", "chained")
            .field("a", 1)
            .field("b", "two")
            .field("c", vec![1, 2, 3])
            .field("d", serde_json::json!({"nested": true}))
            .send();

        let entries = log.entries_sync();
        assert_eq!(entries[0].fields.len(), 4);
        assert_eq!(entries[0].fields.get("a").unwrap(), &1);
        assert_eq!(entries[0].fields.get("b").unwrap(), "two");
    }

    #[test]
    fn test_log_format_as_str() {
        assert_eq!(LogFormat::Json.as_str(), "json");
        assert_eq!(LogFormat::Text.as_str(), "text");
        assert_eq!(LogFormat::OpenTelemetry.as_str(), "otlp");
    }
}
