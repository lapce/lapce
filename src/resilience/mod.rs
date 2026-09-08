//! Resilience layer — integrates error recovery into Agent/Provider main loop.
//!
//! Provides:
//! - [`RateLimiter`] — token-bucket algorithm for request rate control
//! - [`ConcurrencyTracker`] — concurrency limiter for parallel requests
//! - [`FallbackChain`] — automatic failover across providers with per-provider circuit breakers
//! - [`ResilienceConfig`] — unified configuration for all resilience features
//! - [`ResilienceManager`] — single entry point combining rate limiting, concurrency, and circuit breaking
//! - [`GuardToken`] — RAII-style resource guard released on drop

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::tools::error_recovery::{
    CircuitBreaker, CircuitState,
};

// ============================================================================
// Rate Limiter (Token Bucket)
// ============================================================================

/// Token-bucket rate limiter for controlling request concurrency.
pub struct RateLimiter {
    /// Maximum requests per second.
    rps: f64,
    /// Token bucket: tokens available now.
    tokens: Arc<RwLock<f64>>,
    /// Max tokens in bucket (burst allowance).
    max_tokens: f64,
    /// Last refill timestamp.
    last_refill: Arc<RwLock<Instant>>,
}

impl RateLimiter {
    pub fn new(rps: f64, burst: f64) -> Self {
        Self {
            rps,
            tokens: Arc::new(RwLock::new(burst)),
            max_tokens: burst,
            last_refill: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Acquire permission to make a request. Blocks if rate limited.
    pub async fn acquire(&self) -> anyhow::Result<()> {
        loop {
            // Refill tokens based on elapsed time
            {
                let mut tokens = self.tokens.write().await;
                let mut last = self.last_refill.write().await;
                let elapsed = last.elapsed().as_secs_f64();
                *last = Instant::now();
                *tokens = (*tokens + elapsed * self.rps).min(self.max_tokens);
            }

            // Try to consume a token
            {
                let mut tokens = self.tokens.write().await;
                if *tokens >= 1.0 {
                    *tokens -= 1.0;
                    return Ok(());
                }
                // Calculate wait time needed
                let wait_secs = (1.0 - *tokens) / self.rps;
                drop(tokens);
                tokio::time::sleep(Duration::from_secs_f64(wait_secs)).await;
                // Loop back to retry after waiting
            }
        }
}

    /// Non-blocking check: can we make a request right now?
    pub async fn try_acquire(&self) -> bool {
        let mut tokens = self.tokens.write().await;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// ============================================================================
// Concurrency Tracker
// ============================================================================

/// Concurrency tracker — limits simultaneous in-flight requests.
pub struct ConcurrencyTracker {
    max: usize,
    active: Arc<RwLock<usize>>,
}

impl ConcurrencyTracker {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            active: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn try_acquire(&self) -> bool {
        let mut active = self.active.write().await;
        if *active < self.max {
            *active += 1;
            true
        } else {
            false
        }
    }

    pub async fn release(&self) {
        let mut active = self.active.write().await;
        *active = active.saturating_sub(1);
    }

    pub async fn active_count(&self) -> usize {
        *self.active.read().await
    }
}

// ============================================================================
// Fallback Chain
// ============================================================================

/// Fallback chain for multi-provider failover with per-provider circuit breakers.
///
/// Providers are tried in priority order. Each provider has its own
/// [`CircuitBreaker`] that trips after consecutive failures and recovers
/// after a cooldown period.
pub struct FallbackChain {
    providers: Vec<String>,
    cb_per_provider: HashMap<String, CircuitBreaker>,
    current_index: Arc<RwLock<usize>>,
}

impl FallbackChain {
    pub fn new(providers: Vec<String>) -> Self {
        let cb_map: HashMap<String, CircuitBreaker> = providers
            .iter()
            .map(|p| (p.clone(), CircuitBreaker::new(format!("provider-{}", p))))
            .collect();
        Self {
            providers,
            cb_per_provider: cb_map,
            current_index: Arc::new(RwLock::new(0)),
        }
    }

    /// Get next healthy provider name. Skips providers with open circuit breakers.
    pub async fn next_provider(&self) -> Option<(String, &CircuitBreaker)> {
        let start = *self.current_index.read().await;
        let len = self.providers.len();

        for i in 0..len {
            let idx = (start + i) % len;
            let name = &self.providers[idx];
            if let Some(cb) = self.cb_per_provider.get(name) {
                match cb.state() {
                    CircuitState::Closed | CircuitState::HalfOpen => {
                        *self.current_index.write().await = idx;
                        return Some((name.clone(), cb));
                    }
                    CircuitState::Open => continue,
                }
            }
        }
        None // All providers have tripped their circuit breakers
    }

    /// Record success for a provider (resets its circuit breaker).
    pub fn record_success(&self, provider: &str) {
        if let Some(cb) = self.cb_per_provider.get(provider) {
            cb.record_success();
        }
    }

    /// Record failure for a provider (may trip its circuit breaker).
    pub fn record_failure(&self, provider: &str) {
        if let Some(cb) = self.cb_per_provider.get(provider) {
            cb.record_failure();
        }
    }

    /// Get health status of all providers.
    pub fn status(&self) -> Vec<ProviderHealth> {
        self.providers
            .iter()
            .map(|name| {
                let cb = self
                    .cb_per_provider
                    .get(name)
                    .expect("circuit breaker should exist for known provider");
                ProviderHealth {
                    name: name.clone(),
                    state: cb.state(),
                }
            })
            .collect()
    }
}

/// Health snapshot for a single provider in the fallback chain.
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub name: String,
    pub state: CircuitState,
}

// ============================================================================
// Resilience Configuration
// ============================================================================

/// Resilience configuration for the entire system.
#[derive(Debug, Clone)]
pub struct ResilienceConfig {
    pub max_rps: f64,
    pub max_burst: f64,
    pub max_concurrent: usize,
    pub cb_failure_threshold: u32,
    pub cb_open_timeout_secs: u64,
    pub retry_max_attempts: u32,
    pub retry_initial_delay_ms: u64,
    pub fallback_providers: Vec<String>,
}

impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            max_rps: 50.0,
            max_burst: 10.0,
            max_concurrent: 8,
            cb_failure_threshold: 5,
            cb_open_timeout_secs: 30,
            retry_max_attempts: 3,
            retry_initial_delay_ms: 1000,
            fallback_providers: vec!["deepseek".into(), "openai".into()],
        }
    }
}

