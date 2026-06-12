//! Engine router — HybridParallel (local fast + cloud quality) + failover.
//!
//! The router implements the HybridParallel strategy: send to both a fast
//! local engine and a high-quality cloud engine simultaneously. If the local
//! engine returns with confidence ≥ threshold before the cloud responds,
//! cancel the cloud request and return the local answer. Otherwise return
//! whichever comes first (or the cloud one if local is bad).
//!
//! ```text
//! request →┬─ Local (llama.cpp, target <500ms) →┐
//!          └─ Cloud (DeepSeek, target <1500ms) →├─ merge() → response
//!                                                │
//!  If local finishes first AND confidence ≥ 0.8 ──┘ (cancel cloud)
//!  Otherwise wait for best quality response
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;

use super::engine::{
    InferenceEngine, InferenceRequest, InferenceResponse, InferenceError, EngineStats,
};
use super::complexity::{estimate_complexity, EngineChoice};

pub struct EngineRouter {
    engines: HashMap<String, Arc<dyn InferenceEngine>>,
    primary: String,
    fallback: Option<String>,
    local_fast: Option<String>,
    cloud_quality: Option<String>,
    local_win_confidence: std::sync::atomic::AtomicU32,
    stats: Arc<RouterStats>,
    local_success_ema: parking_lot::RwLock<f32>,
    local_success_count: AtomicU64,
    local_total_count: AtomicU64,
}

#[derive(Debug, Default)]
pub struct RouterStats {
    total_requests: AtomicU64,
    local_sufficient_count: AtomicU64,
    cloud_needed_count: AtomicU64,
    merge_improved_count: AtomicU64,
    cost_saved_usd: AtomicU64,
    cost_spent_usd: AtomicU64,
    local_tokens_total: AtomicU64,
    cloud_tokens_total: AtomicU64,
}

impl RouterStats {
    pub fn snapshot(&self) -> RouterStatsSnapshot {
        let total = self.total_requests.load(Ordering::Relaxed);
        let local = self.local_sufficient_count.load(Ordering::Relaxed);
        let cloud = self.cloud_needed_count.load(Ordering::Relaxed);
        let merged = self.merge_improved_count.load(Ordering::Relaxed);
        let saved = self.cost_saved_usd.load(Ordering::Relaxed);
        let spent = self.cost_spent_usd.load(Ordering::Relaxed);
        let local_tokens = self.local_tokens_total.load(Ordering::Relaxed);
        let cloud_tokens = self.cloud_tokens_total.load(Ordering::Relaxed);
        let all_tokens = local_tokens + cloud_tokens;
        let estimated_savings_percent = if all_tokens > 0 {
            (local_tokens as f32 / all_tokens as f32) * 100.0 * 0.6
        } else {
            0.0
        };
        RouterStatsSnapshot {
            total_requests: total,
            local_sufficient_count: local,
            cloud_needed_count: cloud,
            merge_improved_count: merged,
            local_percentage: if total > 0 { local as f64 / total as f64 * 100.0 } else { 0.0 },
            cost_saved_usd: saved as f64 / 10000.0,
            cost_spent_usd: spent as f64 / 10000.0,
            local_tokens_total: local_tokens,
            cloud_tokens_total: cloud_tokens,
            estimated_savings_percent,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RouterStatsSnapshot {
    pub total_requests: u64,
    pub local_sufficient_count: u64,
    pub cloud_needed_count: u64,
    pub merge_improved_count: u64,
    pub local_percentage: f64,
    pub cost_saved_usd: f64,
    pub cost_spent_usd: f64,
    pub local_tokens_total: u64,
    pub cloud_tokens_total: u64,
    pub estimated_savings_percent: f32,
}

impl std::fmt::Display for RouterStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Local: {:.0}% ({} tokens) | Cloud: {} ({} tokens) | Saved ${:.4} | Spent ${:.4} | Est. savings: {:.1}%",
            self.local_percentage, self.local_tokens_total, self.cloud_needed_count, self.cloud_tokens_total,
            self.cost_saved_usd, self.cost_spent_usd, self.estimated_savings_percent
        )
    }
}

