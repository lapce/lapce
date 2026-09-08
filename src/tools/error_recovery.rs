//! Error recovery + retry strategy + circuit breaker.
//!
//! Ported from CarpAI's `src/error_recovery.rs` and `src/circuit_breaker.rs`.
//! Provides a complete resilience stack for API calls and tool execution.
//!
//! ## Architecture
//!
//! ```text
//! Error → ErrorClassifier → RetryStrategy → RetryExecutor
//!                                        ↘ CircuitBreaker (stopped after N failures)
//! ```
//!
//! ## Error Severities
//!
//! - **Transient**: network hiccup, DNS resolve → immediate retry
//! - **Retryable**: HTTP 429/503 → exponential backoff
//! - **Degradable**: provider 5xx → switch to fallback provider
//! - **Fatal**: auth error 401/403 → no retry, propagate

use std::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};

// ============================================================================
// Error Classification
// ============================================================================

/// Severity of an error — determines retry strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorSeverity {
    /// Network hiccup, DNS resolve — immediate retry with minimal delay.
    Transient,
    /// Rate limit (429), server overload (503) — exponential backoff.
    Retryable,
    /// Provider 5xx error — retry with fallback provider.
    Degradable,
    /// Auth error (401/403), config error — do not retry.
    Fatal,
}

/// Retry strategy for a given error severity.
#[derive(Debug, Clone)]
pub enum RetryStrategy {
    /// Don't retry.
    NoRetry,
    /// Retry immediately with minimal delay.
    Immediate,
    /// Exponential backoff: delay = min(initial * 2^n, max_delay).
    ExponentialBackoff {
        initial_ms: u64,
        max_ms: u64,
        max_attempts: u32,
    },
    /// Fixed interval retry.
    FixedInterval {
        interval_ms: u64,
        max_attempts: u32,
    },
}

impl RetryStrategy {
    /// Determine strategy from error severity.
    pub fn for_severity(severity: ErrorSeverity) -> Self {
        match severity {
            ErrorSeverity::Transient => Self::Immediate,
            ErrorSeverity::Retryable => Self::ExponentialBackoff {
                initial_ms: 1000,
                max_ms: 30000,
                max_attempts: 3,
            },
            ErrorSeverity::Degradable => Self::ExponentialBackoff {
                initial_ms: 2000,
                max_ms: 60000,
                max_attempts: 2,
            },
            ErrorSeverity::Fatal => Self::NoRetry,
        }
    }

    /// Get delay for a given attempt number.
    pub fn delay_ms(&self, attempt: u32) -> u64 {
        match self {
            Self::NoRetry => 0,
            Self::Immediate => 100,
            Self::ExponentialBackoff { initial_ms, max_ms, .. } => {
                (*initial_ms * 2u64.pow(attempt)).min(*max_ms)
            }
            Self::FixedInterval { interval_ms, .. } => *interval_ms,
        }
    }

    /// Maximum retry attempts.
    pub fn max_attempts(&self) -> u32 {
        match self {
            Self::NoRetry => 0,
            Self::Immediate => 1,
            Self::ExponentialBackoff { max_attempts, .. } => *max_attempts,
            Self::FixedInterval { max_attempts, .. } => *max_attempts,
        }
    }
}

/// Classify errors into severities for retry decisions.
pub struct ErrorClassifier;

impl ErrorClassifier {
    /// Classify an HTTP error or generic error.
    pub fn classify_http_status(status: u16) -> ErrorSeverity {
        match status {
            429 => ErrorSeverity::Retryable,      // Rate limited
            503 => ErrorSeverity::Retryable,      // Service unavailable
            500..=502 | 504..=599 => ErrorSeverity::Degradable, // Server errors
            401 | 403 => ErrorSeverity::Fatal,     // Auth errors
            400 | 404 => ErrorSeverity::Fatal,     // Client errors
            _ => match status {
                s if s < 500 => ErrorSeverity::Fatal,
                _ => ErrorSeverity::Degradable,
            },
        }
    }

    /// Classify a network error.
    pub fn classify_network_error(error_msg: &str) -> ErrorSeverity {
        let lower = error_msg.to_lowercase();
        if lower.contains("dns") || lower.contains("resolve")
            || lower.contains("connection refused")
            || lower.contains("no route to host")
            || lower.contains("connection reset")
            || lower.contains("broken pipe")
        {
            ErrorSeverity::Transient
        } else if lower.contains("timeout") {
            ErrorSeverity::Retryable
        } else {
            ErrorSeverity::Fatal
        }
    }
}

// ============================================================================
// Circuit Breaker
// ============================================================================

/// Circuit breaker states (inspired by Polly/Hystrix).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,     // Normal operation, requests pass through
    Open,       // Requests are immediately rejected
    HalfOpen,   // Limited requests allowed to test recovery
}

/// Tracks outcomes within a sliding window.
#[derive(Debug)]
struct FailureWindow {
    results: Vec<bool>,     // true = success, false = failure
    position: usize,
    window_size: usize,
}

impl FailureWindow {
    fn new(window_size: usize) -> Self {
        Self {
            results: vec![true; window_size],
            position: 0,
            window_size,
        }
    }

    fn record(&mut self, success: bool) {
        self.results[self.position % self.window_size] = success;
        self.position += 1;
    }

    fn failure_rate(&self) -> f64 {
        let count = self.results.len().min(self.position);
        if count == 0 { return 0.0; }
        let failures = self.results[..count].iter().filter(|&&s| !s).count();
        failures as f64 / count as f64
    }
}

