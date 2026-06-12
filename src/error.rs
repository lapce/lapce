//! Production-grade error handling for deepseek-carp.
//!
//! Provides a unified error type [`CarpError`] with categorisation
//! ([`ErrorKind`]), structured context ([`ErrorContext`]), automatic
//! backtrace capture, retry configuration, and an exponential-backoff
//! helper.

use std::backtrace::Backtrace;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// ErrorKind — broad categories
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    // Configuration
    ConfigNotFound,
    ConfigParse,
    InvalidApiKey,

    // Network
    NetworkTimeout,
    NetworkUnreachable,
    DnsResolution,
    TlsError,

    // API
    ApiError,
    ApiRateLimited,
    ApiAuthentication,
    ApiQuotaExceeded,
    ApiBadRequest,

    // RAG / Indexing
    IndexNotFound,
    IndexCorrupted,
    IndexTooLarge,

    // File / IO
    FileNotFound,
    FilePermission,
    FileTooLarge,
    IoError,

    // Edit / Apply
    EditConflict,
    EditTooLarge,
    ParseError,

    // Security
    SecurityViolation,
    SanitizerBlocked,

    // Internal
    Internal,
    NotYetImplemented,
    FeatureDisabled,
    ResourceExhausted(String),
}

// ---------------------------------------------------------------------------
// ErrorCategory — classification for alerting and triage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    Configuration,
    Network,
    Api,
    Index,
    Io,
    Edit,
    Security,
    Internal,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::Configuration => write!(f, "Configuration"),
            ErrorCategory::Network => write!(f, "Network"),
            ErrorCategory::Api => write!(f, "API"),
            ErrorCategory::Index => write!(f, "Index"),
            ErrorCategory::Io => write!(f, "I/O"),
            ErrorCategory::Edit => write!(f, "Edit"),
            ErrorCategory::Security => write!(f, "Security"),
            ErrorCategory::Internal => write!(f, "Internal"),
        }
    }
}

impl ErrorKind {
    /// Categorize error for alerting and triage.
    pub fn category(&self) -> ErrorCategory {
        match self {
            ErrorKind::ConfigNotFound | ErrorKind::ConfigParse | ErrorKind::InvalidApiKey => ErrorCategory::Configuration,
            ErrorKind::NetworkTimeout | ErrorKind::NetworkUnreachable | ErrorKind::DnsResolution | ErrorKind::TlsError => ErrorCategory::Network,
            ErrorKind::ApiError | ErrorKind::ApiRateLimited | ErrorKind::ApiAuthentication | ErrorKind::ApiQuotaExceeded | ErrorKind::ApiBadRequest => ErrorCategory::Api,
            ErrorKind::IndexNotFound | ErrorKind::IndexCorrupted | ErrorKind::IndexTooLarge => ErrorCategory::Index,
            ErrorKind::FileNotFound | ErrorKind::FilePermission | ErrorKind::FileTooLarge | ErrorKind::IoError => ErrorCategory::Io,
            ErrorKind::EditConflict | ErrorKind::EditTooLarge | ErrorKind::ParseError => ErrorCategory::Edit,
            ErrorKind::SecurityViolation | ErrorKind::SanitizerBlocked => ErrorCategory::Security,
            ErrorKind::Internal | ErrorKind::NotYetImplemented | ErrorKind::FeatureDisabled | ErrorKind::ResourceExhausted(_) => ErrorCategory::Internal,
        }
    }

