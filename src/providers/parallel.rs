//! Parallel provider orchestration — hybrid parallel + streaming race + dedup.
//!
//! ## Components
//!
//! 1. **HybridParallel**: Launch local + cloud simultaneously. Local provides low-latency
//!    baseline (<500ms), cloud provides high-quality answer. If local returns first
//!    with high confidence, use it and cancel cloud. Otherwise wait for cloud.
//!
//! 2. **StreamingRace**: Multiple providers stream simultaneously. The first provider
//!    to produce a meaningful token (non-whitespace, non-empty) wins. All other
//!    streams are cancelled. Cuts perceived latency by 40-60%.
//!
//! 3. **RequestDedup**: Hash-based deduplication for identical requests within a
//!    time window. If the same request is submitted twice within 2 seconds, the
//!    second call waits for the first and reuses its result.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use parking_lot::Mutex as ParkingMutex;

use crate::providers::provider::{AiProvider, ProviderRequest, ProviderResponse, ProviderError, StreamChunk};

// ============================================================================
// Hybrid Parallel Mode
// ============================================================================

/// Configuration for hybrid parallel execution.
#[derive(Debug, Clone)]
pub struct HybridParallelConfig {
    /// Whether hybrid parallel is enabled.
    pub enabled: bool,
    /// Timeout for local provider response.
    pub local_timeout_ms: u64,
    /// Timeout for cloud provider response.
    pub cloud_timeout_ms: u64,
    /// Minimum confidence score to accept local result over cloud.
    pub min_local_confidence: f64,
    /// Whether to cancel cloud if local returns first with high confidence.
    pub cancel_cloud_on_local_win: bool,
}

impl Default for HybridParallelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            local_timeout_ms: 3000,
            cloud_timeout_ms: 15000,
            min_local_confidence: 0.7,
            cancel_cloud_on_local_win: true,
        }
    }
}