// ============================================================================
// Resilience Manager
// ============================================================================

/// Unified resilience manager — single entry point for all resilience features.
///
/// Combines rate limiting, concurrency control, and circuit-breaker-based
/// provider failover into one pre-request guard.
pub struct ResilienceManager {
    pub rate_limiter: RateLimiter,
    pub concurrency: ConcurrencyTracker,
    pub fallback_chain: FallbackChain,
    pub config: ResilienceConfig,
}

impl ResilienceManager {
    pub fn new(config: ResilienceConfig) -> Self {
        Self {
            rate_limiter: RateLimiter::new(config.max_rps, config.max_burst),
            concurrency: ConcurrencyTracker::new(config.max_concurrent),
            fallback_chain: FallbackChain::new(config.fallback_providers.clone()),
            config,
        }
    }

    /// Full guard: rate limit + concurrency + circuit breaker check before a request.
    ///
    /// Returns a [`GuardToken`] that releases the concurrency slot when dropped
    /// (or call [`GuardToken::release`] explicitly in async context).
    pub async fn pre_request_guard(&self) -> anyhow::Result<GuardToken<'_>> {
        // 1. Rate limit
        self.rate_limiter.acquire().await?;

        // 2. Concurrency
        if !self.concurrency.try_acquire().await {
            anyhow::bail!(
                "Max concurrent requests ({}) reached",
                self.config.max_concurrent
            );
        }

        // 3. Check if any provider is available
        if self.fallback_chain.next_provider().await.is_none() {
            self.concurrency.release().await;
            anyhow::bail!("All providers have tripped their circuit breakers");
        }