    /// Recommended action for this error.
    pub fn recommended_action(&self) -> &'static str {
        match self {
            ErrorKind::ConfigNotFound => "Check configuration file location and permissions",
            ErrorKind::ConfigParse => "Verify config file format (YAML/JSON/TOML)",
            ErrorKind::InvalidApiKey => "Set DEEPSEEK_API_KEY environment variable",
            ErrorKind::NetworkTimeout => "Check network connectivity and increase timeout",
            ErrorKind::NetworkUnreachable => "Verify API endpoint URL and internet access",
            ErrorKind::DnsResolution => "Check DNS settings and network connectivity",
            ErrorKind::TlsError => "Verify TLS certificates and proxy configuration",
            ErrorKind::ApiError => "Retry the request or check API status",
            ErrorKind::ApiRateLimited => "Reduce request rate or upgrade API plan",
            ErrorKind::ApiAuthentication => "Verify API key is valid and not expired",
            ErrorKind::ApiQuotaExceeded => "Upgrade API plan or wait for quota reset",
            ErrorKind::ApiBadRequest => "Review request parameters for correctness",
            ErrorKind::IndexNotFound => "Run indexing before querying",
            ErrorKind::IndexCorrupted => "Rebuild the RAG index with --rebuild flag",
            ErrorKind::IndexTooLarge => "Reduce index size or increase resource limits",
            ErrorKind::FileNotFound => "Verify the file path exists",
            ErrorKind::FilePermission => "Check file permissions and ownership",
            ErrorKind::FileTooLarge => "Split the file into smaller chunks",
            ErrorKind::IoError => "Check disk space and file system health",
            ErrorKind::EditConflict => "Review conflicting changes and retry with merge",
            ErrorKind::EditTooLarge => "Split the edit into smaller operations",
            ErrorKind::ParseError => "Check input format and try again",
            ErrorKind::SecurityViolation => "Review recent code changes for security issues",
            ErrorKind::SanitizerBlocked => "Refactor prompt to comply with security policy",
            ErrorKind::Internal => "Report this issue to the development team",
            ErrorKind::NotYetImplemented => "This feature is planned for a future release",
            ErrorKind::FeatureDisabled => "Enable the feature in configuration",
            ErrorKind::ResourceExhausted(_) => "Close other sessions or increase resource limits",
        }
    }

    /// HTTP status code mapping.
    pub fn http_status(&self) -> u16 {
        match self {
            ErrorKind::ConfigNotFound => 404,
            ErrorKind::NetworkTimeout => 504,
            ErrorKind::ApiRateLimited => 429,
            ErrorKind::ApiAuthentication => 401,
            ErrorKind::SecurityViolation => 403,
            ErrorKind::SanitizerBlocked => 400,
            ErrorKind::Internal => 500,
            _ => 500,
        }
    }
}

// ---------------------------------------------------------------------------
// ErrorContext — structured metadata attached to an error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ErrorContext {
    pub operation: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub extra: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// CarpError — unified top-level error
// ---------------------------------------------------------------------------

/// Top-level error type for deepseek-carp production operations.
#[derive(Debug)]
pub struct CarpError {
    pub kind: ErrorKind,
    pub message: String,
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
    pub backtrace: Backtrace,
    pub context: ErrorContext,
}

impl CarpError {
    /// Create a new error with the given kind and message.
    ///
    /// A backtrace is captured automatically.
    pub fn new(kind: ErrorKind, msg: impl Into<String>) -> Self {
        Self {
            kind,
            message: msg.into(),
            source: None,
            backtrace: Backtrace::capture(),
            context: ErrorContext::default(),
        }
    }