/// A circuit breaker that stops API calls when failure rate exceeds threshold.
#[derive(Debug)]
pub struct CircuitBreaker {
    state: parking_lot::Mutex<CircuitState>,
    window: parking_lot::Mutex<FailureWindow>,
    /// Failure count before opening circuit.
    failure_threshold: usize,
    /// Failure rate threshold (0.0 - 1.0) before opening.
    failure_rate_threshold: f64,
    /// Time to wait before transitioning to half-open.
    recovery_timeout: Duration,
    /// When the circuit was opened (for recovery).
    opened_at: parking_lot::Mutex<Option<std::time::Instant>>,
    /// Half-open limit: number of requests allowed before deciding.
    half_open_limit: usize,
    /// Successful half-open trials.
    half_open_successes: AtomicUsize,
    /// Circuit name for logging.
    name: String,
}

impl CircuitBreaker {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            state: parking_lot::Mutex::new(CircuitState::Closed),
            window: parking_lot::Mutex::new(FailureWindow::new(20)),
            failure_threshold: 5,
            failure_rate_threshold: 0.5,
            recovery_timeout: Duration::from_secs(30),
            opened_at: parking_lot::Mutex::new(None),
            half_open_limit: 3,
            half_open_successes: AtomicUsize::new(0),
            name: name.into(),
        }
    }

    /// Check if a request should be allowed through.
    pub fn allow_request(&self) -> bool {
        let state = *self.state.lock();
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let now = std::time::Instant::now();
                let opened = self.opened_at.lock();
                if let Some(opened_time) = *opened {
                    if now.duration_since(opened_time) >= self.recovery_timeout {
                        drop(opened);
                        *self.state.lock() = CircuitState::HalfOpen;
                        self.half_open_successes.store(0, Ordering::Relaxed);
                        tracing::info!(circuit=%self.name, "Transitioning to HalfOpen");
                        return true;
                    }
                }
                tracing::debug!(circuit=%self.name, "Request blocked — circuit open");
                false
            }
            CircuitState::HalfOpen => {
                let allowed = self.half_open_successes.load(Ordering::Relaxed)
                    < self.half_open_limit;
                if !allowed {
                    tracing::debug!(circuit=%self.name, "HalfOpen limit reached");
                }
                allowed
            }
        }
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        self.window.lock().record(true);
        let state = *self.state.lock();
        if state == CircuitState::HalfOpen {
            let successes = self.half_open_successes.fetch_add(1, Ordering::Relaxed) + 1;
            if successes >= self.half_open_limit {
                *self.state.lock() = CircuitState::Closed;
                self.window.lock().record(true);
                tracing::info!(circuit=%self.name, "Circuit closed — recovered");
            }
        }
    }

    /// Record a failed request.
    pub fn record_failure(&self) {
        self.window.lock().record(false);
        let window = self.window.lock();
        let rate = window.failure_rate();
        let count = window.position;
        drop(window);

        let state = *self.state.lock();
        match state {
            CircuitState::Closed => {
                if count >= self.failure_threshold || rate >= self.failure_rate_threshold {
                    *self.state.lock() = CircuitState::Open;
                    *self.opened_at.lock() = Some(std::time::Instant::now());
                    tracing::warn!(
                        circuit=%self.name,
                        failure_rate=rate,
                        failures=count,
                        "Circuit opened"
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state re-opens the circuit
                *self.state.lock() = CircuitState::Open;
                *self.opened_at.lock() = Some(std::time::Instant::now());
                tracing::warn!(circuit=%self.name, "Half-open test failed — circuit re-opened");
            }
            _ => {}
        }
    }

    /// Get current state for observability.
    pub fn state(&self) -> CircuitState {
        *self.state.lock()
    }

    /// Reset circuit to closed state.
    pub fn reset(&self) {
        *self.state.lock() = CircuitState::Closed;
        *self.window.lock() = FailureWindow::new(20);
        *self.opened_at.lock() = None;
        self.half_open_successes.store(0, Ordering::Relaxed);
    }
}

// ============================================================================
// Retry Executor
// ============================================================================

/// Execute an async operation with retry logic.
pub async fn retry_async<F, Fut, T, E>(
    operation: F,
    strategy: RetryStrategy,
    on_error: Option<&dyn Fn(usize, &E)>,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let max = strategy.max_attempts();
    let max_count: u32 = max;
    for attempt in 0..=max_count {
        match operation().await {
            Ok(val) => return Ok(val),
            Err(ref e) if attempt < max_count => {
                let delay = strategy.delay_ms(attempt);
                if let Some(cb) = on_error {
                    cb((attempt + 1) as usize, e);
                }
                tracing::warn!(attempt=attempt+1, delay_ms=delay, error=%e, "Retrying");
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_classifier() {
        assert_eq!(ErrorClassifier::classify_http_status(429), ErrorSeverity::Retryable);
        assert_eq!(ErrorClassifier::classify_http_status(503), ErrorSeverity::Retryable);
        assert_eq!(ErrorClassifier::classify_http_status(401), ErrorSeverity::Fatal);
        assert_eq!(ErrorClassifier::classify_http_status(500), ErrorSeverity::Degradable);
    }

    #[test]
    fn test_retry_strategy_delays() {
        let s = RetryStrategy::ExponentialBackoff { initial_ms: 1000, max_ms: 10000, max_attempts: 3 };
        assert_eq!(s.delay_ms(0), 1000);
        assert_eq!(s.delay_ms(1), 2000);
        assert_eq!(s.delay_ms(2), 4000);
        assert_eq!(s.delay_ms(10), 10000); // capped
    }

    #[test]
    fn test_circuit_breaker_lifecycle() {
        let cb = CircuitBreaker::new("test");
        assert_eq!(cb.state(), CircuitState::Closed);

        // Simulate failures
        for _ in 0..6 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());

        // Reset
        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }
}