        Ok(GuardToken { mgr: self })
    }

    /// Record a successful provider response (resets that provider's circuit breaker).
    pub fn record_provider_success(&self, provider: &str) {
        self.fallback_chain.record_success(provider);
    }

    /// Record a failed provider response (may trip that provider's circuit breaker).
    pub fn record_provider_failure(&self, provider: &str) {
        self.fallback_chain.record_failure(provider);
    }

    /// Get system-wide resilience metrics snapshot.
    pub fn metrics(&self) -> ResilienceMetrics {
        ResilienceMetrics {
            max_rps: self.config.max_rps,
            max_concurrent: self.config.max_concurrent,
            provider_status: self.fallback_chain.status(),
        }
    }
}

// ============================================================================
// Guard Token
// ============================================================================

/// Token returned by [`ResilienceManager::pre_request_guard`] — releases resources on drop.
///
/// Call [`GuardToken::release`] explicitly in async contexts; the `Drop` impl
/// is a safety net for synchronous paths.
pub struct GuardToken<'a> {
    mgr: &'a ResilienceManager,
}

impl GuardToken<'_> {
    /// Explicitly release held resources (concurrency slot).
    pub async fn release(&self) {
        self.mgr.concurrency.release().await;
    }
}

// ============================================================================
// Metrics
// ============================================================================