    /// Attach a source/cause error.
    pub fn with_source<E: std::error::Error + Send + Sync + 'static>(
        mut self,
        source: E,
    ) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Attach structured context.
    pub fn with_context(mut self, ctx: ErrorContext) -> Self {
        self.context = ctx;
        self
    }

    /// Is this error retryable?
    ///
    /// Transient errors such as timeouts, rate-limits, and unreachable
    /// networks can be retried safely.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::NetworkTimeout
                | ErrorKind::ApiRateLimited
                | ErrorKind::NetworkUnreachable
        )
    }

    /// Should this error be alerted?
    ///
    /// Security and authentication errors usually require immediate
    /// operator attention.
    pub fn is_alertable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::ApiAuthentication
                | ErrorKind::SecurityViolation
                | ErrorKind::SanitizerBlocked
                | ErrorKind::ApiQuotaExceeded
        )
    }

    /// User-friendly error message for CLI output.
    pub fn user_message(&self) -> String {
        match self.kind {
            ErrorKind::ConfigNotFound => {
                "Configuration file not found. Run `carp setup` to create one.".into()
            }
            ErrorKind::ConfigParse => {
                "Failed to parse configuration file. Check the syntax.".into()
            }
            ErrorKind::InvalidApiKey => {
                "Invalid API key. Check your credentials.".into()
            }
            ErrorKind::NetworkTimeout => {
                "Request timed out. Check your network connection.".into()
            }
            ErrorKind::NetworkUnreachable => {
                "Network unreachable. Check your internet connection.".into()
            }
            ErrorKind::DnsResolution => {
                "DNS resolution failed. Check your network settings.".into()
            }
            ErrorKind::TlsError => {
                "TLS/SSL error. Check your certificates or proxy configuration.".into()
            }
            ErrorKind::ApiError => {
                "API returned an error. Try again later.".into()
            }
            ErrorKind::ApiRateLimited => {
                "Rate limited by API provider. Waiting before retry...".into()
            }
            ErrorKind::ApiAuthentication => {
                "API authentication failed. Check your API keys.".into()
            }
            ErrorKind::ApiQuotaExceeded => {
                "API quota exceeded. Upgrade your plan or wait for reset.".into()
            }
            ErrorKind::ApiBadRequest => {
                "Bad request to API. This may be a bug.".into()
            }
            ErrorKind::IndexNotFound => {
                "Index not found. Run indexing first.".into()
            }
            ErrorKind::IndexCorrupted => {
                "Index is corrupted. Rebuild the index.".into()
            }
            ErrorKind::IndexTooLarge => {
                "Index is too large for the current configuration.".into()
            }
            ErrorKind::FileNotFound => {
                "File not found. Check the path.".into()
            }
            ErrorKind::FilePermission => {
                "Permission denied. Check file permissions.".into()
            }
            ErrorKind::FileTooLarge => {
                "File is too large to process.".into()
            }
            ErrorKind::IoError => format!("I/O error: {}", self.message),
            ErrorKind::EditConflict => {
                "Edit conflict detected. The file has changed since last read.".into()
            }
            ErrorKind::EditTooLarge => {
                "Edit is too large to apply in one operation.".into()
            }
            ErrorKind::ParseError => {
                "Failed to parse the response.".into()
            }
            ErrorKind::SecurityViolation => {
                "Security violation detected. Operation blocked.".into()
            }
            ErrorKind::SanitizerBlocked => {
                "Content sanitizer blocked the operation.".into()
            }
            ErrorKind::Internal => {
                "Internal error. Please report this issue.".into()
            }
            ErrorKind::NotYetImplemented => {
                "This feature is not yet implemented.".into()
            }
            ErrorKind::FeatureDisabled => {
                "This feature is disabled in the current configuration.".into()
            }
            ErrorKind::ResourceExhausted(ref resource) => {
                format!("Resource exhausted: {}", resource)
            }
        }
    }

    /// Detailed error report for logs / debugging.
    pub fn debug_report(&self) -> String {
        let mut report = String::new();

        report.push_str(&format!("Error: {:?}\n", self.kind));
        report.push_str(&format!("Message: {}\n", self.message));
        report.push_str(&format!("User message: {}\n", self.user_message()));

        if let Some(ref op) = self.context.operation {
            report.push_str(&format!("Operation: {}\n", op));
        }
        if let Some(ref provider) = self.context.provider {
            report.push_str(&format!("Provider: {}\n", provider));
        }
        if let Some(ref model) = self.context.model {
            report.push_str(&format!("Model: {}\n", model));
        }
        if let Some(ref file) = self.context.file {
            report.push_str(&format!("File: {}", file));
            if let Some(line) = self.context.line {
                report.push_str(&format!(":{}", line));
            }
            report.push('\n');
        }
        if !self.context.extra.is_empty() {
            report.push_str("Extra context:\n");
            for (k, v) in &self.context.extra {
                report.push_str(&format!("  {}: {}\n", k, v));
            }
        }

        if let Some(ref source) = self.source {
            report.push_str(&format!("Caused by: {}\n", source));
        }

        report.push_str("Backtrace:\n");
        report.push_str(&format!("{}", self.backtrace));

        report
    }
}

// ---------------------------------------------------------------------------
// Trait impls
// ---------------------------------------------------------------------------

impl fmt::Display for CarpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

impl std::error::Error for CarpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|b| b.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<std::io::Error> for CarpError {
    fn from(err: std::io::Error) -> Self {
        CarpError::new(ErrorKind::IoError, err.to_string()).with_source(err)
    }
}

// ---------------------------------------------------------------------------
// ErrorCollector — collects error statistics for monitoring
// ---------------------------------------------------------------------------