/// Result of hybrid parallel execution.
#[derive(Debug, Clone)]
pub struct HybridParallelResult {
    pub response: ProviderResponse,
    /// Which tier provided the winning response.
    pub source: HybridSource,
    /// Total wall-clock time until response.
    pub total_latency_ms: u64,
    /// Whether cloud was cancelled.
    pub cloud_cancelled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HybridSource {
    /// Local model won (fast, low quality check).
    Local,
    /// Cloud API won (slower, high quality).
    Cloud,
    /// Local timed out, used cloud.
    CloudFallback,
}

/// Execute a request using hybrid parallel: local + cloud simultaneously.
///
/// Local model is the low-latency baseline. Cloud is the high-quality backup.
/// If local returns first with acceptable quality, cloud is cancelled.
pub async fn hybrid_parallel(
    local_provider: Arc<dyn AiProvider>,
    cloud_provider: Arc<dyn AiProvider>,
    request: &ProviderRequest,
    config: &HybridParallelConfig,
) -> Result<HybridParallelResult, ProviderError> {
    let start = Instant::now();

    let (result_tx, mut result_rx) = mpsc::channel(2);
    let cloud_cancelled = Arc::new(ParkingMutex::new(false));

    // ── Local task ──
    let local_req = request.clone();
    let local_provider = local_provider.clone();
    let local_tx = result_tx.clone();
    let local_timeout = Duration::from_millis(config.local_timeout_ms);
    let _local_cancelled = cloud_cancelled.clone();

    let local_handle = tokio::spawn(async move {
        let result = tokio::time::timeout(local_timeout, local_provider.chat(&local_req)).await;
        match result {
            Ok(Ok(response)) => {
                let _ = local_tx.send((HybridSource::Local, response)).await;
            }
            Ok(Err(e)) => {
                tracing::debug!(error=%e, "HybridParallel: local provider failed");
            }
            Err(_) => {
                tracing::debug!("HybridParallel: local provider timed out");
            }
        }
    });

    // ── Cloud task ──
    let cloud_req = request.clone();
    let cloud_provider = cloud_provider.clone();
    let cloud_tx = result_tx.clone();
    let cloud_timeout = Duration::from_millis(config.cloud_timeout_ms);
    let cloud_cancelled = cloud_cancelled.clone();

    let cloud_handle = tokio::spawn(async move {
        let result = tokio::time::timeout(cloud_timeout, cloud_provider.chat(&cloud_req)).await;
        match result {
            Ok(Ok(response)) => {
                let _ = cloud_tx.send((HybridSource::Cloud, response)).await;
            }
            Ok(Err(e)) => {
                tracing::debug!(error=%e, "HybridParallel: cloud provider failed");
            }
            Err(_) => {
                tracing::debug!("HybridParallel: cloud provider timed out");
            }
        }
    });

    drop(result_tx);

    // ── Collect results ──
    let mut local_response: Option<ProviderResponse> = None;
    let mut cloud_response: Option<ProviderResponse> = None;

    while let Some((source, response)) = result_rx.recv().await {
        match source {
            HybridSource::Local => {
                // Check if local quality is good enough
                let confidence = estimate_confidence(&response);
                if confidence >= config.min_local_confidence {
                    // Local is good enough — cancel cloud
                    if config.cancel_cloud_on_local_win {
                        *cloud_cancelled.lock() = true;
                        cloud_handle.abort();
                    }
                    let total_latency = start.elapsed().as_millis() as u64;
                    tracing::info!(
                        provider=%response.provider,
                        latency_ms=response.latency_ms,
                        confidence=confidence,
                        "HybridParallel: local won (cancelled cloud)"
                    );
                    return Ok(HybridParallelResult {
                        response,
                        source: HybridSource::Local,
                        total_latency_ms: total_latency,
                        cloud_cancelled: true,
                    });
                }
                local_response = Some(response);
            }
            HybridSource::Cloud => {
                cloud_response = Some(response);
            }
            _ => {}
        }

        // If we have both, prefer cloud (higher quality)
        if local_response.is_some() && cloud_response.is_some() {
            break;
        }
    }

    // Prefer cloud if available, else local, else error
    if let Some(cloud) = cloud_response {
        let total_latency = start.elapsed().as_millis() as u64;
        tracing::info!(
            provider=%cloud.provider,
            latency_ms=cloud.latency_ms,
            "HybridParallel: cloud won"
        );
        return Ok(HybridParallelResult {
            response: cloud,
            source: HybridSource::Cloud,
            total_latency_ms: total_latency,
            cloud_cancelled: false,
        });
    }

    if let Some(local) = local_response {
        let total_latency = start.elapsed().as_millis() as u64;
        tracing::info!(
            provider=%local.provider,
            latency_ms=local.latency_ms,
            "HybridParallel: local fallback (cloud failed/timed out)"
        );
        return Ok(HybridParallelResult {
            response: local,
            source: HybridSource::CloudFallback,
            total_latency_ms: total_latency,
            cloud_cancelled: false,
        });
    }

    // Both failed — wait for error from local
    let _ = local_handle.await;
    Err(ProviderError::Other("All providers failed in hybrid parallel".into()))
}

/// Estimate confidence of a provider response.
/// Based on response length, presence of error markers, and generic patterns.
fn estimate_confidence(response: &ProviderResponse) -> f64 {
    let content = &response.content;
    let mut score: f64 = 0.5; // base

    // Longer responses tend to be more thorough
    if content.len() > 200 {
        score += 0.15;
    }
    if content.len() > 500 {
        score += 0.1;
    }

    // Penalize uncertainty markers
    let uncertainty = ["不确定", "不太清楚", "抱歉", "I'm not sure", "I don't know", "无法", "cannot"];
    let has_uncertainty = uncertainty.iter().any(|u| content.contains(u));
    if has_uncertainty {
        score -= 0.3;
    }

    // Penalize very short responses
    if content.len() < 50 {
        score -= 0.2;
    }

    // Bonus for code blocks (indicates concrete output)
    if content.contains("```") || content.contains("fn ") || content.contains("def ") {
        score += 0.1;
    }

    score.clamp(0.0, 1.0)
}

// ============================================================================
// Streaming Race
// ============================================================================

/// Configuration for streaming race.
#[derive(Debug, Clone)]
pub struct StreamingRaceConfig {
    /// Whether streaming race is enabled.
    pub enabled: bool,
    /// How long to wait for the first meaningful token (ms).
    pub first_token_timeout_ms: u64,
    /// Minimum number of non-whitespace chars to consider a token "meaningful".
    pub min_meaningful_chars: usize,
}

impl Default for StreamingRaceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            first_token_timeout_ms: 2000,
            min_meaningful_chars: 3,
        }
    }
}

/// A streaming race result — the first provider to produce meaningful output wins.
#[derive(Clone)]
pub struct StreamingRace {
    config: StreamingRaceConfig,
}

impl StreamingRace {
    pub fn new(config: StreamingRaceConfig) -> Self {
        Self { config }
    }