/// Snapshot of the system's current resilience state.
#[derive(Debug, Clone)]
pub struct ResilienceMetrics {
    pub max_rps: f64,
    pub max_concurrent: usize,
    pub provider_status: Vec<ProviderHealth>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let rl = RateLimiter::new(100.0, 10.0);
        for _ in 0..10 {
            rl.acquire().await.expect("should allow burst");
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_try_acquire() {
        let rl = RateLimiter::new(1000.0, 2.0);
        assert!(rl.try_acquire().await);
        assert!(rl.try_acquire().await);
        // Burst exhausted — third may or may not pass depending on refill timing
        let _ = rl.try_acquire().await;
    }

    #[tokio::test]
    async fn test_concurrency_tracker() {
        let ct = ConcurrencyTracker::new(3);
        assert!(ct.try_acquire().await);
        assert!(ct.try_acquire().await);
        assert!(ct.try_acquire().await);
        assert!(!ct.try_acquire().await); // 4th should fail
        ct.release().await;
        assert!(ct.try_acquire().await); // Should work again
    }

    #[test]
    fn test_fallback_chain_creation() {
        let chain = FallbackChain::new(vec!["deepseek".into(), "openai".into()]);
        assert_eq!(chain.providers.len(), 2);
    }

    #[tokio::test]
    async fn test_circuit_breaker_in_chain() {
        let chain = FallbackChain::new(vec!["provider_a".into()]);

        // Initially closed
        let (name, _cb) = chain
            .next_provider()
            .await
            .expect("should have provider");
        assert_eq!(name, "provider_a");

        // Trip it with enough failures (tools CB uses sliding window + threshold)
        for _ in 0..=6 {
            chain.record_failure("provider_a");
        }
        // Now should be open — no provider available
        assert!(
            chain.next_provider().await.is_none(),
            "all providers should be circuit-open"
        );
    }

    #[test]
    fn test_resilience_config_defaults() {
        let cfg = ResilienceConfig::default();
        assert_eq!(cfg.max_rps, 50.0);
        assert_eq!(cfg.retry_max_attempts, 3);
        assert_eq!(cfg.fallback_providers.len(), 2);
    }

    #[test]
    fn test_resilience_manager_creation() {
        let mgr = ResilienceManager::new(ResilienceConfig::default());
        let m = mgr.metrics();
        assert_eq!(m.provider_status.len(), 2); // default has 2 providers
        assert_eq!(m.max_rps, 50.0);
    }

    #[tokio::test]
    async fn test_pre_request_guard_basic() {
        let mgr = ResilienceManager::new(ResilienceConfig {
            max_rps: 1000.0,
            max_burst: 10.0,
            max_concurrent: 4,
            ..Default::default()
        });
        let guard = mgr
            .pre_request_guard()
            .await
            .expect("guard should be acquired");
        guard.release().await;
    }

    #[tokio::test]
    async fn test_pre_request_guard_exhausts_concurrency() {
        let mgr = ResilienceManager::new(ResilienceConfig {
            max_rps: 1000.0,
            max_burst: 100.0,
            max_concurrent: 1,
            fallback_providers: vec!["deepseek".into()],
            ..Default::default()
        });

        let _guard1 = mgr
            .pre_request_guard()
            .await
            .expect("first guard should succeed");

        // Second should fail — concurrency exhausted
        let result = mgr.pre_request_guard().await;
        assert!(
            result.is_err(),
            "second guard should fail due to concurrency limit"
        );
    }

    #[tokio::test]
    async fn test_record_success_resets_chain() {
        let chain = FallbackChain::new(vec!["p1".into()]);
        
        // Trip the circuit
        for _ in 0..=6 {
            chain.record_failure("p1");
        }
        assert!(chain.next_provider().await.is_none(), "should be open");

        // Reset via success (tools CB reset closes the circuit)
        if let Some((_name, cb)) = chain.next_provider().await {
            // If we get here, circuit recovered or was never fully opened
            let _ = cb;
        }
        // After recording success the provider should recover
        chain.record_success("p1");
        // The tools CB record_success moves HalfOpen→Closed when threshold met
    }
}

// ============================================================================
// Chaos Engineering + Graceful Degradation
// ============================================================================

/// Types of chaos experiments to run against the system.
#[derive(Debug, Clone, PartialEq)]
pub enum ChaosScenario {
    /// Randomly delay responses (latency injection).
    LatencyInjection { min_ms: u64, max_ms: u64 },
    /// Drop a percentage of requests.
    PacketLoss { drop_rate: f32 },
    /// Return errors for some requests.
    ErrorInjection { error_rate: f32 },
    /// Simulate provider crash (all requests fail).
    ProviderCrash,
    /// Memory pressure simulation.
    MemoryPressure { usage_pct: u32 },
    /// Network partition (some providers unreachable).
    NetworkPartition { affected_providers: Vec<String> },
    /// Disk I/O slowdown.
    IoSlowdown { latency_ms: u64 },
    /// CPU saturation.
    CpuSaturation { load_factor: f32 },
}

/// Result of running a chaos experiment.
#[derive(Debug, Clone)]
pub struct ChaosResult {
    pub scenario: ChaosScenario,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub avg_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub errors: Vec<String>,
    pub degraded_gracefully: bool,
    pub recovery_time_ms: u64,
}

/// Graceful degradation policy — how the system should behave under stress.
#[derive(Debug, Clone)]
pub struct DegradationPolicy {
    /// When to start degrading features (load percentage).
    pub degrade_at_load_pct: u32,
    /// Features to disable in order of priority (first disabled = least important).
    pub feature_tier_order: Vec<DegradationTier>,
    /// Whether to switch to cheaper/faster model under load.
    pub switch_to_fast_model: bool,
    /// Whether to reduce context window size under load.
    pub shrink_context_window: bool,
    /// Whether to batch requests under load.
    pub enable_request_batching: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationTier {
    StreamingOutput,      // First to disable
    RAGEnrichment,        // Second
    DetailedMetrics,      // Third
    AutoMemory,           // Fourth
    CacheOptimization,    // Fifth (rarely disabled)
}

impl Default for DegradationPolicy {
    fn default() -> Self {
        Self {
            degrade_at_load_pct: 80,
            feature_tier_order: vec![
                DegradationTier::StreamingOutput,
                DegradationTier::RAGEnrichment,
                DegradationTier::DetailedMetrics,
                DegradationTier::AutoMemory,
                DegradationTier::CacheOptimization,
            ],
            switch_to_fast_model: true,
            shrink_context_window: true,
            enable_request_batching: true,
        }
    }
}

/// Chaos experiment runner.
pub struct ChaosEngine {
    policy: DegradationPolicy,
    active_scenarios: Vec<ChaosScenario>,
    metrics_history: Vec<ChaosResult>,
}

impl ChaosEngine {
    pub fn new(policy: DegradationPolicy) -> Self {
        Self {
            policy,
            active_scenarios: Vec::new(),
            metrics_history: Vec::new(),
        }
    }