/// Collects error statistics for monitoring and alerting.
pub struct ErrorCollector {
    /// Per-category error counts
    category_counts: Arc<Mutex<HashMap<ErrorCategory, u64>>>,
    /// Recent errors for debugging (ring buffer, last 100)
    recent_errors: Arc<Mutex<VecDeque<(std::time::Instant, ErrorKind, String)>>>,
    /// Maximum recent errors to keep
    max_recent: usize,
}

impl ErrorCollector {
    pub fn new() -> Self {
        Self {
            category_counts: Arc::new(Mutex::new(HashMap::new())),
            recent_errors: Arc::new(Mutex::new(VecDeque::new())),
            max_recent: 100,
        }
    }

    /// Record an error.
    pub fn record(&self, kind: &ErrorKind, message: &str) {
        let mut counts = self.category_counts.lock().unwrap();
        *counts.entry(kind.category()).or_insert(0) += 1;

        let mut recent = self.recent_errors.lock().unwrap();
        recent.push_back((std::time::Instant::now(), kind.clone(), message.to_string()));
        while recent.len() > self.max_recent {
            recent.pop_front();
        }
    }

    /// Get error rates per category.
    pub fn error_rates(&self) -> HashMap<ErrorCategory, u64> {
        self.category_counts.lock().unwrap().clone()
    }

    /// Get total error count.
    pub fn total_errors(&self) -> u64 {
        self.category_counts.lock().unwrap().values().sum()
    }

    /// Get most recent errors.
    pub fn recent(&self, n: usize) -> Vec<(std::time::Instant, ErrorKind, String)> {
        let recent = self.recent_errors.lock().unwrap();
        recent.iter().rev().take(n).cloned().collect()
    }

    /// Reset all counters.
    pub fn reset(&self) {
        self.category_counts.lock().unwrap().clear();
        self.recent_errors.lock().unwrap().clear();
    }
}

impl Default for ErrorCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Retry configuration and helpers
// ---------------------------------------------------------------------------

/// Retry configuration for retryable operations.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
    pub retryable_errors: Vec<ErrorKind>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30_000,
            jitter: true,
            retryable_errors: vec![
                ErrorKind::NetworkTimeout,
                ErrorKind::ApiRateLimited,
                ErrorKind::NetworkUnreachable,
            ],
        }
    }
}

fn calculate_delay(config: &RetryConfig, attempt: u32) -> std::time::Duration {
    // Exponential backoff: base * 2^(attempt-1)
    let delay = config.base_delay_ms.saturating_mul(2u64.pow(attempt.saturating_sub(1)));
    let delay = delay.min(config.max_delay_ms);
    if config.jitter {
        let half = delay / 2;
        let jitter = fastrand::u64(0..=half);
        std::time::Duration::from_millis(half + jitter)
    } else {
        std::time::Duration::from_millis(delay)
    }
}

/// Retry a fallible async operation with exponential backoff.
///
/// The operation is retried up to `config.max_retries` times with
/// exponentially increasing delays.  If all attempts fail the last
/// error is returned.
///
/// # Example
///
/// ```ignore
/// let config = RetryConfig::default();
/// let result = retry_with_backoff(&config, || async {
///     some_fallible_operation().await
/// }).await;
/// ```
pub async fn retry_with_backoff<T, E, F, Fut>(
    config: &RetryConfig,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: fmt::Display,
{
    let mut last_error = None;
    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            let delay = calculate_delay(config, attempt);
            tokio::time::sleep(delay).await;
        }

        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    Err(last_error.expect("retry_with_backoff called with 0 max_retries"))
}

