//! Structured logging — JSON-formatted output compatible with OpenTelemetry/ELK/Datadog.

pub mod structured;

pub use structured::{
    StructuredLogger, LogEntry, LogLevel, LogFormat, FieldBuilder, logger,
};