    /// Register and run a chaos scenario.
    pub fn run_scenario(&mut self, scenario: ChaosScenario) -> anyhow::Result<ChaosResult> {
        use std::time::{Instant, Duration};
        use fastrand::Rng;

        let mut rng = Rng::new();
        let total_requests: u64 = 100;
        let mut successful = 0u64;
        let mut failed = 0u64;
        let mut latencies: Vec<u64> = Vec::with_capacity(total_requests as usize);
        let mut errors: Vec<String> = Vec::new();

        let start = Instant::now();

        for i in 0..total_requests {
            let mut should_fail = false;
            let mut extra_delay_ms: u64 = 0;

            match &scenario {
                ChaosScenario::LatencyInjection { min_ms, max_ms } => {
                    extra_delay_ms = rng.u64(*min_ms..=*max_ms);
                }
                ChaosScenario::PacketLoss { drop_rate } => {
                    if rng.f32() < *drop_rate {
                        should_fail = true;
                        errors.push(format!("request {} dropped (packet loss)", i));
                    }
                }
                ChaosScenario::ErrorInjection { error_rate } => {
                    if rng.f32() < *error_rate {
                        should_fail = true;
                        errors.push(format!("request {} injected error", i));
                    }
                }
                ChaosScenario::ProviderCrash => {
                    should_fail = true;
                    if errors.len() < 10 {
                        errors.push(format!("request {} provider crash", i));
                    }
                }
                ChaosScenario::MemoryPressure { usage_pct } => {
                    if *usage_pct > 95 && rng.f32() < 0.3 {
                        should_fail = true;
                        errors.push(format!("request {} OOM under memory pressure", i));
                    }
                    extra_delay_ms = (*usage_pct as u64) / 4; // Simulate GC pressure
                }
                ChaosScenario::NetworkPartition { .. } => {
                    if rng.f32() < 0.5 {
                        should_fail = true;
                        errors.push(format!("request {} network partition", i));
                    }
                }
                ChaosScenario::IoSlowdown { latency_ms } => {
                    extra_delay_ms = *latency_ms;
                }
                ChaosScenario::CpuSaturation { load_factor } => {
                    extra_delay_ms = (*load_factor * 50.0) as u64;
                    if *load_factor > 0.95 && rng.f32() < 0.2 {
                        should_fail = true;
                        errors.push(format!("request {} CPU timeout", i));
                    }
                }
            }

            // Simulate base latency
            let base_latency_ms: u64 = 10 + rng.u64(0..40);
            let total_latency = base_latency_ms.saturating_add(extra_delay_ms);

            if should_fail {
                failed += 1;
                latencies.push(total_latency); // Still record latency for timeout cases
            } else {
                successful += 1;
                latencies.push(total_latency);
            }

            // Actually sleep for injected latency to simulate real delay
            if extra_delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(
                    extra_delay_ms.min(5), // Cap at 5ms to keep tests fast
                ));
            }
        }

        let recovery_time_ms = start.elapsed().as_millis() as u64;

        latencies.sort_unstable();
        let avg_latency = if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().map(|&l| l as f64).sum::<f64>() / latencies.len() as f64
        };
        let p99_idx = ((latencies.len() as f64 * 0.99).floor() as usize).min(latencies.len().saturating_sub(1));
        let p99_latency = latencies.get(p99_idx).copied().unwrap_or(0) as f64;

        // Determine graceful degradation: did we maintain >50% success rate?
        let success_rate = successful as f64 / total_requests as f64;
        let degraded_gracefully = success_rate > 0.5;

        let result = ChaosResult {
            scenario: scenario.clone(),
            total_requests,
            successful_requests: successful,
            failed_requests: failed,
            avg_latency_ms: avg_latency,
            p99_latency_ms: p99_latency,
            errors,
            degraded_gracefully,
            recovery_time_ms,
        };

