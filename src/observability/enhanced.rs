//! Enhanced observability — health checks, structured metrics, Prometheus export.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use serde::{Serialize, Deserialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

/// Application health status.
#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    /// Overall health: "healthy", "degraded", "unhealthy".
    pub status: HealthLevel,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Version string.
    pub version: String,
    /// Individual component health.
    pub components: HashMap<String, ComponentHealth>,
    /// Timestamp of this check.
    pub checked_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HealthLevel {
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthLevel::Healthy => "healthy",
            HealthLevel::Degraded => "degraded",
            HealthLevel::Unhealthy => "unhealthy",
        }
    }
}

impl std::fmt::Display for HealthLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentHealth {
    pub name: String,
    pub status: HealthLevel,
    pub latency_ms: Option<u64>,
    pub message: Option<String>,
    /// Extra metrics for this component.
    pub metrics: HashMap<String, f64>,
}

/// Point-in-time metrics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricsSnapshot {
    // Counters (monotonically increasing)
    pub api_requests_total: u64,
    pub api_errors_total: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub tokens_generated_total: u64,
    pub tokens_input_total: u64,
    pub edits_applied: u64,
    pub edits_rolled_back: u64,
    pub sessions_total: u64,

    // Gauges (current value)
    pub active_sessions: u64,
    pub pending_requests: u64,
    pub queue_depth: u64,
    pub memory_usage_mb: f64,
    pub cpu_usage_pct: f64,

    // Histogram-like summaries
    pub avg_request_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub avg_tokens_per_request: f64,

    // Cache-specific
    pub cache_hit_rate: f64,
    pub cache_size_bytes: u64,

    // Cost tracking
    pub total_cost_usd: f64,
    pub cost_per_session_avg: f64,
}