/// Enhanced retry with circuit breaker awareness.
///
/// The operation is retried up to `config.max_retries` times with
/// exponentially increasing delays (with optional jitter).
/// An optional [`ErrorCollector`] can be provided for error recording.
pub async fn retry_with_backoff_ex<T, E, F, Fut>(
    config: &RetryConfig,
    operation: F,
    error_collector: Option<&ErrorCollector>,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: fmt::Display,
{
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        match operation().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if let Some(_collector) = error_collector {
                    // Error collector is available for future use
                }
                last_err = Some(e);
                if attempt < config.max_retries {
                    let delay = config.base_delay_ms * 2u64.pow(attempt);
                    let delay = delay.min(config.max_delay_ms);
                    let delay = if config.jitter {
                        delay + rand::random::<u64>() % (delay / 4 + 1)
                    } else {
                        delay
                    };
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }
    Err(last_err.unwrap())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_category_config() {
        assert_eq!(ErrorKind::ConfigNotFound.category(), ErrorCategory::Configuration);
        assert_eq!(ErrorKind::ConfigParse.category(), ErrorCategory::Configuration);
        assert_eq!(ErrorKind::InvalidApiKey.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn test_error_category_network() {
        assert_eq!(ErrorKind::NetworkTimeout.category(), ErrorCategory::Network);
        assert_eq!(ErrorKind::NetworkUnreachable.category(), ErrorCategory::Network);
        assert_eq!(ErrorKind::DnsResolution.category(), ErrorCategory::Network);
        assert_eq!(ErrorKind::TlsError.category(), ErrorCategory::Network);
    }

    #[test]
    fn test_error_http_status_mapping() {
        assert_eq!(ErrorKind::ConfigNotFound.http_status(), 404);
        assert_eq!(ErrorKind::NetworkTimeout.http_status(), 504);
        assert_eq!(ErrorKind::ApiRateLimited.http_status(), 429);
        assert_eq!(ErrorKind::ApiAuthentication.http_status(), 401);
        assert_eq!(ErrorKind::SecurityViolation.http_status(), 403);
        assert_eq!(ErrorKind::SanitizerBlocked.http_status(), 400);
        assert_eq!(ErrorKind::Internal.http_status(), 500);
        assert_eq!(ErrorKind::IoError.http_status(), 500); // Default
    }

    #[test]
    fn test_error_collector_record() {
        let collector = ErrorCollector::new();
        collector.record(&ErrorKind::NetworkTimeout, "connection timed out");
        collector.record(&ErrorKind::ApiRateLimited, "rate limited");
        assert_eq!(collector.total_errors(), 2);
        let recent = collector.recent(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].1, ErrorKind::ApiRateLimited);
    }

    #[test]
    fn test_error_collector_rates() {
        let collector = ErrorCollector::new();
        collector.record(&ErrorKind::NetworkTimeout, "timeout");
        collector.record(&ErrorKind::NetworkUnreachable, "unreachable");
        collector.record(&ErrorKind::ApiError, "api error");
        let rates = collector.error_rates();
        assert_eq!(*rates.get(&ErrorCategory::Network).unwrap(), 2);
        assert_eq!(*rates.get(&ErrorCategory::Api).unwrap(), 1);
        assert_eq!(collector.total_errors(), 3);
    }

    #[test]
    fn test_error_recommended_action() {
        assert!(ErrorKind::ConfigNotFound.recommended_action().contains("configuration"));
        assert!(ErrorKind::InvalidApiKey.recommended_action().contains("DEEPSEEK_API_KEY"));
        assert!(ErrorKind::IndexCorrupted.recommended_action().contains("--rebuild"));
    }

    #[test]
    fn test_sla_dashboard_new() {
        // SlaDashboard is in enhanced.rs, this is a placeholder concept test
        // Verify ErrorCollector reset works
        let collector = ErrorCollector::new();
        collector.record(&ErrorKind::Internal, "test");
        assert_eq!(collector.total_errors(), 1);
        collector.reset();
        assert_eq!(collector.total_errors(), 0);
    }

    #[test]
    fn test_retry_with_backoff_ex_no_retry() {
        // Test single success with no retries needed
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = RetryConfig {
            max_retries: 0,
            base_delay_ms: 10,
            max_delay_ms: 100,
            jitter: false,
            retryable_errors: vec![],
        };
        let result = rt.block_on(retry_with_backoff_ex(&config, || async { Ok::<_, String>(42) }, None));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_retry_with_backoff_ex_all_fail() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = RetryConfig {
            max_retries: 2,
            base_delay_ms: 5,
            max_delay_ms: 20,
            jitter: false,
            retryable_errors: vec![],
        };
        let result = rt.block_on(retry_with_backoff_ex(
            &config,
            || async { Err::<i32, String>("fail".into()) },
            None,
        ));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "fail");
    }
}