//! Error Recovery Module - Circuit breakers, retries, and fallback mechanisms.
//!
//! This module provides production-grade error handling with:
//! - Circuit breaker pattern for failing providers
//! - Exponential backoff retry logic
//! - Automatic fallback to healthy providers
//! - Graceful degradation

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally.
    Closed,
    /// Circuit is open, requests fail fast.
    Open,
    /// Circuit is half-open, testing if provider recovered.
    HalfOpen,
}

/// Circuit breaker for a single provider.
pub struct CircuitBreaker {
    /// Current state.
    state: CircuitState,
    /// Number of consecutive failures.
    failures: u32,
    /// Number of consecutive successes (in half-open state).
    successes: u32,
    /// Threshold to open circuit.
    failure_threshold: u32,
    /// Threshold to close circuit from half-open.
    success_threshold: u32,
    /// Time the circuit was opened.
    opened_at: Option<Instant>,
    /// Cooldown duration before trying again.
    cooldown: Duration,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, success_threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            failures: 0,
            successes: 0,
            failure_threshold,
            success_threshold,
            opened_at: None,
            cooldown: Duration::from_secs(cooldown_secs),
        }
    }

    /// Check if requests are allowed.
    pub fn is_allowed(&self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if cooldown has passed
                if let Some(opened_at) = self.opened_at {
                    if opened_at.elapsed() >= self.cooldown {
                        return true; // Will transition to half-open
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful request.
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failures = 0;
            }
            CircuitState::HalfOpen => {
                self.successes += 1;
                if self.successes >= self.success_threshold {
                    // Recovery successful, close circuit
                    self.state = CircuitState::Closed;
                    self.failures = 0;
                    self.successes = 0;
                }
            }
            CircuitState::Open => {
                // Should not happen, but handle gracefully
            }
        }
    }

    /// Record a failed request.
    pub fn record_failure(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failures += 1;
                if self.failures >= self.failure_threshold {
                    self.state = CircuitState::Open;
                    self.opened_at = Some(Instant::now());
                }
            }
            CircuitState::HalfOpen => {
                // Failure during testing, immediately open
                self.state = CircuitState::Open;
                self.opened_at = Some(Instant::now());
                self.successes = 0;
            }
            CircuitState::Open => {
                // Already open, reset timer
                self.opened_at = Some(Instant::now());
            }
        }
    }

    /// Get current state.
    pub fn state(&self) -> CircuitState {
        if let Some(opened_at) = self.opened_at {
            if self.state == CircuitState::Open && opened_at.elapsed() >= self.cooldown {
                return CircuitState::HalfOpen;
            }
        }
        self.state
    }
}

/// Retry configuration.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries.
    pub max_retries: u32,
    /// Base delay for exponential backoff.
    pub base_delay_ms: u64,
    /// Maximum delay cap.
    pub max_delay_ms: u64,
    /// Enable jitter.
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 5000,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Calculate delay for given attempt with exponential backoff.
    pub fn delay(&self, attempt: u32) -> Duration {
        let exp_delay = self.base_delay_ms * 2u64.pow(attempt);
        let delay = exp_delay.min(self.max_delay_ms);

        let final_delay = if self.jitter {
            // Add jitter: random value between 0.5x and 1.5x
            let jitter_range = delay / 2;
            let jitter = rand_simple() % jitter_range;
            delay - jitter_range / 2 + jitter
        } else {
            delay
        };

        Duration::from_millis(final_delay)
    }
}

/// Simple pseudo-random number generator (for jitter).
fn rand_simple() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    
    
    RandomState::new().hash_one(Instant::now())
}