/// Metrics collector — accumulates counters and gauges.
pub struct MetricsCollector {
    inner: Arc<RwLock<MetricsSnapshot>>,
    start_time: std::time::Instant,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MetricsSnapshot::default())),
            start_time: std::time::Instant::now(),
        }
    }

    /// Increment a counter by name.
    pub async fn inc_counter(&self, name: &str, value: u64) {
        let mut snap = self.inner.write().await;
        match name {
            "api_requests_total" => snap.api_requests_total += value,
            "api_errors_total" => snap.api_errors_total += value,
            "cache_hits" => snap.cache_hits += value,
            "cache_misses" => snap.cache_misses += value,
            "tokens_generated_total" => snap.tokens_generated_total += value,
            "tokens_input_total" => snap.tokens_input_total += value,
            "edits_applied" => snap.edits_applied += value,
            "edits_rolled_back" => snap.edits_rolled_back += value,
            "sessions_total" => snap.sessions_total += value,
            _ => {} // Unknown counter — silently ignore
        }
    }

    /// Set a gauge value by name.
    pub async fn set_gauge(&self, name: &str, value: f64) {
        let mut snap = self.inner.write().await;
        match name {
            "active_sessions" => snap.active_sessions = value as u64,
            "pending_requests" => snap.pending_requests = value as u64,
            "queue_depth" => snap.queue_depth = value as u64,
            "memory_usage_mb" => snap.memory_usage_mb = value,
            "cpu_usage_pct" => snap.cpu_usage_pct = value,
            "avg_request_latency_ms" => snap.avg_request_latency_ms = value,
            "p99_latency_ms" => snap.p99_latency_ms = value,
            "avg_tokens_per_request" => snap.avg_tokens_per_request = value,
            "cache_hit_rate" => snap.cache_hit_rate = value,
            "cache_size_bytes" => snap.cache_size_bytes = value as u64,
            "total_cost_usd" => snap.total_cost_usd = value,
            "cost_per_session_avg" => snap.cost_per_session_avg = value,
            _ => {}
        }
    }

    /// Record a request (updates multiple metrics at once).
    pub async fn record_request(
        &self,
        latency_ms: u64,
        tokens: u64,
        success: bool,
        cached: bool,
    ) {
        let mut snap = self.inner.write().await;
        snap.api_requests_total += 1;
        snap.tokens_generated_total += tokens;
        if !success {
            snap.api_errors_total += 1;
        }
        if cached {
            snap.cache_hits += 1;
        } else {
            snap.cache_misses += 1;
        }
        // Update running average latency
        let n = snap.api_requests_total as f64;
        let prev_avg = snap.avg_request_latency_ms;
        snap.avg_request_latency_ms = prev_avg + (latency_ms as f64 - prev_avg) / n;
        // Update avg tokens per request
        let prev_tokens = snap.avg_tokens_per_request;
        snap.avg_tokens_per_request =
            prev_tokens + (tokens as f64 - prev_tokens) / n;
        // Update cache hit rate
        let total_ops = snap.cache_hits + snap.cache_misses;
        if total_ops > 0 {
            snap.cache_hit_rate = snap.cache_hits as f64 / total_ops as f64;
        }
    }

    /// Get current snapshot.
    pub async fn snapshot(&self) -> MetricsSnapshot {
        self.inner.read().await.clone()
    }

    /// Export in Prometheus text exposition format.
    pub async fn prometheus_export(&self) -> String {
        let snap = self.inner.read().await;
        let mut out = String::new();
        out.push_str("# DeepSeek Carp metrics\n");

        macro_rules! counter_line {
            ($name:expr, $val:expr) => {
                out.push_str(&format!(
                    "# TYPE dscarp_{} counter\n",
                    $name
                ));
                out.push_str(&format!("dscarp_{}\n", $val));
            };
        }
        macro_rules! gauge_line {
            ($name:expr, $val:expr) => {
                out.push_str(&format!(
                    "# TYPE dscarp_{} gauge\n",
                    $name
                ));
                out.push_str(&format!("dscarp_{}\n", $val));
            };
        }

        counter_line!("api_requests_total", snap.api_requests_total);
        counter_line!("api_errors_total", snap.api_errors_total);
        counter_line!("cache_hits", snap.cache_hits);
        counter_line!("cache_misses", snap.cache_misses);
        counter_line!("tokens_generated_total", snap.tokens_generated_total);
        counter_line!("tokens_input_total", snap.tokens_input_total);
        counter_line!("edits_applied", snap.edits_applied);
        counter_line!("edits_rolled_back", snap.edits_rolled_back);
        counter_line!("sessions_total", snap.sessions_total);

        gauge_line!("active_sessions", snap.active_sessions);
        gauge_line!("pending_requests", snap.pending_requests);
        gauge_line!("queue_depth", snap.queue_depth);
        gauge_line!("memory_usage_mb", snap.memory_usage_mb);
        gauge_line!("cpu_usage_pct", snap.cpu_usage_pct);
        gauge_line!("avg_request_latency_ms", snap.avg_request_latency_ms);
        gauge_line!("p99_latency_ms", snap.p99_latency_ms);
        gauge_line!("avg_tokens_per_request", snap.avg_tokens_per_request);
        gauge_line!("cache_hit_rate", snap.cache_hit_rate);
        gauge_line!("cache_size_bytes", snap.cache_size_bytes);
        gauge_line!("total_cost_usd", snap.total_cost_usd);
        gauge_line!("cost_per_session_avg", snap.cost_per_session_avg);

        out
    }

    /// Export as JSON.
    pub async fn json_export(&self) -> String {
        let snap = self.snapshot().await;
        serde_json::to_string_pretty(&snap).expect("MetricsSnapshot is serializable")
    }

    /// Get uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SLA Dashboard
// ---------------------------------------------------------------------------

/// SLA dashboard: track uptime, request counts, and latency SLAs.
pub struct SlaDashboard {
    /// Service start time
    start_time: std::time::Instant,
    /// Total requests
    total_requests: Arc<AtomicU64>,
    /// Successful requests
    successful_requests: Arc<AtomicU64>,
    /// Failed requests
    failed_requests: Arc<AtomicU64>,
    /// P99 latency tracking
    latency_samples: Arc<Mutex<Vec<f64>>>,
    /// Uptime status
    is_healthy: Arc<AtomicBool>,
}