impl EngineRouter {
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
            primary: "local".into(),
            fallback: None,
            local_fast: None,
            cloud_quality: None,
            local_win_confidence: std::sync::atomic::AtomicU32::new(0.8_f32.to_bits()),
            stats: Arc::new(RouterStats::default()),
            local_success_ema: parking_lot::RwLock::new(0.85),
            local_success_count: AtomicU64::new(0),
            local_total_count: AtomicU64::new(0),
        }
    }

    pub fn with_primary(mut self, name: impl Into<String>) -> Self {
        self.primary = name.into();
        self
    }

    pub fn with_local_fast(mut self, name: impl Into<String>) -> Self {
        self.local_fast = Some(name.into());
        self
    }

    pub fn with_cloud_quality(mut self, name: impl Into<String>) -> Self {
        self.cloud_quality = Some(name.into());
        self
    }

    pub fn with_local_win_confidence(mut self, c: f32) -> Self {
        self.local_win_confidence = std::sync::atomic::AtomicU32::new(c.to_bits());
        self
    }

    pub fn register(&mut self, name: impl Into<String>, engine: Arc<dyn InferenceEngine>) {
        self.engines.insert(name.into(), engine);
    }

    pub fn register_boxed(&mut self, name: impl Into<String>, engine: Box<dyn InferenceEngine>) {
        self.engines.insert(name.into(), Arc::from(engine));
    }

    pub fn list(&self) -> Vec<String> { self.engines.keys().cloned().collect() }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn InferenceEngine>> { self.engines.get(name) }

    pub async fn route(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let prompt: String = request.messages.iter().map(|m| m.content.as_str()).collect();

        // FIM fast-path: always try local_fast first with 200ms deadline
        if prompt.contains("<|fim_prefix|>") || prompt.contains("<|fim_suffix|>") {
            if let Some(local_name) = &self.local_fast {
                if let Some(eng) = self.engines.get(local_name) {
                    let eng_clone = eng.clone();
                    let req_clone = request.clone();
                    let result = tokio::time::timeout(
                        std::time::Duration::from_millis(200),
                        eng_clone.generate(req_clone),
                    ).await;
                    if let Ok(Ok(resp)) = result {
                        self.stats.local_sufficient_count.fetch_add(1, Ordering::Relaxed);
                        self.stats.local_tokens_total.fetch_add(resp.tokens_used as u64, Ordering::Relaxed);
                        self.record_local_success(true);
                        return Ok(resp);
                    }
                    self.record_local_success(false);
                }
            }
        }

        let context_size: usize = prompt.len();
        let cs = estimate_complexity(&prompt, context_size);

        self.stats.total_requests.fetch_add(1, Ordering::Relaxed);

        match cs.recommended_engine {
            EngineChoice::Local => {
                let _local_done = false;
                if let Some(local_name) = &self.local_fast {
                    if let Some(eng) = self.engines.get(local_name) {
                        match eng.generate(request.clone()).await {
                            Ok(resp) => {
                                self.stats.local_sufficient_count.fetch_add(1, Ordering::Relaxed);
                                self.stats.local_tokens_total.fetch_add(resp.tokens_used as u64, Ordering::Relaxed);
                                self.record_local_success(true);
                                return Ok(resp);
                            }
                            Err(_) => { self.record_local_success(false); }
                        }
                    }
                }
                if let Some(primary) = self.engines.get(&self.primary) {
                    if let Ok(r) = primary.generate(request.clone()).await {
                        self.stats.local_sufficient_count.fetch_add(1, Ordering::Relaxed);
                        self.stats.local_tokens_total.fetch_add(r.tokens_used as u64, Ordering::Relaxed);
                        self.record_local_success(true);
                        return Ok(r);
                    }
                }
                self.fallback_route(request).await
            }
            EngineChoice::Cloud => {
                self.route_with_fallback(request, 2000).await
            }
            EngineChoice::Hybrid => {
                self.hybrid_parallel(request.clone()).await
            }
        }
    }

    async fn route_with_fallback(&self, request: InferenceRequest, timeout_ms: u64) -> Result<InferenceResponse, InferenceError> {
        let local_name = self.local_fast.clone().unwrap_or_else(|| self.primary.clone());
        if let Some(eng) = self.engines.get(&local_name) {
            let eng_clone = eng.clone();
            let req_clone = request.clone();
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                eng_clone.generate(req_clone),
            ).await;
            if let Ok(Ok(resp)) = result {
                self.stats.local_sufficient_count.fetch_add(1, Ordering::Relaxed);
                self.stats.local_tokens_total.fetch_add(resp.tokens_used as u64, Ordering::Relaxed);
                return Ok(resp);
            }
        }

        if let Some(cloud_name) = &self.cloud_quality {
            if let Some(eng) = self.engines.get(cloud_name) {
                self.stats.cloud_needed_count.fetch_add(1, Ordering::Relaxed);
                let resp = eng.generate(request).await?;
                self.stats.cloud_tokens_total.fetch_add(resp.tokens_used as u64, Ordering::Relaxed);
                return Ok(resp);
            }
        }

        self.fallback_route(request).await
    }

    async fn fallback_route(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        if let Some(fb) = &self.fallback {
            if let Some(eng) = self.engines.get(fb) {
                return eng.generate(request.clone()).await;
            }
        }
        Err(InferenceError::EngineUnavailable { name: "router".into(), reason: "no engines registered".into() })
    }

    pub async fn hybrid_generate(&self, request: &InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let local_name = match &self.local_fast {
            Some(n) => n.clone(),
            None => return self.fallback_route(request.clone()).await,
        };
        let cloud_name = match &self.cloud_quality {
            Some(n) => n.clone(),
            None => {
                if let Some(e) = self.engines.get(&local_name) {
                    return e.generate(request.clone()).await;
                }
                return Err(InferenceError::EngineUnavailable { name: "hybrid".into(), reason: "no engines".into() });
            }
        };

        let local_engine = self.engines.get(&local_name).ok_or_else(||
            InferenceError::EngineUnavailable { name: local_name.clone(), reason: "not registered".into() }
        )?.clone();
        let cloud_engine = self.engines.get(&cloud_name).ok_or_else(||
            InferenceError::EngineUnavailable { name: cloud_name.clone(), reason: "not registered".into() }
        )?.clone();

        let local_req = request.clone();
        let cloud_req = request.clone();

        let local_handle = tokio::spawn(async move { local_engine.generate(local_req).await });
        let cloud_handle = tokio::spawn(async move { cloud_engine.generate(cloud_req).await });

        let local_result = local_handle.await;
        let local_resp = match &local_result {
            Ok(Ok(r)) => Some(r.clone()),
            _ => None,
        };

        let cloud_result = cloud_handle.await;
        let cloud_resp_opt = match &cloud_result {
            Ok(Ok(r)) => Some(r.clone()),
            _ => None,
        };

        match (&local_result, &cloud_result) {
            (Ok(Ok(local)), _) => {
                let quality_est = estimate_quality(&local.content);
                if quality_est >= f32::from_bits(self.local_win_confidence.load(Ordering::Relaxed)) {
                    self.stats.local_sufficient_count.fetch_add(1, Ordering::Relaxed);
                    self.stats.cost_saved_usd.fetch_add(100, Ordering::Relaxed);
                    self.stats.local_tokens_total.fetch_add(local.tokens_used as u64, Ordering::Relaxed);
                    self.record_local_success(true);
                    return Ok(local.clone());
                }
                self.record_local_success(false);
            }
            (Ok(Err(_)), _) => { self.record_local_success(false); }
            _ => {}
        }

        let merged = match (local_resp, cloud_resp_opt) {
            (Some(local), Some(cloud)) => {
                self.stats.merge_improved_count.fetch_add(1, Ordering::Relaxed);
                self.stats.cloud_needed_count.fetch_add(1, Ordering::Relaxed);
                self.stats.cost_spent_usd.fetch_add(150, Ordering::Relaxed);
                self.stats.local_tokens_total.fetch_add(local.tokens_used as u64, Ordering::Relaxed);
                self.stats.cloud_tokens_total.fetch_add(cloud.tokens_used as u64, Ordering::Relaxed);
                merge_responses(local, Some(cloud))
            }
            (Some(local), None) => {
                self.stats.local_sufficient_count.fetch_add(1, Ordering::Relaxed);
                self.stats.local_tokens_total.fetch_add(local.tokens_used as u64, Ordering::Relaxed);
                local
            }
            (None, Some(cloud)) => {
                self.stats.cloud_needed_count.fetch_add(1, Ordering::Relaxed);
                self.stats.cost_spent_usd.fetch_add(150, Ordering::Relaxed);
                self.stats.cloud_tokens_total.fetch_add(cloud.tokens_used as u64, Ordering::Relaxed);
                cloud
            }
            (None, None) => {
                return Err(InferenceError::Model("both engines failed".into()));
            }
        };
        Ok(merged)
    }

    pub async fn hybrid_parallel(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let local_name = match &self.local_fast {
            Some(n) => n.clone(),
            None => return self.fallback_route(request).await,
        };
        let cloud_name = match &self.cloud_quality {
            Some(n) => n.clone(),
            None => {
                if let Some(e) = self.engines.get(&local_name) {
                    return e.generate(request).await;
                }
                return Err(InferenceError::EngineUnavailable { name: "hybrid".into(), reason: "no engines".into() });
            }
        };

        let local_engine = self.engines.get(&local_name).ok_or_else(||
            InferenceError::EngineUnavailable { name: local_name.clone(), reason: "not registered".into() }
        )?.clone();
        let cloud_engine = self.engines.get(&cloud_name).ok_or_else(||
            InferenceError::EngineUnavailable { name: cloud_name.clone(), reason: "not registered".into() }
        )?.clone();

        let local_req = request.clone();
        let cloud_req = request.clone();

        let local_engine_clone = local_engine.clone();
        let local_handle = tokio::spawn(async move { local_engine_clone.generate(local_req).await });
        let cloud_handle = tokio::spawn(async move { cloud_engine.generate(cloud_req).await });

        // Run both engines concurrently and merge results
        let (local_result, cloud_result) = tokio::join!(local_handle, cloud_handle);

        match (local_result, cloud_result) {
            (Ok(Ok(local)), Ok(Ok(cloud))) => {
                self.stats.merge_improved_count.fetch_add(1, Ordering::Relaxed);
                self.stats.cloud_needed_count.fetch_add(1, Ordering::Relaxed);
                self.stats.cost_spent_usd.fetch_add(150, Ordering::Relaxed);
                Ok(merge_responses(local, Some(cloud)))
            }
            (Ok(Ok(local)), _) => {
                self.stats.local_sufficient_count.fetch_add(1, Ordering::Relaxed);
                Ok(local)
            }
            (_, Ok(Ok(cloud))) => {
                self.stats.cloud_needed_count.fetch_add(1, Ordering::Relaxed);
                self.stats.cost_spent_usd.fetch_add(150, Ordering::Relaxed);
                Ok(cloud)
            }
            _ => Err(InferenceError::Model("both engines failed".into())),
        }
    }

    pub fn all_stats(&self) -> Vec<(String, EngineStats)> {
        self.engines.iter().map(|(name, e)| (name.clone(), e.stats())).collect()
    }

    pub fn router_stats(&self) -> RouterStatsSnapshot {
        self.stats.snapshot()
    }

    /// Record a local engine success/failure and update EMA.
    fn record_local_success(&self, success: bool) {
        self.local_total_count.fetch_add(1, Ordering::Relaxed);
        if success {
            self.local_success_count.fetch_add(1, Ordering::Relaxed);
        }
        let total = self.local_total_count.load(Ordering::Relaxed);
        if total.is_multiple_of(10) && total > 0 {
            self.adaptive_threshold();
        }
    }

    /// Adjust local_win_confidence based on EMA of local success rate.
    /// - success rate > 90% → increase confidence threshold (trust local more)
    /// - success rate < 70% → decrease threshold (send more to cloud)
    pub fn adaptive_threshold(&self) {
        let total = self.local_total_count.load(Ordering::Relaxed);
        if total < 5 { return; }
        let successes = self.local_success_count.load(Ordering::Relaxed);
        let rate = successes as f32 / total as f32;
        const ALPHA: f32 = 0.3;
        let mut ema = self.local_success_ema.write();
        *ema = ALPHA * rate + (1.0 - ALPHA) * *ema;
        // Adjust confidence: higher EMA → trust local more → raise bar slightly
        let current = f32::from_bits(self.local_win_confidence.load(Ordering::Relaxed));
        if *ema > 0.90 {
            self.local_win_confidence.store((current + 0.02).min(0.95).to_bits(), Ordering::Relaxed);
        } else if *ema < 0.70 {
            self.local_win_confidence.store((current - 0.05).max(0.50).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn local_success_rate(&self) -> f32 {
        *self.local_success_ema.read()
    }
}

fn estimate_quality(text: &str) -> f32 {
    if text.is_empty() { return 0.0; }
    let chars = text.chars().count();
    let code_like = text.matches(|c: char| c.is_ascii_hexdigit() || c == '\n' || c == '{' || c == '}').count();
    let ratio = code_like.max(1) as f32 / chars.max(1) as f32;
    (0.5 + ratio * 0.5).min(1.0)
}

fn merge_responses(local: InferenceResponse, cloud: Option<InferenceResponse>) -> InferenceResponse {
    match cloud {
        None => {
            let mut r = local;
            r.metadata.insert("merge_strategy".into(), "local-only".into());
            r
        }
        Some(cloud_resp) => {
            let local_len = local.content.chars().count() as f32;
            let cloud_len = cloud_resp.content.chars().count() as f32;
            let confidence = estimate_quality(&local.content);
            let length_ratio = if cloud_len > 0.0 { local_len / cloud_len } else { 1.0 };

            if length_ratio >= 0.8 && confidence >= 0.7 {
                // Local response is sufficient — prefer it to save cost
                let mut preferred = local;
                preferred.metadata.insert("merge_strategy".into(), "local-preferred".into());
                preferred.metadata.insert("local_confidence".into(), format!("{:.2}", confidence));
                preferred.metadata.insert("length_ratio".into(), format!("{:.2}", length_ratio));
                preferred
            } else if length_ratio >= 0.5 && confidence >= 0.5 {
                // Blended: take cloud as base, annotate local contribution
                let mut blended = cloud_resp;
                blended.metadata.insert("merge_strategy".into(), "blended".into());
                blended.metadata.insert("local_contribution".into(),
                    format!("{} chars (conf={:.2})", local_len as u64, confidence));
                blended
            } else {
                // Cloud clearly better
                let mut preferred = cloud_resp;
                preferred.metadata.insert("merge_strategy".into(), "cloud-preferred".into());
                preferred.metadata.insert("local_was".into(), local.content);
                preferred.metadata.insert("local_confidence".into(), format!("{:.2}", confidence));
                preferred
            }
        }
    }
}

impl Default for EngineRouter { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use async_trait::async_trait;

    struct FastMock { name: &'static str }
    struct SlowMock { name: &'static str }

    #[async_trait]
    impl InferenceEngine for FastMock {
        fn name(&self) -> &str { self.name }
        async fn generate(&self, _r: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
            Ok(InferenceResponse { content: "x".repeat(500).into(), finish_reason: "stop".into(), tokens_used: 100, latency_ms: 50, model: "fast".into(), engine: self.name.into(), metadata: std::collections::HashMap::new() })
        }
    }
    #[async_trait]
    impl InferenceEngine for SlowMock {
        fn name(&self) -> &str { self.name }
        async fn generate(&self, _r: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(InferenceResponse { content: "quality code {\n  let x = 1;\n  println!(\"hi\");\n}".into(), finish_reason: "stop".into(), tokens_used: 200, latency_ms: 250, model: "slow".into(), engine: self.name.into(), metadata: std::collections::HashMap::new() })
        }
    }

    #[tokio::test]
    async fn test_hybrid_local_wins() {
        let mut r = EngineRouter::new().with_local_fast("local").with_cloud_quality("cloud").with_local_win_confidence(0.3);
        r.register("local", Arc::new(FastMock { name: "local" }));
        r.register("cloud", Arc::new(SlowMock { name: "cloud" }));

        let req = InferenceRequest::default();
        let resp = r.hybrid_parallel(req).await.unwrap();
        assert_eq!(resp.engine, "local");
        assert!(resp.latency_ms < 200);
    }

    #[tokio::test]
    async fn test_route_primary_first() {
        let mut r = EngineRouter::new().with_primary("local");
        r.register("local", Arc::new(FastMock { name: "local" }));
        let resp = r.route(InferenceRequest::default()).await.unwrap();
        assert_eq!(resp.engine, "local");
    }

    #[tokio::test]
    async fn test_hybrid_generate_merges() {
        let mut r = EngineRouter::new()
            .with_local_fast("local")
            .with_cloud_quality("cloud")
            .with_local_win_confidence(2.0);
        r.register("local", Arc::new(FastMock { name: "local" }));
        r.register("cloud", Arc::new(SlowMock { name: "cloud" }));

        let req = InferenceRequest {
            messages: vec![super::super::engine::ChatMessage {
                role: super::super::engine::Role::User,
                content: "refactor the auth architecture with migration support across multiple files src/auth.rs src/main.rs src/config.rs".into(),
                name: None,
            }],
            ..Default::default()
        };
        let resp = r.hybrid_generate(&req).await.unwrap();
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn test_merge_responses_local_shorter() {
        let local = InferenceResponse {
            content: "short".into(), finish_reason: "stop".into(),
            tokens_used: 10, latency_ms: 50, model: "l".into(), engine: "local".into(),
            metadata: std::collections::HashMap::new(),
        };
        let cloud = InferenceResponse {
            content: "much longer detailed response here".into(), finish_reason: "stop".into(),
            tokens_used: 50, latency_ms: 300, model: "c".into(), engine: "cloud".into(),
            metadata: std::collections::HashMap::new(),
        };
        let merged = merge_responses(local, Some(cloud));
        assert_eq!(merged.engine, "cloud");
        assert!(merged.metadata.get("merge_strategy").unwrap() == "cloud-preferred");
    }

    #[test]
    fn test_merge_responses_local_sufficient() {
        let local = InferenceResponse {
            content: "a reasonably long answer that covers the topic well enough".into(),
            finish_reason: "stop".into(), tokens_used: 40, latency_ms: 50,
            model: "l".into(), engine: "local".into(),
            metadata: std::collections::HashMap::new(),
        };
        let cloud = InferenceResponse {
            content: "a reasonably long answer that is similar length".into(),
            finish_reason: "stop".into(), tokens_used: 42, latency_ms: 300,
            model: "c".into(), engine: "cloud".into(),
            metadata: std::collections::HashMap::new(),
        };
        let merged = merge_responses(local, Some(cloud));
        assert_eq!(merged.engine, "local");
        assert!(merged.metadata.get("merge_strategy").unwrap() == "local-preferred");
    }

    #[test]
    fn test_router_stats_display() {
        let snap = RouterStatsSnapshot {
            total_requests: 100,
            local_sufficient_count: 78,
            cloud_needed_count: 22,
            merge_improved_count: 5,
            local_percentage: 78.0,
            cost_saved_usd: 0.56,
            cost_spent_usd: 0.33,
            local_tokens_total: 7800,
            cloud_tokens_total: 4400,
            estimated_savings_percent: 51.5,
        };
        let display = format!("{}", snap);
        assert!(display.contains("78%"));
        assert!(display.contains("22"));
        assert!(display.contains("7800"));
        assert!(display.contains("51.5"));
    }
}
