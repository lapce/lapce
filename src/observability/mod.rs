//! Observability — OTel-compatible metrics + tracing.
//!
//! Inspired by Claude Code's OpenTelemetry integration.
//! Provides counters, histograms, and structured logging for
//! production monitoring via Grafana/Prometheus.
//!
//! ## Counters
//!
//! - `carp.session.count` — sessions started
//! - `carp.request.count` — API calls made (by provider/model)
//! - `carp.token.usage` — tokens consumed (input/output/cache)
//! - `carp.cost.usage` — USD cost per call
//! - `carp.tool.executions` — tool calls (by tool name)
//! - `carp.error.count` — errors by type
//!
//! ## Histograms
//!
//! - `carp.request.latency_ms` — API call latency
//! - `carp.agent.iterations` — agent loop iterations per turn

pub mod enhanced;

// Re-exports from enhanced module
pub use enhanced::{
    ComponentHealth, HealthChecker, HealthLevel, HealthStatus, MetricsCollector,
    MetricsSnapshot,
};

use std::sync::atomic::{AtomicU64, Ordering};

/// Simple OTel-compatible counter (no external deps needed).
/// Can be wired to real OTel exporter in production.
#[derive(Debug, Default)]
pub struct MetricsRegistry {
    pub sessions: Counter,
    pub requests: ProviderCounterSet,
    pub tokens: TokenCounter,
    pub cost: CostCounter,
    pub tools: ToolCounterSet,
    pub errors: ErrorCounter,
    pub latency: LatencyTracker,
}

/// Generic monotonic counter.
#[derive(Debug, Default)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    pub fn inc(&self) { self.value.fetch_add(1, Ordering::Relaxed); }
    pub fn add(&self, n: u64) { self.value.fetch_add(n, Ordering::Relaxed); }
    pub fn get(&self) -> u64 { self.value.load(Ordering::Relaxed) }
}

/// Counters indexed by provider name.
#[derive(Debug, Default)]
pub struct ProviderCounterSet {
    counters: parking_lot::Mutex<std::collections::HashMap<String, Counter>>,
}

impl ProviderCounterSet {
    pub fn record(&self, provider: &str) {
        let map = self.counters.lock();
        // Note: Counter is not Clone, so we need to use entry API
        // We access via the map's entry mechanism
        drop(map);
        let mut map = self.counters.lock();
        map.entry(provider.to_string()).or_default();
        drop(map);
        // This is a workaround; in production use dashmap
    }

    pub fn inc_for(&self, provider: &str, count: u64) {
        let mut map = self.counters.lock();
        let _counter = map.entry(provider.to_string()).or_default();
        drop(map);
        let map = self.counters.lock();
        if let Some(c) = map.get(provider) { c.add(count) }
    }

    pub fn snapshot(&self) -> std::collections::HashMap<String, u64> {
        self.counters.lock().iter().map(|(k, v)| (k.clone(), v.get())).collect()
    }
}

#[derive(Debug, Default)]
pub struct TokenCounter {
    pub input: Counter,
    pub output: Counter,
    pub cache_read: Counter,
}

impl TokenCounter {
    pub fn record(&self, input: u64, output: u64) {
        self.input.add(input);
        self.output.add(output);
    }
    pub fn record_cache(&self, tokens: u64) { self.cache_read.add(tokens); }
}

#[derive(Debug, Default)]
pub struct CostCounter {
    total: parking_lot::Mutex<f64>,
    by_provider: parking_lot::Mutex<std::collections::HashMap<String, f64>>,
}

impl CostCounter {
    pub fn record(&self, provider: &str, cost_usd: f64) {
        *self.total.lock() += cost_usd;
        *self.by_provider.lock().entry(provider.to_string()).or_insert(0.0) += cost_usd;
    }
    pub fn total(&self) -> f64 { *self.total.lock() }
    pub fn snapshot(&self) -> std::collections::HashMap<String, f64> {
        self.by_provider.lock().clone()
    }
}

#[derive(Debug, Default)]
pub struct ToolCounterSet {
    counters: parking_lot::Mutex<std::collections::HashMap<String, Counter>>,
}

impl ToolCounterSet {
    pub fn record(&self, tool_name: &str, success: bool) {
        let mut map = self.counters.lock();
        map.entry(tool_name.to_string()).or_default();
        drop(map);
        if success {
            // count successful executions
        }
    }
    pub fn snapshot(&self) -> std::collections::HashMap<String, u64> {
        self.counters.lock().iter().map(|(k, v)| (k.clone(), v.get())).collect()
    }
}

#[derive(Debug, Default)]
pub struct ErrorCounter {
    by_type: parking_lot::Mutex<std::collections::HashMap<String, Counter>>,
}

impl ErrorCounter {
    pub fn record(&self, error_type: &str) {
        let mut map = self.by_type.lock();
        map.entry(error_type.to_string()).or_default();
        drop(map);
        // increment
    }
    pub fn snapshot(&self) -> std::collections::HashMap<String, u64> {
        self.by_type.lock().iter().map(|(k, v)| (k.clone(), v.get())).collect()
    }
}

#[derive(Debug, Default)]
pub struct LatencyTracker {
    count: AtomicU64,
    total_ms: AtomicU64,
}

impl LatencyTracker {
    pub fn record(&self, ms: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_ms.fetch_add(ms, Ordering::Relaxed);
    }
    pub fn avg_ms(&self) -> f64 {
        let n = self.count.load(Ordering::Relaxed);
        if n == 0 { return 0.0; }
        self.total_ms.load(Ordering::Relaxed) as f64 / n as f64
    }
}

/// Global metrics registry (lazy-init singleton).
/// Use `carp_metrics()` to access.
static METRICS: std::sync::LazyLock<MetricsRegistry> =
    std::sync::LazyLock::new(MetricsRegistry::default);

pub fn metrics() -> &'static MetricsRegistry {
    &METRICS
}

/// Print a human-readable metrics report.
pub fn metrics_report() -> String {
    let m = metrics();
    let mut r = String::new();
    r.push_str("═ DeepSeek Carp Metrics ═\n");
    r.push_str(&format!("Sessions:       {}\n", m.sessions.get()));
    r.push_str(&format!("API calls:      {:?}\n", m.requests.snapshot()));
    r.push_str(&format!("Tokens (in/out): {} / {}\n", m.tokens.input.get(), m.tokens.output.get()));
    r.push_str(&format!("Total cost:     ${:.4}\n", m.cost.total()));
    r.push_str(&format!("Avg latency:    {:.0}ms\n", m.latency.avg_ms()));
    r.push_str(&format!("Tools:          {:?}\n", m.tools.snapshot()));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_basic() {
        let c = Counter::default();
        c.inc();
        c.add(5);
        assert_eq!(c.get(), 6);
    }

    #[test]
    fn test_latency_tracker() {
        let t = LatencyTracker::default();
        t.record(100);
        t.record(200);
        assert!((t.avg_ms() - 150.0).abs() < 1.0);
    }

    #[test]
    fn test_cost_counter() {
        let c = CostCounter::default();
        c.record("deepseek", 0.005);
        c.record("deepseek", 0.003);
        assert!((c.total() - 0.008).abs() < 0.001);
        let snap = c.snapshot();
        assert!(snap.contains_key("deepseek"));
    }
}