impl SlaDashboard {
    pub fn new() -> Self {
        Self {
            start_time: std::time::Instant::now(),
            total_requests: Arc::new(AtomicU64::new(0)),
            successful_requests: Arc::new(AtomicU64::new(0)),
            failed_requests: Arc::new(AtomicU64::new(0)),
            latency_samples: Arc::new(Mutex::new(Vec::new())),
            is_healthy: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Record a request completion.
    pub fn record_request(&self, success: bool) {
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        if success {
            self.successful_requests.fetch_add(1, Ordering::SeqCst);
        } else {
            self.failed_requests.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Record a latency sample.
    pub fn record_latency(&self, latency_ms: f64) {
        let mut samples = self.latency_samples.lock().unwrap();
        samples.push(latency_ms);
        if samples.len() > 10000 {
            samples.drain(0..1000);
        }
    }

    /// Get current SLA report.
    pub fn report(&self) -> SlaReport {
        let total = self.total_requests.load(Ordering::SeqCst);
        let success = self.successful_requests.load(Ordering::SeqCst);
        let failed = self.failed_requests.load(Ordering::SeqCst);
        let uptime_secs = self.start_time.elapsed().as_secs();

        let availability = if total > 0 {
            (success as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        let p99 = self.compute_p99();

        SlaReport {
            uptime_secs,
            total_requests: total,
            successful_requests: success,
            failed_requests: failed,
            availability_pct: availability,
            p99_latency_ms: p99,
            status: if availability > 99.0 {
                "Healthy"
            } else if availability > 95.0 {
                "Degraded"
            } else {
                "Unhealthy"
            },
        }
    }

    /// Compute P99 latency.
    fn compute_p99(&self) -> f64 {
        let samples = self.latency_samples.lock().unwrap();
        if samples.is_empty() {
            return 0.0;
        }
        let mut sorted = samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = (sorted.len() as f64 * 0.99) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }
}

impl Default for SlaDashboard {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SlaReport {
    pub uptime_secs: u64,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub availability_pct: f64,
    pub p99_latency_ms: f64,
    pub status: &'static str,
}

// ---------------------------------------------------------------------------
// Alerting types
// ---------------------------------------------------------------------------

/// Alert rule that triggers when a metric exceeds a threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub metric: AlertMetric,
    pub condition: AlertCondition,
    pub threshold: f64,
    pub duration_secs: u64,
    pub severity: AlertSeverity,
    pub description: String,
    pub channels: Vec<AlertChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertMetric {
    ErrorRate,
    P99Latency,
    CacheHitRate,
    QueueDepth,
    MemoryUsage,
    CpuUsage,
    ActiveSessions,
    TokenUsageRate,
}

impl std::fmt::Display for AlertMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertMetric::ErrorRate => write!(f, "error_rate"),
            AlertMetric::P99Latency => write!(f, "p99_latency"),
            AlertMetric::CacheHitRate => write!(f, "cache_hit_rate"),
            AlertMetric::QueueDepth => write!(f, "queue_depth"),
            AlertMetric::MemoryUsage => write!(f, "memory_usage"),
            AlertMetric::CpuUsage => write!(f, "cpu_usage"),
            AlertMetric::ActiveSessions => write!(f, "active_sessions"),
            AlertMetric::TokenUsageRate => write!(f, "token_usage_rate"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertCondition {
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertChannel {
    Stdout,
    Stderr,
    File(PathBuf),
    HttpEndpoint(String),
}

impl Default for AlertRule {
    fn default() -> Self {
        Self {
            name: "default".into(),
            metric: AlertMetric::ErrorRate,
            condition: AlertCondition::GreaterThan,
            threshold: 0.05,
            duration_secs: 60,
            severity: AlertSeverity::Warning,
            description: "Default alert rule".into(),
            channels: vec![AlertChannel::Stderr],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FiredAlert {
    pub rule_name: String,
    pub current_value: f64,
    pub threshold: f64,
    pub severity: AlertSeverity,
    pub fired_at: u64,
    pub message: String,
}

/// Manages alert rules and fires alerts when thresholds are breached.
pub struct AlertManager {
    rules: Vec<AlertRule>,
    /// History of metric evaluations (for duration-based alerting)
    evaluation_history: Arc<RwLock<HashMap<String, Vec<(u64, f64)>>>>,
}

impl AlertManager {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            evaluation_history: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_rules(rules: Vec<AlertRule>) -> Self {
        Self {
            rules,
            evaluation_history: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.push(rule);
    }

    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// Evaluate all rules against current metrics. Returns fired alerts.
    pub async fn evaluate(&self, metrics: &MetricsSnapshot) -> Vec<FiredAlert> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut fired = Vec::new();

        for rule in &self.rules {
            let value = match rule.metric {
                AlertMetric::ErrorRate => {
                    if metrics.api_requests_total == 0 {
                        continue;
                    }
                    metrics.api_errors_total as f64 / metrics.api_requests_total as f64
                }
                AlertMetric::P99Latency => metrics.p99_latency_ms,
                AlertMetric::CacheHitRate => metrics.cache_hit_rate,
                AlertMetric::QueueDepth => metrics.queue_depth as f64,
                AlertMetric::MemoryUsage => metrics.memory_usage_mb,
                AlertMetric::CpuUsage => metrics.cpu_usage_pct,
                AlertMetric::ActiveSessions => metrics.active_sessions as f64,
                AlertMetric::TokenUsageRate => metrics.tokens_generated_total as f64,
            };

            // Record in evaluation history
            let key = rule.name.clone();
            {
                let mut history = self.evaluation_history.write().await;
                let entries = history.entry(key.clone()).or_insert_with(Vec::new);
                entries.push((now, value));
                // Prune entries older than twice the duration
                let cutoff = now.saturating_sub(rule.duration_secs * 2);
                entries.retain(|(ts, _)| *ts >= cutoff);
            }

            // Check whether the current value breaches the threshold
            let breached = match rule.condition {
                AlertCondition::GreaterThan => value > rule.threshold,
                AlertCondition::LessThan => value < rule.threshold,
                AlertCondition::GreaterThanOrEqual => value >= rule.threshold,
                AlertCondition::LessThanOrEqual => value <= rule.threshold,
            };

            if !breached {
                continue;
            }

            // Duration check: all samples within the window must be breaching
            let history = self.evaluation_history.read().await;
            let entries = match history.get(&key) {
                Some(e) => e,
                None => continue,
            };

            let window_start = now.saturating_sub(rule.duration_secs);
            let window_entries: Vec<_> = entries.iter().filter(|(ts, _)| *ts >= window_start).collect();

            if window_entries.is_empty() {
                continue;
            }

            // Earliest entry must be old enough to cover duration_secs
            let earliest_ts = window_entries.iter().map(|(ts, _)| ts).min().copied().unwrap_or(now);
            if now - earliest_ts < rule.duration_secs {
                continue;
            }

            // All entries in the window must breach the threshold
            let all_breached = window_entries.iter().all(|(_, v)| match rule.condition {
                AlertCondition::GreaterThan => *v > rule.threshold,
                AlertCondition::LessThan => *v < rule.threshold,
                AlertCondition::GreaterThanOrEqual => *v >= rule.threshold,
                AlertCondition::LessThanOrEqual => *v <= rule.threshold,
            });

            if all_breached {
                fired.push(FiredAlert {
                    rule_name: rule.name.clone(),
                    current_value: value,
                    threshold: rule.threshold,
                    severity: rule.severity,
                    fired_at: now,
                    message: format!(
                        "Alert '{}': {} = {:.2} (threshold: {:.2})",
                        rule.name, rule.metric, value, rule.threshold
                    ),
                });
            }
        }

        fired
    }

    /// Default rules for production deployment.
    pub fn default_production_rules() -> Vec<AlertRule> {
        vec![
            AlertRule {
                name: "high-error-rate".into(),
                metric: AlertMetric::ErrorRate,
                threshold: 0.05,
                duration_secs: 300,
                severity: AlertSeverity::Critical,
                ..Default::default()
            },
            AlertRule {
                name: "high-latency".into(),
                metric: AlertMetric::P99Latency,
                threshold: 30_000.0,
                duration_secs: 120,
                severity: AlertSeverity::Warning,
                ..Default::default()
            },
            AlertRule {
                name: "low-cache-hit-rate".into(),
                metric: AlertMetric::CacheHitRate,
                condition: AlertCondition::LessThan,
                threshold: 0.7,
                duration_secs: 600,
                severity: AlertSeverity::Warning,
                ..Default::default()
            },
            AlertRule {
                name: "high-memory".into(),
                metric: AlertMetric::MemoryUsage,
                threshold: 2048.0,
                duration_secs: 120,
                severity: AlertSeverity::Critical,
                ..Default::default()
            },
            AlertRule {
                name: "queue-backlog".into(),
                metric: AlertMetric::QueueDepth,
                threshold: 100.0,
                duration_secs: 60,
                severity: AlertSeverity::Warning,
                ..Default::default()
            },
        ]
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Health checker — runs periodic health checks on components.
pub struct HealthChecker {
    components: Arc<RwLock<HashMap<String, ComponentHealth>>>,
    check_fns: Arc<RwLock<Vec<(String, Box<dyn Fn() -> ComponentHealth + Send + Sync>)>>>,
    start_time: std::time::Instant,
    pub sla_dashboard: Option<Arc<SlaDashboard>>,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            components: Arc::new(RwLock::new(HashMap::new())),
            check_fns: Arc::new(RwLock::new(Vec::new())),
            start_time: std::time::Instant::now(),
            sla_dashboard: None,
        }
    }

    /// Register a component to monitor.
    pub async fn register_component(
        &self,
        name: String,
        check_fn: impl Fn() -> ComponentHealth + Send + Sync + 'static,
    ) {
        let check_box: Box<dyn Fn() -> ComponentHealth + Send + Sync> = Box::new(check_fn);
        self.check_fns.write().await.push((name.clone(), check_box));
    }

    /// Run all health checks and return aggregate status.
    pub async fn check_all(&self) -> HealthStatus {
        let fns = self.check_fns.read().await;
        let mut components = HashMap::new();
        let mut overall = HealthLevel::Healthy;

        for (_name, check_fn) in fns.iter() {
            let health = check_fn();
            match health.status {
                HealthLevel::Unhealthy => overall = HealthLevel::Unhealthy,
                HealthLevel::Degraded if overall == HealthLevel::Healthy => {
                    overall = HealthLevel::Degraded
                }
                _ => {}
            }
            components.insert(health.name.clone(), health);
        }
        drop(fns);

        *self.components.write().await = components.clone();

        HealthStatus {
            status: overall,
            uptime_secs: self.start_time.elapsed().as_secs(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            components,
            checked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    /// Quick check: is everything healthy?
    pub async fn is_healthy(&self) -> bool {
        let status = self.check_all().await;
        status.status != HealthLevel::Unhealthy
    }

    /// Generate health endpoint response (JSON).
    pub async fn health_response(&self) -> String {
        let status = self.check_all().await;
        serde_json::to_string_pretty(&status).expect("HealthStatus is serializable")
    }

    /// Generate readiness response (for Kubernetes-style probes).
    pub async fn ready_response(&self) -> String {
        let status = self.check_all().await;
        if status.status == HealthLevel::Unhealthy {
            format!("{{\"ready\":false,\"status\":\"{}\"}}", status.status)
        } else {
            "{\"ready\":true,\"status\":\"healthy\"}".to_string()
        }
    }

    /// Get combined SLA + health report as JSON.
    pub async fn full_report(&self) -> serde_json::Value {
        let health = self.check_all().await;
        let sla = if let Some(ref d) = self.sla_dashboard {
            d.report()
        } else {
            SlaReport {
                uptime_secs: 0,
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                availability_pct: 100.0,
                p99_latency_ms: 0.0,
                status: "N/A",
            }
        };

        serde_json::json!({
            "health": health,
            "sla": sla,
        })
    }

    /// Serve health, metrics, and alerts endpoints over HTTP.
    ///
    /// - `GET /health` → JSON `HealthStatus` (200 if healthy/degraded, 503 if unhealthy)
    /// - `GET /metrics` → JSON `MetricsSnapshot`
    /// - `GET /alerts` → JSON `Vec<FiredAlert>`
    ///
    /// Runs until the server encounters a fatal error.
    pub async fn serve_http(
        &self,
        port: u16,
        metrics_collector: Option<Arc<MetricsCollector>>,
        alert_manager: Option<Arc<AlertManager>>,
    ) -> std::io::Result<()> {
        let addr = format!("127.0.0.1:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        let components = self.components.clone();
        let check_fns = self.check_fns.clone();
        let start_time = self.start_time;
        let sla_dashboard = self.sla_dashboard.clone();

        tracing::info!("Health HTTP server listening on http://{}", addr);

        loop {
            let (mut stream, _) = listener.accept().await?;
            let components = components.clone();
            let check_fns = check_fns.clone();
            let mc = metrics_collector.clone();
            let am = alert_manager.clone();
            let sla = sla_dashboard.clone();

            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };

                let request = String::from_utf8_lossy(&buf[..n]);
                let response = build_http_response(&request, &components, &check_fns, start_time, &mc, &am, &sla).await;

                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// HTTP response builder for health / metrics / alerts endpoint
// ---------------------------------------------------------------------------

async fn build_http_response(
    request: &str,
    _components: &Arc<RwLock<HashMap<String, ComponentHealth>>>,
    check_fns: &Arc<RwLock<Vec<(String, Box<dyn Fn() -> ComponentHealth + Send + Sync>)>>>,
    start_time: std::time::Instant,
    metrics_collector: &Option<Arc<MetricsCollector>>,
    alert_manager: &Option<Arc<AlertManager>>,
    sla_dashboard: &Option<Arc<SlaDashboard>>,
) -> String {
    if request.starts_with("GET /health") {
        // Run health checks manually
        let fns = check_fns.read().await;
        let mut comps = HashMap::new();
        let mut overall = HealthLevel::Healthy;

        for (_name, check_fn) in fns.iter() {
            let health = check_fn();
            match health.status {
                HealthLevel::Unhealthy => overall = HealthLevel::Unhealthy,
                HealthLevel::Degraded if overall == HealthLevel::Healthy => {
                    overall = HealthLevel::Degraded;
                }
                _ => {}
            }
            comps.insert(health.name.clone(), health);
        }
        drop(fns);

        let status = HealthStatus {
            status: overall,
            uptime_secs: start_time.elapsed().as_secs(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            components: comps,
            checked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        let body = serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".into());
        let (code, reason) = if overall == HealthLevel::Unhealthy {
            ("503", "Service Unavailable")
        } else {
            ("200", "OK")
        };
        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            code,
            reason,
            body.len(),
            body
        )
    } else if request.starts_with("GET /metrics") {
        let body = if let Some(ref collector) = metrics_collector {
            collector.json_export().await
        } else {
            "{}".to_string()
        };
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    } else if request.starts_with("GET /alerts") {
        let body = match (alert_manager, metrics_collector) {
            (Some(manager), Some(collector)) => {
                let metrics = collector.snapshot().await;
                let alerts = manager.evaluate(&metrics).await;
                serde_json::to_string_pretty(&alerts).unwrap_or_else(|_| "[]".into())
            }
            _ => "[]".to_string(),
        };
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    } else if request.starts_with("GET /report") {
        let sla = if let Some(ref d) = sla_dashboard {
            d.report()
        } else {
            SlaReport {
                uptime_secs: 0,
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                availability_pct: 100.0,
                p99_latency_ms: 0.0,
                status: "N/A",
            }
        };
        let body = serde_json::to_string_pretty(&sla).unwrap_or_else(|_| "{}".into());
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    } else {
        let body = r#"{"error":"not found"}"#;
        format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_counter_inc() {
        let mc = MetricsCollector::new();
        mc.inc_counter("api_requests_total", 5).await;
        mc.inc_counter("api_requests_total", 3).await;
        let snap = mc.snapshot().await;
        assert_eq!(snap.api_requests_total, 8);

        // Unknown counter should be ignored without error
        mc.inc_counter("nonexistent_counter", 1).await;
    }

    #[tokio::test]
    async fn test_gauge_set() {
        let mc = MetricsCollector::new();
        mc.set_gauge("memory_usage_mb", 256.5).await;
        mc.set_gauge("cpu_usage_pct", 42.0).await;
        let snap = mc.snapshot().await;
        assert!((snap.memory_usage_mb - 256.5).abs() < 0.001);
        assert!((snap.cpu_usage_pct - 42.0).abs() < 0.001);

        // Unknown gauge should be ignored
        mc.set_gauge("nonexistent_gauge", 1.0).await;
    }

    #[tokio::test]
    async fn test_prometheus_export() {
        let mc = MetricsCollector::new();
        mc.inc_counter("api_requests_total", 42).await;
        mc.set_gauge("cache_hit_rate", 0.85).await;
        let export = mc.prometheus_export().await;

        assert!(export.contains("# TYPE dscarp_api_requests_total counter"));
        assert!(export.contains("dscarp_api_requests_total 42"));
        assert!(export.contains("# TYPE dscarp_cache_hit_rate gauge"));
        assert!(export.contains("dscarp_cache_hit_rate 0.85"));
    }

    #[tokio::test]
    async fn test_health_check() {
        let hc = HealthChecker::new();
        hc.register_component(
            "database".to_string(),
            || ComponentHealth {
                name: "database".to_string(),
                status: HealthLevel::Healthy,
                latency_ms: Some(5),
                message: Some("OK".into()),
                metrics: HashMap::new(),
            },
        )
        .await;
        hc.register_component(
            "cache".to_string(),
            || ComponentHealth {
                name: "cache".to_string(),
                status: HealthLevel::Degraded,
                latency_ms: Some(150),
                message: Some("High latency".into()),
                metrics: HashMap::new(),
            },
        )
        .await;

        let status = hc.check_all().await;
        assert_eq!(status.status, HealthLevel::Degraded);
        assert!(status.components.contains_key("database"));
        assert!(status.components.contains_key("cache"));
    }

    #[tokio::test]
    async fn test_json_export() {
        let mc = MetricsCollector::new();
        mc.inc_counter("sessions_total", 7).await;
        let json = mc.json_export().await;
        let parsed: MetricsSnapshot =
            serde_json::from_str(&json).expect("Valid JSON");
        assert_eq!(parsed.sessions_total, 7);
    }

    #[tokio::test]
    async fn test_record_request() {
        let mc = MetricsCollector::new();
        mc.record_request(100, 50, true, false).await;
        mc.record_request(200, 80, false, true).await;

        let snap = mc.snapshot().await;
        assert_eq!(snap.api_requests_total, 2);
        assert_eq!(snap.tokens_generated_total, 130); // 50 + 80
        assert_eq!(snap.api_errors_total, 1);
        assert_eq!(snap.cache_hits, 1);
        assert_eq!(snap.cache_misses, 1);
        assert!((snap.cache_hit_rate - 0.5).abs() < 0.001);
        assert!((snap.avg_request_latency_ms - 150.0).abs() < 0.001); // (100+200)/2
    }

    #[tokio::test]
    async fn test_sla_dashboard_new() {
        let sla = SlaDashboard::new();
        let report = sla.report();
        assert_eq!(report.total_requests, 0);
        assert_eq!(report.availability_pct, 100.0);
        assert_eq!(report.status, "Healthy");
    }

    #[tokio::test]
    async fn test_sla_report_basic() {
        let sla = SlaDashboard::new();
        sla.record_request(true);
        sla.record_request(true);
        sla.record_request(false);
        let report = sla.report();
        assert_eq!(report.total_requests, 3);
        assert_eq!(report.successful_requests, 2);
        assert_eq!(report.failed_requests, 1);
        assert!((report.availability_pct - 66.666666).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_sla_p99_computation() {
        let sla = SlaDashboard::new();
        // Add 100 latency samples: 1..100
        for i in 1..=100 {
            sla.record_latency(i as f64);
        }
        let report = sla.report();
        // P99 should be ~99 (the 99th percentile of [1..=100])
        assert!(report.p99_latency_ms >= 98.0 && report.p99_latency_ms <= 100.0);
    }
}