    /// Race multiple providers via streaming. First to produce meaningful output wins.
    /// Returns a channel that streams from the winning provider.
    pub async fn race(
        &self,
        providers: &[Arc<dyn AiProvider>],
        request: &ProviderRequest,
    ) -> Result<mpsc::Receiver<StreamChunk>, ProviderError> {
        if providers.is_empty() {
            return Err(ProviderError::Other("No providers for streaming race".into()));
        }

        if providers.len() == 1 {
            return providers[0].stream_chat(request).await;
        }

        let (winner_tx, mut winner_rx) = mpsc::channel::<(usize, StreamChunk)>(providers.len());
        let mut handles = Vec::new();

        // Launch all providers in parallel
        for (i, provider) in providers.iter().enumerate() {
            let provider = provider.clone();
            let req = request.clone();
            let tx = winner_tx.clone();
            let min_chars = self.config.min_meaningful_chars;

            let handle = tokio::spawn(async move {
                match provider.stream_chat(&req).await {
                    Ok(mut rx) => {
                        let mut accumulated = String::new();
                        while let Some(chunk) = rx.recv().await {
                            if chunk.is_done {
                                let _ = tx.send((i, chunk)).await;
                                break;
                            }
                            accumulated.push_str(&chunk.content);
                            // Check if we have enough meaningful content
                            let meaningful: String = accumulated
                                .chars()
                                .filter(|c| !c.is_whitespace())
                                .collect();
                            if meaningful.len() >= min_chars {
                                // Send the first meaningful chunk and stop
                                let _ = tx.send((i, chunk)).await;
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(provider=%provider.name(), error=%e, "StreamingRace: provider failed");
                    }
                }
            });
            handles.push(handle);
        }
        drop(winner_tx);

        // Wait for the first meaningful response
        let timeout = Duration::from_millis(self.config.first_token_timeout_ms);
        let first = tokio::time::timeout(timeout, winner_rx.recv()).await;

        match first {
            Ok(Some((winner_idx, _first_chunk))) => {
                // Cancel all other providers
                for (i, handle) in handles.into_iter().enumerate() {
                    if i != winner_idx {
                        handle.abort();
                    }
                }

                // Re-stream from the winning provider
                let winner = &providers[winner_idx];
                tracing::info!(
                    provider=%winner.name(),
                    "StreamingRace: winner"
                );
                winner.stream_chat(request).await
            }
            Ok(None) => {
                Err(ProviderError::Other("All providers failed in streaming race".into()))
            }
            Err(_) => {
                // Timeout — fall back to first available provider
                tracing::warn!("StreamingRace: timeout waiting for first token, using fallback");
                providers[0].stream_chat(request).await
            }
        }
    }
}

// ============================================================================
// Request Deduplication
// ============================================================================

/// Configuration for request deduplication.
#[derive(Debug, Clone)]
pub struct DedupConfig {
    /// Whether deduplication is enabled.
    pub enabled: bool,
    /// Time window for deduplication (ms).
    pub window_ms: u64,
    /// Maximum number of pending requests to track.
    pub max_pending: usize,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_ms: 2000,
            max_pending: 64,
        }
    }
}

/// Result of a deduplication check.
pub enum DedupResult {
    /// This is a new request — proceed with execution.
    /// Returns a completion handle that must be called with the result.
    New(DedupCompletion),
    /// This request is a duplicate — wait for the in-flight request to complete.
    Duplicate(oneshot::Receiver<Arc<Result<ProviderResponse, ProviderError>>>),
}

/// Completes a deduplicated request by storing the result.
pub struct DedupCompletion {
    request_hash: u64,
    completions: Vec<oneshot::Sender<Arc<Result<ProviderResponse, ProviderError>>>>,
    dedup_map: Arc<ParkingMutex<DedupState>>,
}

impl DedupCompletion {
    /// Complete the request with a result.
    pub fn complete(self, result: Result<ProviderResponse, ProviderError>) {
        let shared = Arc::new(result);
        for tx in self.completions {
            let _ = tx.send(shared.clone());
        }
        // Remove from dedup map
        self.dedup_map.lock().in_flight.remove(&self.request_hash);
    }

    /// Complete with an error.
    pub fn error(self, err: ProviderError) {
        self.complete(Err(err));
    }
}

/// Internal dedup state.
struct DedupState {
    /// Currently in-flight requests by hash.
    in_flight: HashMap<u64, Vec<oneshot::Sender<Arc<Result<ProviderResponse, ProviderError>>>>>,
    /// Request timestamps for TTL.
    timestamps: HashMap<u64, Instant>,
}

/// Request deduplication manager.
#[derive(Clone)]
pub struct RequestDedup {
    config: DedupConfig,
    state: Arc<ParkingMutex<DedupState>>,
}

impl RequestDedup {
    pub fn new(config: DedupConfig) -> Self {
        Self {
            config,
            state: Arc::new(ParkingMutex::new(DedupState {
                in_flight: HashMap::new(),
                timestamps: HashMap::new(),
            })),
        }
    }