/// Retry wrapper for operations.
pub async fn with_retry<F, Fut, T, E, Fn>(
    config: RetryConfig,
    mut operation: Fn,
) -> Result<T, E>
where
    Fn: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut attempt = 0;

    loop {
        let result = operation().await;

        match result {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt >= config.max_retries {
                    return Err(e);
                }
                let delay = config.delay(attempt);
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

/// Fallback chain - try providers in order until one succeeds.
pub struct FallbackChain<T> {
    items: Vec<T>,
    circuit_breakers: Arc<RwLock<HashMap<String, CircuitBreaker>>>,
}

impl<T> FallbackChain<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Execute with fallback, skipping unhealthy providers.
    pub async fn execute<F, Fut, R>(&self, name: &str, mut f: F) -> Result<R, FallbackError>
    where
        F: FnMut(&T) -> Fut,
        Fut: std::future::Future<Output = Result<R, ProviderFallbackError>>,
    {
        let circuits = self.circuit_breakers.read().await;
        for (i, item) in self.items.iter().enumerate() {
            // Check circuit breaker
            if let Some(circuit) = circuits.get(name) {
                if !circuit.is_allowed() {
                    continue;
                }
            }

            match f(item).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Record failure in circuit breaker
                    let mut circuits = self.circuit_breakers.write().await;
                    let circuit = circuits.entry(name.to_string()).or_insert_with(|| {
                        CircuitBreaker::new(5, 2, 30)
                    });
                    circuit.record_failure();

                    // If this was the last provider, return error
                    if i == self.items.len() - 1 {
                        return Err(FallbackError::AllProvidersFailed(vec![e]));
                    }
                }
            }
        }

        Err(FallbackError::NoProviders)
    }
}

/// Error when all fallback providers fail.
#[derive(Debug)]
pub enum FallbackError {
    AllProvidersFailed(Vec<ProviderFallbackError>),
    NoProviders,
}

/// Error from a single provider during fallback.
#[derive(Debug)]
pub struct ProviderFallbackError {
    pub provider: String,
    pub error: String,
}

/// Recovery manager - coordinates all recovery mechanisms.
pub struct RecoveryManager {
    circuit_breakers: Arc<RwLock<HashMap<String, Arc<RwLock<CircuitBreaker>>>>>,
    retry_config: RetryConfig,
}

impl RecoveryManager {
    pub fn new() -> Self {
        Self {
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            retry_config: RetryConfig::default(),
        }
    }

    /// Get or create circuit breaker for a provider.
    pub async fn get_circuit(&self, name: &str) -> Arc<RwLock<CircuitBreaker>> {
        let mut circuits = self.circuit_breakers.write().await;
        if !circuits.contains_key(name) {
            circuits.insert(name.to_string(), Arc::new(RwLock::new(CircuitBreaker::new(5, 2, 30))));
        }
        Arc::clone(circuits.get(name).expect("unwrap failed: recovery.rs:293"))
    }

    /// Check if provider is healthy (circuit allows requests).
    pub async fn is_healthy(&self, name: &str) -> bool {
        let circuits = self.circuit_breakers.read().await;
        if let Some(cb) = circuits.get(name) {
            cb.read().await.is_allowed()
        } else {
            true
        }
    }

    /// Record success for a provider.
    pub async fn record_success(&self, name: &str) {
        let circuits = self.circuit_breakers.read().await;
        if let Some(cb) = circuits.get(name) {
            cb.write().await.record_success();
        }
    }

    /// Record failure for a provider.
    pub async fn record_failure(&self, name: &str) {
        let circuits = self.circuit_breakers.read().await;
        if let Some(cb) = circuits.get(name) {
            cb.write().await.record_failure();
        }
    }
}

impl Default for RecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryManager {
    /// Get the retry configuration for this recovery manager.
    pub fn retry_config(&self) -> &RetryConfig {
        &self.retry_config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_opens_after_failures() {
        let mut cb = CircuitBreaker::new(3, 2, 60);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_allowed());

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.is_allowed());
    }

    #[test]
    fn test_circuit_breaker_recovery() {
        let mut cb = CircuitBreaker::new(3, 2, 60);

        // Open the circuit
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for cooldown (in test we just check half-open state)
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert!(cb.is_allowed());

        // Record successes to close
        cb.record_success();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_retry_config_exponential_backoff() {
        let config = RetryConfig {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 1000,
            jitter: false,
        };

        assert_eq!(config.delay(0), Duration::from_millis(100));
        assert_eq!(config.delay(1), Duration::from_millis(200));
        assert_eq!(config.delay(2), Duration::from_millis(400));
        assert_eq!(config.delay(3), Duration::from_millis(800));
        assert_eq!(config.delay(10), Duration::from_millis(1000)); // capped
    }
}