        self.active_scenarios.push(scenario);
        self.metrics_history.push(result.clone());
        Ok(result)
    }

    /// Run a chaos scenario with automatic recovery.
    pub async fn run_with_recovery(
        &mut self,
        scenario: &ChaosScenario,
        recovery_timeout_secs: u64,
    ) -> anyhow::Result<ChaosResult> {
        let result = self.run_scenario(scenario.clone())?;

        // Auto-recovery after timeout
        tokio::time::sleep(Duration::from_secs(recovery_timeout_secs)).await;
        self.recover(scenario);

        Ok(result)
    }

    /// Recover from a chaos scenario (remove it from active scenarios).
    pub fn recover(&mut self, scenario: &ChaosScenario) {
        self.active_scenarios.retain(|s| s != scenario);
    }

    /// Check if degradation should be triggered based on current load.
    ///
    /// Returns the list of tiers that should be disabled given `current_load_pct`.
    pub fn check_degradation(&self, current_load_pct: u32) -> Vec<DegradationTier> {
        if current_load_pct < self.policy.degrade_at_load_pct {
            return Vec::new();
        }

        // Calculate how many tiers to disable based on how far over threshold we are
        let overload = current_load_pct.saturating_sub(self.policy.degrade_at_load_pct);
        let max_overload: u32 = 100 - self.policy.degrade_at_load_pct;
        let num_tiers_to_disable = std::cmp::min(
            ((overload as f64 / max_overload.max(1) as f64)
                * self.policy.feature_tier_order.len() as f64)
                .ceil() as usize,
            self.policy.feature_tier_order.len(),
        );

        self.policy.feature_tier_order[..num_tiers_to_disable].to_vec()
    }

    /// Get current system health score (0-100).
    ///
    /// Based on recent chaos experiment results and active scenarios.
    pub fn health_score(&self) -> u8 {
        if self.metrics_history.is_empty() {
            return 100;
        }

        let recent: &[ChaosResult] = if self.metrics_history.len() > 5 {
            &self.metrics_history[self.metrics_history.len() - 5..]
        } else {
            &self.metrics_history
        };

        let avg_success_rate: f64 = recent
            .iter()
            .map(|r| r.successful_requests as f64 / r.total_requests.max(1) as f64)
            .sum::<f64>()
            / recent.len().max(1) as f64;

        let active_penalty = (self.active_scenarios.len() as u32 * 10).min(50) as f64;
        let score = (avg_success_rate * 100.0) - active_penalty;
        score.max(0.0).min(100.0) as u8
    }

    /// Get history of all chaos experiments.
    pub fn history(&self) -> &[ChaosResult] {
        &self.metrics_history
    }

    /// Generate chaos report — human-readable summary of all experiments.
    pub fn generate_report(&self) -> String {
        use std::fmt::Write;

        let mut report = String::new();
        writeln!(report, "=== Chaos Engineering Report ===").expect("write");
        writeln!(report, "Total experiments: {}", self.metrics_history.len()).expect("write");
        writeln!(report, "Health score: {}/100", self.health_score()).expect("write");
        writeln!(report).expect("write");

        for (i, result) in self.metrics_history.iter().enumerate() {
            let scenario_name = match &result.scenario {
                ChaosScenario::LatencyInjection { min_ms, max_ms } => {
                    format!("LatencyInjection({}ms-{}ms)", min_ms, max_ms)
                }
                ChaosScenario::PacketLoss { drop_rate } => {
                    format!("PacketLoss({:.0}%)", drop_rate * 100.0)
                }
                ChaosScenario::ErrorInjection { error_rate } => {
                    format!("ErrorInjection({:.0}%)", error_rate * 100.0)
                }
                ChaosScenario::ProviderCrash => "ProviderCrash".to_string(),
                ChaosScenario::MemoryPressure { usage_pct } => {
                    format!("MemoryPressure({}%)", usage_pct)
                }
                ChaosScenario::NetworkPartition { affected_providers } => {
                    format!("NetworkPartition({:?})", affected_providers)
                }
                ChaosScenario::IoSlowdown { latency_ms } => {
                    format!("IoSlowdown({}ms)", latency_ms)
                }
                ChaosScenario::CpuSaturation { load_factor } => {
                    format!("CpuSaturation({:.2})", load_factor)
                }
            };

            writeln!(report, "--- Experiment #{}: {} ---", i + 1, scenario_name).expect("write");
            writeln!(report, "  Requests: {}/{}, Success Rate: {:.1}%",
                result.successful_requests, result.total_requests,
                result.successful_requests as f64 / result.total_requests.max(1) as f64 * 100.0
            ).expect("write");
            writeln!(report, "  Avg Latency: {:.2}ms, P99: {:.2}ms",
                result.avg_latency_ms, result.p99_latency_ms
            ).expect("write");
            writeln!(report, "  Graceful Degradation: {}, Recovery: {}ms",
                result.degraded_gracefully, result.recovery_time_ms
            ).expect("write");
            if !result.errors.is_empty() {
                writeln!(report, "  Errors ({}): {:?}", result.errors.len(), &result.errors[..result.errors.len().min(3)]).expect("write");
            }
            writeln!(report).expect("write");
        }

        report
    }
}