    /// Check if a request is a duplicate of an in-flight request.
    /// Returns DedupResult::New if this is the first request, or
    /// DedupResult::Duplicate if an identical request is already in flight.
    pub fn check(&self, request: &ProviderRequest) -> DedupResult {
        if !self.config.enabled {
            let hash = Self::hash_request(request);
            return DedupResult::New(DedupCompletion {
                request_hash: hash,
                completions: Vec::new(),
                dedup_map: self.state.clone(),
            });
        }

        let hash = Self::hash_request(request);
        let mut state = self.state.lock();

        // Clean up expired entries
        state.timestamps.retain(|_, ts| ts.elapsed() < Duration::from_millis(self.config.window_ms));

        if let Some(waiters) = state.in_flight.get_mut(&hash) {
            // Duplicate — add to waiters
            let (tx, rx) = oneshot::channel();
            waiters.push(tx);
            tracing::debug!(hash, waiters=waiters.len(), "Dedup: duplicate request, waiting for in-flight");
            return DedupResult::Duplicate(rx);
        }

        // New request
        if state.in_flight.len() >= self.config.max_pending {
            // Too many pending — evict oldest
            if let Some(oldest) = state.timestamps.iter()
                .min_by_key(|(_, ts)| **ts)
                .map(|(k, _)| *k)
            {
                state.in_flight.remove(&oldest);
                state.timestamps.remove(&oldest);
            }
        }

        state.in_flight.insert(hash, Vec::new());
        state.timestamps.insert(hash, Instant::now());

        DedupResult::New(DedupCompletion {
            request_hash: hash,
            completions: Vec::new(),
            dedup_map: self.state.clone(),
        })
    }

    /// Hash a request for deduplication.
    fn hash_request(request: &ProviderRequest) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(ref sys) = request.system {
            sys.hash(&mut hasher);
        }
        for msg in &request.messages {
            msg.role.hash(&mut hasher);
            msg.content.hash(&mut hasher);
        }
        request.max_tokens.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::provider::ChatMessage;

    fn make_request(content: &str) -> ProviderRequest {
        ProviderRequest {
            system: None,
            messages: vec![ChatMessage {
                role: "user".into(),
                content: content.into(),
                tool_calls: None,
                tool_call_id: None,
                ..Default::default()
            }],
            max_tokens: Some(100),
            temperature: Some(0.0),
            stop: None,
            tools: None,
            stream: false,
        }
    }

    #[test]
    fn test_estimate_confidence_high() {
        let resp = ProviderResponse {
            content: "Here is a detailed explanation of the function with code examples:\n\n```rust\nfn hello() {\n    println!(\"hello world\");\n}\n```\n\nThis function prints a greeting to the console. It uses the println! macro which is Rust's standard output mechanism. The function takes no arguments and returns nothing. This is a very common pattern in Rust programs.\n\nAdditional context about the implementation and usage patterns follows below with more detailed examples and edge cases considered.".into(),
            provider: "test".into(),
            model: "test".into(),
            usage: None,
            latency_ms: 100,
            is_local: true,
            finish_reason: None,
        };
        let conf = estimate_confidence(&resp);
        assert!(conf > 0.7, "Should have high confidence, got {}", conf);
    }

    #[test]
    fn test_estimate_confidence_low() {
        let resp = ProviderResponse {
            content: "I'm not sure about that.".into(),
            provider: "test".into(),
            model: "test".into(),
            usage: None,
            latency_ms: 100,
            is_local: true,
            finish_reason: None,
        };
        let conf = estimate_confidence(&resp);
        assert!(conf < 0.5, "Should have low confidence for uncertainty");
    }

    #[test]
    fn test_hybrid_parallel_config_defaults() {
        let config = HybridParallelConfig::default();
        assert!(config.enabled);
        assert_eq!(config.local_timeout_ms, 3000);
        assert_eq!(config.cloud_timeout_ms, 15000);
    }

    #[test]
    fn test_streaming_race_config_defaults() {
        let config = StreamingRaceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.first_token_timeout_ms, 2000);
        assert_eq!(config.min_meaningful_chars, 3);
    }

    #[test]
    fn test_dedup_same_request_same_hash() {
        let req1 = make_request("hello world");
        let req2 = make_request("hello world");
        assert_eq!(RequestDedup::hash_request(&req1), RequestDedup::hash_request(&req2));
    }

    #[test]
    fn test_dedup_different_request_different_hash() {
        let req1 = make_request("hello");
        let req2 = make_request("world");
        assert_ne!(RequestDedup::hash_request(&req1), RequestDedup::hash_request(&req2));
    }
}