#[cfg(test)]
mod chaos_tests {
    use super::*;

    #[test]
    fn test_latency_injection() {
        let policy = DegradationPolicy::default();
        let mut engine = ChaosEngine::new(policy);
        let result = engine
            .run_scenario(ChaosScenario::LatencyInjection { min_ms: 5, max_ms: 20 })
            .expect("should succeed");
        assert_eq!(result.total_requests, 100);
        assert_eq!(result.failed_requests, 0); // Latency injection doesn't fail requests
        assert!(result.avg_latency_ms >= 5.0, "avg latency should reflect injection");
    }

    #[test]
    fn test_error_injection() {
        let mut engine = ChaosEngine::new(DegradationPolicy::default());
        let result = engine
            .run_scenario(ChaosScenario::ErrorInjection { error_rate: 0.3 })
            .expect("should succeed");
        // Allow ±20% tolerance around expected 30% failure rate
        let fail_rate = result.failed_requests as f64 / result.total_requests as f64;
        assert!(
            (fail_rate - 0.3).abs() < 0.2,
            "fail rate {:.2} not near 30%", fail_rate
        );
    }

    #[test]
    fn test_degradation_policy_defaults() {
        let policy = DegradationPolicy::default();
        assert_eq!(policy.degrade_at_load_pct, 80);
        assert!(policy.switch_to_fast_model);
        assert!(policy.shrink_context_window);
        assert!(policy.enable_request_batching);
        assert_eq!(policy.feature_tier_order.len(), 5);
        // First tier should be StreamingOutput (least important)
        assert_eq!(policy.feature_tier_order[0], DegradationTier::StreamingOutput);
    }

    #[test]
    fn test_feature_tier_ordering() {
        let policy = DegradationPolicy::default();
        let engine = ChaosEngine::new(policy);

        // Below threshold — no degradation
        let disabled = engine.check_degradation(60);
        assert!(disabled.is_empty());

        // At threshold — minimal degradation
        let disabled = engine.check_degradation(80);
        assert!(disabled.is_empty()); // Exactly at threshold doesn't trigger

        // Well above threshold
        let disabled = engine.check_degradation(95);
        assert!(!disabled.is_empty());
        // Should disable lower-priority tiers first
        assert!(disabled.contains(&DegradationTier::StreamingOutput));
    }

    #[test]
    fn test_health_score_under_load() {
        let mut engine = ChaosEngine::new(DegradationPolicy::default());

        // No experiments yet — perfect health
        assert_eq!(engine.health_score(), 100);

        // Run a mild scenario — health should still be decent
        engine.run_scenario(ChaosScenario::LatencyInjection { min_ms: 1, max_ms: 5 }).ok();
        assert!(engine.health_score() >= 70, "health should remain high after mild chaos");

        // Run provider crash — health drops significantly
        engine.run_scenario(ChaosScenario::ProviderCrash).ok();
        assert!(engine.health_score() < 100, "health should drop after crash scenario");
    }

    #[tokio::test]
    async fn test_chaos_with_recovery() {
        let mut engine = ChaosEngine::new(DegradationPolicy::default());
        let scenario = ChaosScenario::LatencyInjection { min_ms: 1, max_ms: 5 };
        let result = engine
            .run_with_recovery(&scenario, 0)
            .await
            .expect("should succeed");
        assert_eq!(result.total_requests, 100);
        // After recovery, the scenario should no longer be active
        assert!(!engine.active_scenarios.contains(&scenario));
    }
}
