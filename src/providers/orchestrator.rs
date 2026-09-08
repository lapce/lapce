//! Multi-provider orchestration engine.
//!
//! ## Architecture
//!
//! ```text
//! User Request
//!     │
//!     ▼
//! InferenceMode::Cloud?
//!     │
//!     ├── YES → SmartUpgrade (default)
//!     │         ├── Step 1: Try local Qwen3.6-27b
//!     │         │     ├── Success + high confidence → return (节省成本)
//!     │         │     └── Low confidence / complexity keywords → Step 2
//!     │         └── Step 2: Smart route to best cloud API
//!     │               ├── TaskBased → code→DeepSeek, chat→GLM, reasoning→Kimi
//!     │               └── Cascade fallback if selected API fails
//!     │
//!     └── NO (Enterprise mode) → Local Qwen → Enterprise cluster
//!                                   (NO external APIs called)
//! ```
//!
//! ## Cross-Provider Context Memory
//!
//! DeepSeek Carp maintains all conversation state internally. Each provider
//! call is **stateless** — the full message history is sent with every request.
//! This means:
//! - Switching providers mid-conversation is safe and transparent
//! - No provider-specific state is leaked between calls
//! - All providers use OpenAI-compatible format (universal)
//!
//! Note: DeepSeek's prefix-cache optimization is handled by the `cache_aware`
//! module (see completions). When switching providers, the cache is reset.

use crate::config::{DeepSeekConfig, InferenceMode, OrchestrationStrategy, UpgradeStrategy};
use crate::providers::provider::{
    AiProvider, OpenAiCompatibleProvider, ProviderError, ProviderRequest, ProviderResponse, StreamChunk,
};
use crate::providers::parallel::{
    HybridParallelConfig, hybrid_parallel,
    StreamingRaceConfig, StreamingRace,
    DedupConfig, RequestDedup, DedupResult,
};
use crate::hooks::HookRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

// ============================================================================
// Health & Performance Tracking
// ============================================================================

#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub name: String,
    pub is_healthy: bool,
    pub consecutive_failures: u32,
    pub last_failure_time: Option<std::time::Instant>,
    pub last_health_check: Option<std::time::Instant>,
    pub disabled_until: Option<std::time::Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderStats {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_latency_ms: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
}

impl ProviderStats {
    pub fn success_rate(&self) -> f64 {
        if self.total_calls == 0 { return 1.0; }
        self.successful_calls as f64 / self.total_calls as f64
    }
    pub fn avg_latency_ms(&self) -> f64 {
        if self.successful_calls == 0 { return 0.0; }
        self.total_latency_ms as f64 / self.successful_calls as f64
    }
}

// ============================================================================
// Task Classification for Smart Routing
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum TaskCategory {
    CodeGeneration,
    CodeReview,
    Chat,
    ComplexReasoning,
    Translation,
    Summarization,
    General,
}

impl TaskCategory {
    /// Classify from user prompt using keyword + length heuristics.
    pub fn classify(prompt: &str) -> Self {
        let lower = prompt.to_lowercase();

        let code_score = [
            "write", "code", "function", "implement", "class", "struct", "trait",
            "mod", "import", "fn ", "def ", "写", "代码", "函数", "实现", "类",
            "重构", "debug", "fix", "bug", "error", "compile", "@file",
        ].iter().filter(|k| lower.contains(*k)).count();

        let reason_score = [
            "analyze", "explain", "why", "how", "reasoning", "think",
            "compare", "evaluate", "trade-off", "分析", "解释", "为什么", "推理",
        ].iter().filter(|k| lower.contains(*k)).count();

        let review_score = [
            "review", "critique", "improve", "suggest", "optimize",
            "审查", "改进", "建议", "优化", "refactor",
        ].iter().filter(|k| lower.contains(*k)).count();

        let trans_score = ["translate", "翻译", "en to zh", "zh to en"]
            .iter().filter(|k| lower.contains(*k)).count();

        let sum_score = ["summarize", "summary", "tl;dr", "总结", "摘要"]
            .iter().filter(|k| lower.contains(*k)).count();

        let scores = [
            (code_score, TaskCategory::CodeGeneration),
            (reason_score, TaskCategory::ComplexReasoning),
            (review_score, TaskCategory::CodeReview),
            (trans_score, TaskCategory::Translation),
            (sum_score, TaskCategory::Summarization),
        ];

        let cat = scores.iter().max_by_key(|(s, _)| *s)
            .map(|(_, c)| c.clone())
            .unwrap_or(TaskCategory::Chat);
        if scores.iter().any(|(s, _)| *s >= 1) { return cat; }

        if prompt.len() > 200 { TaskCategory::ComplexReasoning }
        else { TaskCategory::Chat }
    }

    /// Get preferred provider order for this task.
    /// Only includes domestic providers in auto-upgrade chain.
    /// Overseas providers (Claude/OpenAI/Copilot) are manual opt-in only.
    pub fn preferred_providers(&self) -> Vec<(&str, &str)> {
        // (provider_name, specialty)
        match self {
            TaskCategory::CodeGeneration => vec![
                ("deepseek", "code"), ("kimi", "reasoning"),
            ],
            TaskCategory::CodeReview => vec![
                ("deepseek", "code"), ("kimi", "reasoning"),
            ],
            TaskCategory::ComplexReasoning => vec![
                ("kimi", "reasoning"), ("deepseek", "code"),
            ],
            TaskCategory::Chat => vec![
                ("glm", "chat"), ("deepseek", "code"),
            ],
            TaskCategory::Translation => vec![
                ("glm", "chat"), ("minimax", "general"),
            ],
            TaskCategory::Summarization => vec![
                ("kimi", "reasoning"), ("deepseek", "code"),
            ],
            TaskCategory::General => vec![
                ("deepseek", "code"), ("glm", "chat"),
                ("kimi", "reasoning"), ("minimax", "general"),
            ],
        }
    }
}

// ============================================================================
// Orchestrator
// ============================================================================

#[derive(Clone)]
pub struct ProviderOrchestrator {
    /// All configured providers, indexed by name.
    providers: HashMap<String, Arc<dyn AiProvider>>,
    /// Provider entry configs for lazy initialization.
    #[allow(dead_code)]
    provider_entries: Vec<crate::config::ProviderEntry>,
    /// Health tracking.
    health: Arc<RwLock<HashMap<String, ProviderHealth>>>,
    /// Performance statistics.
    stats: Arc<RwLock<HashMap<String, ProviderStats>>>,
    /// Event hooks for extensibility.
    hooks: Arc<HookRegistry>,
    /// Active inference mode (Cloud / Enterprise).
    inference_mode: InferenceMode,
    /// Active strategy.
    strategy: OrchestrationStrategy,
    /// SmartUpgrade config.
    smart_upgrade: crate::config::SmartUpgradeConfig,
    /// Hybrid parallel config.
    hybrid_parallel: HybridParallelConfig,
    /// Streaming race engine.
    streaming_race: StreamingRace,
    /// Request deduplication.
    request_dedup: RequestDedup,
    /// API provider order.
    api_order: Vec<String>,
    /// Local provider order.
    local_order: Vec<String>,
    /// Max failures before disabling.
    max_failures: u32,
    /// Cooldown period.
    cooldown_secs: u64,
    /// Health check cache: last known health statuses persisted to disk.
    health_cache: Arc<RwLock<HashMap<String, HealthCacheEntry>>>,
}

/// Cached health check result for fast startup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthCacheEntry {
    is_healthy: bool,
    last_check: String,
}

impl ProviderOrchestrator {
    /// Create a new orchestrator with all providers initialized in parallel.
    /// This is the async version that parallelizes provider creation for faster startup.
    pub async fn new_async(config: &DeepSeekConfig) -> anyhow::Result<Self> {
        let credentials = DeepSeekConfig::load_credentials().unwrap_or_default();
        let health_cache = Self::load_health_cache();

        // Collect enabled provider entries
        let entries: Vec<&crate::config::ProviderEntry> = config.providers.iter()
            .filter(|e| e.enabled)
            .filter(|e| config.inference_mode != InferenceMode::Enterprise || e.is_local)
            .collect();

        // ── Parallel provider creation ──
        let mut handles = Vec::new();
        for entry in &entries {
            let entry = (*entry).clone();
            let creds = credentials.clone();
            let handle = tokio::spawn(async move {
                let api_key = entry
                    .api_key_ref.as_ref()
                    .and_then(|r| creds.api_keys.get(r).cloned())
                    .or_else(|| std::env::var(format!(
                        "DEEPCARP_API_KEY_{}",
                        entry.name.to_uppercase().replace('-', "_")
                    )).ok());

                let provider: anyhow::Result<Arc<dyn AiProvider>> = OpenAiCompatibleProvider::new(entry.clone(), api_key)
                    .map(|p| Arc::new(p) as Arc<dyn AiProvider>)
                    .map_err(|e| anyhow::anyhow!("Failed to create provider '{}': {}", entry.name, e));

                (entry.name.clone(), provider)
            });
            handles.push(handle);
        }

        let mut providers: HashMap<String, Arc<dyn AiProvider>> = HashMap::new();
        let mut health_map: HashMap<String, ProviderHealth> = HashMap::new();

        for handle in handles {
            let (name, result) = handle.await?;
            match result {
                Ok(provider) => {
                    // Restore cached health status if available
                    let cached = health_cache.get(&name);
                    health_map.insert(name.clone(), ProviderHealth {
                        name: name.clone(),
                        is_healthy: cached.map(|c| c.is_healthy).unwrap_or(true),
                        consecutive_failures: 0,
                        last_failure_time: None,
                        last_health_check: None,
                        disabled_until: None,
                    });
                    providers.insert(name, provider);
                }
                Err(e) => {
                    tracing::warn!("Skipping provider '{}': {}", name, e);
                }
            }
        }

        Ok(Self {
            providers,
            provider_entries: entries.into_iter().cloned().collect(),
            health: Arc::new(RwLock::new(health_map)),
            stats: Arc::new(RwLock::new(HashMap::new())),
            hooks: Arc::new(HookRegistry::new()),
            inference_mode: config.inference_mode.clone(),
            strategy: config.orchestration.strategy.clone(),
            smart_upgrade: config.orchestration.smart_upgrade.clone(),
            hybrid_parallel: HybridParallelConfig::default(),
            streaming_race: StreamingRace::new(StreamingRaceConfig::default()),
            request_dedup: RequestDedup::new(DedupConfig::default()),
            api_order: config.orchestration.api_order.clone(),
            local_order: config.orchestration.local_order.clone(),
            max_failures: config.orchestration.max_failures_before_disable,
            cooldown_secs: config.orchestration.cooldown_secs,
            health_cache: Arc::new(RwLock::new(health_cache)),
        })
    }

    /// Synchronous constructor (kept for backward compatibility).
    /// Creates providers sequentially — use `new_async` for parallel startup.
    pub fn new(config: &DeepSeekConfig) -> anyhow::Result<Self> {
        let mut providers: HashMap<String, Arc<dyn AiProvider>> = HashMap::new();
        let mut health_map: HashMap<String, ProviderHealth> = HashMap::new();
        let credentials = DeepSeekConfig::load_credentials().unwrap_or_default();
        let health_cache = Self::load_health_cache();

        let entries: Vec<&crate::config::ProviderEntry> = config.providers.iter()
            .filter(|e| e.enabled)
            .filter(|e| config.inference_mode != InferenceMode::Enterprise || e.is_local)
            .collect();

        for entry in &entries {
            let api_key = entry
                .api_key_ref.as_ref()
                .and_then(|r| credentials.api_keys.get(r).cloned())
                .or_else(|| std::env::var(format!(
                    "DEEPCARP_API_KEY_{}",
                    entry.name.to_uppercase().replace('-', "_")
                )).ok());

            let provider: Arc<dyn AiProvider> = Arc::new(
                OpenAiCompatibleProvider::new((*entry).clone(), api_key)
                    .map_err(|e| anyhow::anyhow!("Failed to create provider '{}': {}", entry.name, e))?,
            );

            let cached = health_cache.get(&entry.name);
            health_map.insert(entry.name.clone(), ProviderHealth {
                name: entry.name.clone(),
                is_healthy: cached.map(|c| c.is_healthy).unwrap_or(true),
                consecutive_failures: 0,
                last_failure_time: None,
                last_health_check: None,
                disabled_until: None,
            });

            providers.insert(entry.name.clone(), provider);
        }

        Ok(Self {
            providers,
            provider_entries: entries.into_iter().cloned().collect(),
            health: Arc::new(RwLock::new(health_map)),
            stats: Arc::new(RwLock::new(HashMap::new())),
            hooks: Arc::new(HookRegistry::new()),
            inference_mode: config.inference_mode.clone(),
            strategy: config.orchestration.strategy.clone(),
            smart_upgrade: config.orchestration.smart_upgrade.clone(),
            hybrid_parallel: HybridParallelConfig::default(),
            streaming_race: StreamingRace::new(StreamingRaceConfig::default()),
            request_dedup: RequestDedup::new(DedupConfig::default()),
            api_order: config.orchestration.api_order.clone(),
            local_order: config.orchestration.local_order.clone(),
            max_failures: config.orchestration.max_failures_before_disable,
            cooldown_secs: config.orchestration.cooldown_secs,
            health_cache: Arc::new(RwLock::new(health_cache)),
        })
    }

    /// Load cached health check results from disk.
    fn load_health_cache() -> HashMap<String, HealthCacheEntry> {
        let cache_path = crate::config::paths::sessions_dir().join("_health_cache.json");
        if let Ok(data) = std::fs::read_to_string(&cache_path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            HashMap::new()
        }
    }

    /// Save current health status to disk cache for faster next startup.
    async fn save_health_cache(&self) {
        let health = self.health.read().await;
        let cache: HashMap<String, HealthCacheEntry> = health.iter()
            .map(|(name, h)| (name.clone(), HealthCacheEntry {
                is_healthy: h.is_healthy,
                last_check: chrono::Utc::now().to_rfc3339(),
            }))
            .collect();
        drop(health);

        let cache_path = crate::config::paths::sessions_dir().join("_health_cache.json");
        if let Ok(json) = serde_json::to_string_pretty(&cache) {
            let _ = std::fs::write(&cache_path, json);
        }
    }

    /// Get provider by name.
    pub fn get_provider(&self, name: &str) -> Option<Arc<dyn AiProvider>> {
        self.providers.get(name).cloned()
    }

    /// Get all provider names.
    pub fn provider_names(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    // -- Health --

    async fn is_healthy(&self, name: &str) -> bool {
        let health = self.health.read().await;
        health.get(name).is_some_and(|h| {
            if let Some(until) = h.disabled_until {
                if std::time::Instant::now() < until { return false; }
            }
            h.is_healthy
        })
    }

    async fn record_success(&self, response: &ProviderResponse) {
        let mut stats = self.stats.write().await;
        let s = stats.entry(response.provider.clone()).or_default();
        s.total_calls += 1;
        s.successful_calls += 1;
        s.total_latency_ms += response.latency_ms;
        if let Some(ref u) = response.usage {
            s.total_prompt_tokens += u.prompt_tokens as u64;
            s.total_completion_tokens += u.completion_tokens as u64;
        }
        let mut health = self.health.write().await;
        if let Some(h) = health.get_mut(&response.provider) {
            h.is_healthy = true;
            h.consecutive_failures = 0;
            h.disabled_until = None;
        }
        drop(health);
        drop(stats);

        // Persist health cache on success
        self.save_health_cache().await;

        // Fire provider response hook
        self.hooks.fire(crate::hooks::HookEvent::ProviderResponse {
            provider: response.provider.clone(),
            latency_ms: response.latency_ms,
            tokens: response.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
        }).await;
    }

    async fn record_failure(&self, provider_name: &str) {
        let mut health = self.health.write().await;
        if let Some(h) = health.get_mut(provider_name) {
            h.consecutive_failures += 1;
            if h.consecutive_failures >= self.max_failures {
                h.is_healthy = false;
                h.disabled_until = Some(
                    std::time::Instant::now() + std::time::Duration::from_secs(self.cooldown_secs),
                );
                tracing::warn!(provider=%provider_name, "Disabled after {} failures", h.consecutive_failures);
            }
        }
        drop(health);
        let mut stats = self.stats.write().await;
        stats.entry(provider_name.to_string()).or_default().failed_calls += 1;
        drop(stats);

        // Persist health cache on failure
        self.save_health_cache().await;
    }

    // ========================================================================
    // Strategy: SmartUpgrade ⭐ (default & recommended)
    // ========================================================================

    /// Smart Upgrade: local Qwen first, auto-upgrade to cloud when needed.
    ///
    /// Strategy:
    ///   Tier 1 (qwen-local / Qwen2.5-7B): Always first — fast, low RAM, handles most tasks
    ///   Tier 2 (deepseek-local / DeepSeek-R1-14B): Optional, tried before cloud if enabled
    ///   Cloud API: Last resort for tasks beyond local models
    async fn orchestrate_smart_upgrade(
        &self,
        request: Arc<ProviderRequest>,
    ) -> Result<ProviderResponse, ProviderError> {
        let prompt = request.messages.last().map(|m| m.content.as_str()).unwrap_or("");
        let _category = TaskCategory::classify(prompt);

        // ── Tier 1: Always try qwen-local (Qwen2.5-7B, always resident) ──
        let primary = &self.smart_upgrade.primary_local;
        let mut local_response: Option<ProviderResponse> = None;

        if let Some(provider) = self.providers.get(primary) {
            if self.is_healthy(primary).await {
                tracing::info!(provider=%primary, "SmartUpgrade: trying qwen-local (Qwen2.5-7B)");

                match tokio::time::timeout(
                    std::time::Duration::from_secs(self.smart_upgrade.local_timeout_secs),
                    provider.chat(&request),
                ).await {
                    Ok(Ok(response)) => {
                        let needs_upgrade = self.should_upgrade_to_cloud(prompt, &response);
                        if needs_upgrade {
                            tracing::info!("SmartUpgrade: qwen-local OK but task too complex, saving & upgrading");
                            local_response = Some(response);
                        } else {
                            tracing::info!(provider=%response.provider, latency_ms=response.latency_ms, "SmartUpgrade: qwen-local handled it");
                            self.record_success(&response).await;
                            return Ok(response);
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(error=%e, "SmartUpgrade: qwen-local failed");
                        self.record_failure(primary).await;
                    }
                    Err(_) => {
                        tracing::warn!("SmartUpgrade: qwen-local timed out");
                        self.record_failure(primary).await;
                    }
                }
            }
        }

        // ── Tier 2: Try deepseek-local (DeepSeek-R1-14B) if enabled ──
        if let Some(ds_provider) = self.providers.get("deepseek-local") {
            if self.is_healthy("deepseek-local").await {
                tracing::info!("SmartUpgrade: trying deepseek-local (DeepSeek-R1-14B)");

                match tokio::time::timeout(
                    std::time::Duration::from_secs(self.smart_upgrade.local_timeout_secs * 2),
                    ds_provider.chat(&request),
                ).await {
                    Ok(Ok(response)) => {
                        tracing::info!(provider=%response.provider, latency_ms=response.latency_ms, "SmartUpgrade: deepseek-local handled it");
                        self.record_success(&response).await;
                        return Ok(response);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(provider="deepseek-local", error=%e, "SmartUpgrade: deepseek-local failed");
                        self.record_failure("deepseek-local").await;
                    }
                    Err(_) => {
                        tracing::warn!("SmartUpgrade: deepseek-local timed out");
                        self.record_failure("deepseek-local").await;
                    }
                }
            }
        }

        // ── Fallback: cloud API ──
        tracing::info!("SmartUpgrade: local models exhausted, upgrading to cloud");
        match self.upgrade_to_cloud(&request, prompt).await {
            Ok(cloud_resp) => Ok(cloud_resp),
            Err(cloud_err) => {
                if let Some(local) = local_response {
                    tracing::warn!(error=%cloud_err, "SmartUpgrade: cloud failed, falling back to Tier 1 response");
                    self.record_success(&local).await;
                    Ok(local)
                } else {
                    Err(cloud_err)
                }
            }
        }
    }

    /// Decide whether a local response should trigger a cloud upgrade.
    fn should_upgrade_to_cloud(&self, prompt: &str, response: &ProviderResponse) -> bool {
        // Check for complexity keywords in the user prompt
        let lower = prompt.to_lowercase();
        for kw in &self.smart_upgrade.complexity_keywords {
            if lower.contains(&kw.to_lowercase()) {
                return true;
            }
        }

        // Check response quality signals
        let content = &response.content;
        // Short generic responses often indicate the model didn't fully understand
        if content.len() < 100 && prompt.len() > 200 {
            return true;
        }
        // Model expressed uncertainty
        let uncertainty = ["不确定", "不太清楚", "抱歉", "I'm not sure", "I don't know"];
        if uncertainty.iter().any(|u| content.contains(u)) {
            return true;
        }

        false
    }

    /// Upgrade to cloud: smart route + cascade fallback.
    async fn upgrade_to_cloud(
        &self,
        request: &ProviderRequest,
        prompt: &str,
    ) -> Result<ProviderResponse, ProviderError> {
        let use_task_routing = matches!(
            self.smart_upgrade.upgrade_strategy,
            UpgradeStrategy::TaskBased
        );

        let cloud_order: Vec<String> = if use_task_routing {
            let category = TaskCategory::classify(prompt);
            let preferred = category.preferred_providers();
            let mut order = Vec::new();
            for (name, _specialty) in &preferred {
                if self.providers.contains_key(*name) && self.is_healthy(name).await {
                    order.push(name.to_string());
                }
            }
            // Append remaining healthy cloud providers
            for name in &self.api_order {
                if !order.contains(name) && self.providers.contains_key(name) && self.is_healthy(name).await {
                    order.push(name.clone());
                }
            }
            tracing::info!(category=?category, providers=?order, "SmartUpgrade: task-based routing");
            order
        } else {
            let mut order = Vec::new();
            for n in &self.api_order {
                if self.providers.contains_key(n) && self.is_healthy(n).await {
                    order.push(n.clone());
                }
            }
            order
        };

        self.orchestrate_cascade(request, &cloud_order).await
    }

    // ========================================================================
    // Strategy: Cascade (sequential fallback)
    // ========================================================================

    async fn orchestrate_cascade(
        &self,
        request: &ProviderRequest,
        provider_order: &[String],
    ) -> Result<ProviderResponse, ProviderError> {
        let mut last_error: Option<ProviderError> = None;

        for name in provider_order {
            let provider = match self.providers.get(name) {
                Some(p) => p.clone(),
                None => continue,
            };

            match provider.chat(request).await {
                Ok(response) => {
                    self.record_success(&response).await;
                    return Ok(response);
                }
                Err(e) => {
                    self.record_failure(name).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(ProviderError::Other("All providers failed".into())))
    }

    // ========================================================================
    // Strategy: Parallel Race (fast but expensive)
    // ========================================================================

    async fn orchestrate_parallel_race(
        &self,
        request: Arc<ProviderRequest>,
        provider_order: &[String],
    ) -> Result<ProviderResponse, ProviderError> {
        if provider_order.is_empty() {
            return Err(ProviderError::Other("No providers available".into()));
        }

        let (tx, mut rx) = mpsc::channel(provider_order.len());
        let mut handles = Vec::new();

        for name in provider_order {
            if let Some(provider) = self.providers.get(name).cloned() {
                let req = Arc::clone(&request);
                let tx = tx.clone();
                let handle = tokio::spawn(async move {
                    let _ = tx.send(provider.chat(&req).await).await;
                });
                handles.push(handle);
            }
        }
        drop(tx);

        let mut first: Option<ProviderResponse> = None;
        while let Some(result) = rx.recv().await {
            match result {
                Ok(resp) if first.is_none() => {
                    tracing::info!(provider=%resp.provider, latency_ms=resp.latency_ms, "ParallelRace: first win");
                    self.record_success(&resp).await;
                    first = Some(resp);
                    // Cancel remaining tasks
                    for h in &handles {
                        h.abort();
                    }
                    break;
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error=%e, "ParallelRace: failed"),
            }
        }

        first.ok_or(ProviderError::Other("All providers failed in parallel race".into()))
    }

    // ========================================================================
    // Strategy: Hybrid Parallel (v0.2.0)
    // ========================================================================

    /// Hybrid Parallel: local + cloud simultaneously.
    ///
    /// Local model provides low-latency baseline (<500ms), cloud provides
    /// high-quality answer. If local returns first with high confidence,
    /// cloud is cancelled. This is the best balance of speed and quality.
    async fn orchestrate_hybrid_parallel(
        &self,
        request: Arc<ProviderRequest>,
    ) -> Result<ProviderResponse, ProviderError> {
        // Find local and cloud providers
        let local = self.providers.iter()
            .find(|(_, p)| p.name().contains("local") || p.name().contains("qwen"))
            .map(|(_, p)| p.clone());

        let cloud = self.providers.iter()
            .find(|(_, p)| !p.name().contains("local") && !p.name().contains("qwen"))
            .map(|(_, p)| p.clone());

        match (local, cloud) {
            (Some(local), Some(cloud)) => {
                tracing::info!(
                    local=%local.name(),
                    cloud=%cloud.name(),
                    "HybridParallel: racing local + cloud"
                );
                let result = hybrid_parallel(
                    local,
                    cloud,
                    &request,
                    &self.hybrid_parallel,
                ).await?;
                self.record_success(&result.response).await;
                Ok(result.response)
            }
            (Some(local), None) => {
                tracing::info!("HybridParallel: only local available, using direct");
                let response = local.chat(&request).await?;
                self.record_success(&response).await;
                Ok(response)
            }
            (None, Some(cloud)) => {
                tracing::info!("HybridParallel: only cloud available, using direct");
                let response = cloud.chat(&request).await?;
                self.record_success(&response).await;
                Ok(response)
            }
            (None, None) => {
                Err(ProviderError::Other("No providers available for hybrid parallel".into()))
            }
        }
    }

    // ========================================================================
    // Main orchestrate method
    // ========================================================================

    pub async fn orchestrate(
        &self,
        request: &ProviderRequest,
    ) -> Result<ProviderResponse, ProviderError> {
        let req = Arc::new(request.clone());
        let prompt = req.messages.last().map(|m| m.content.as_str());

        match &self.strategy {
            OrchestrationStrategy::SmartUpgrade => {
                self.orchestrate_smart_upgrade(req).await
            }
            OrchestrationStrategy::ParallelRace => {
                let order = self.get_cloud_order().await;
                self.orchestrate_parallel_race(req, &order).await
            }
            OrchestrationStrategy::HybridParallel => {
                // ── Request dedup check ──
                match self.request_dedup.check(&req) {
                    DedupResult::Duplicate(rx) => {
                        tracing::debug!("Orchestrator: dedup hit, waiting for in-flight request");
                        match rx.await {
                            Ok(shared) => match Arc::try_unwrap(shared) {
                                Ok(result) => result,
                                Err(shared) => {
                                    // Clone the inner values
                                    match shared.as_ref() {
                                        Ok(resp) => Ok(resp.clone()),
                                        Err(e) => Err(ProviderError::Other(e.to_string())),
                                    }
                                }
                            },
                            Err(_) => Err(ProviderError::Other("Dedup channel closed".into())),
                        }
                    }
                    DedupResult::New(completion) => {
                        let result = self.orchestrate_hybrid_parallel(req).await;
                        match &result {
                            Ok(resp) => completion.complete(Ok(resp.clone())),
                            Err(_) => {
                                // Note: ProviderError doesn't impl Clone, completion is dropped
                            }
                        }
                        result
                    }
                }
            }
            OrchestrationStrategy::TaskBasedRouting => {
                let order = if let Some(p) = prompt {
                    let cat = TaskCategory::classify(p);
                    cat.preferred_providers().iter()
                        .filter_map(|(n, _)| {
                            if self.providers.contains_key(*n) { Some(n.to_string()) }
                            else { None }
                        })
                        .collect()
                } else {
                    self.api_order.clone()
                };
                self.orchestrate_cascade(&req, &order).await
            }
            OrchestrationStrategy::AdaptiveWeighted => {
                let order = self.get_adaptive_weighted_order(prompt).await;
                self.orchestrate_cascade(&req, &order).await
            }
            OrchestrationStrategy::CostOptimized => {
                let order = self.get_cost_optimized_order().await;
                self.orchestrate_cascade(&req, &order).await
            }
            OrchestrationStrategy::Cascade => {
                let order = self.get_fallback_order().await;
                self.orchestrate_cascade(&req, &order).await
            }
        }
    }

    async fn get_cloud_order(&self) -> Vec<String> {
        let mut order = Vec::new();
        for n in &self.api_order {
            if self.providers.contains_key(n) && self.is_healthy(n).await {
                order.push(n.clone());
            }
        }
        order
    }

    async fn get_fallback_order(&self) -> Vec<String> {
        let mut order = vec![];
        // Local first
        for name in &self.local_order {
            if self.providers.contains_key(name) && self.is_healthy(name).await {
                order.push(name.clone());
            }
        }
        // Then cloud APIs (only in Cloud mode)
        if self.inference_mode == InferenceMode::Cloud {
            for name in &self.api_order {
                if self.providers.contains_key(name) && self.is_healthy(name).await && !order.contains(name) {
                    order.push(name.clone());
                }
            }
        }
        // Any remaining healthy
        for name in self.providers.keys() {
            if !order.contains(name) && self.is_healthy(name).await {
                order.push(name.clone());
            }
        }
        order
    }

    // ========================================================================
    // Adaptive Weighted: score providers by historical performance
    // ========================================================================

    /// Build provider order weighted by (success_rate × 0.5 + latency_score × 0.3 + task_match × 0.2).
    async fn get_adaptive_weighted_order(&self, prompt: Option<&str>) -> Vec<String> {
        let stats = self.stats.read().await;
        let category = prompt.map(TaskCategory::classify);

        // Build sorted list of healthy providers
        let mut scored: Vec<(String, f64)> = Vec::new();
        let mut healthy = Vec::new();
        for n in self.providers.keys() {
            if self.is_healthy(n).await {
                healthy.push(n.clone());
            }
        }
        for name in &healthy {
                let s = stats.get(name);
                let success_rate = s.map(|s| s.success_rate()).unwrap_or(1.0);
                let avg_latency = s.map(|s| s.avg_latency_ms()).unwrap_or(500.0);

                // Latency score: lower is better, cap at 2000ms
                let latency_score = if avg_latency > 0.0 {
                    (1.0 - (avg_latency / 2000.0).min(1.0)).max(0.0)
                } else { 0.5 };

                // Task match bonus: provider's specialty matches task category
                let task_score = if let Some(ref cat) = category {
                    let prefs = cat.preferred_providers();
                    if prefs.iter().any(|(n, _)| n == name) { 1.0 } else { 0.2 }
                } else { 0.5 };

                // Weighted score: 50% reliability + 30% speed + 20% task match
                let score = success_rate * 0.5 + latency_score * 0.3 + task_score * 0.2;

                scored.push((name.clone(), score));
            }

        // Sort descending by score
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        tracing::debug!(
            providers=?scored.iter().take(4).collect::<Vec<_>>(),
            "AdaptiveWeighted: scoring complete"
        );

        scored.into_iter().map(|(n, _)| n).collect()
    }

    // ========================================================================
    // Cost Optimized: cheapest-first ordering
    // ========================================================================

    /// Build provider order prioritizing cost: free local → cheapest API → most expensive.
    async fn get_cost_optimized_order(&self) -> Vec<String> {
        let mut order = Vec::new();

        // Phase 1: Local providers (free)
        for name in &self.local_order {
            if self.providers.contains_key(name) && self.is_healthy(name).await {
                order.push(name.clone());
            }
        }

        // Phase 2: API providers ordered by estimated cost
        // Cost estimates (USD/Mtok input):
        // deepseek ~$0.14, glm ~$0.50, kimi ~$0.60, minimax ~$0.50
        // Overseas (manual opt-in only): openai ~$2.50, claude ~$3.00, copilot ~$0 (sub)
        let cost_order: Vec<(&str, f64)> = vec![
            ("deepseek", 0.14),
            ("glm", 0.50),
            ("minimax", 0.50),
            ("kimi", 0.60),
        ];

        for (name, _cost) in &cost_order {
            let name_str = name.to_string();
            if self.providers.contains_key(*name) && self.is_healthy(name).await && !order.contains(&name_str) {
                order.push(name_str);
            }
        }

        // Any remaining healthy providers
        for name in self.providers.keys() {
            if !order.contains(name) && self.is_healthy(name).await {
                order.push(name.clone());
            }
        }

        tracing::debug!(order=?order, "CostOptimized: provider order");
        order
    }

    // ========================================================================
    // Streaming orchestrate — yields StreamChunk via channel
    // ========================================================================

    /// Orchestrate a streaming request, yielding chunks via channel.
    /// Directly pipes provider SSE output to consumer — no intermediate buffering.
    pub async fn stream_orchestrate(
        &self,
        request: &ProviderRequest,
    ) -> Result<mpsc::Receiver<StreamChunk>, ProviderError> {
        let req = Arc::new(request.clone());

        let order = match self.strategy {
            OrchestrationStrategy::SmartUpgrade => {
                let mut o = vec![self.smart_upgrade.primary_local.clone()];
                o.extend(self.api_order.iter().cloned());
                o
            }
            _ => self.get_fallback_order().await,
        };

        let provider = match order.iter().find(|n| self.providers.contains_key(*n)).and_then(|n| self.providers.get(n)) {
            Some(p) => p.clone(),
            None => return Err(ProviderError::Other("No providers available".into())),
        };

        // stream_chat now returns a Receiver directly — zero intermediate buffer
        provider.stream_chat(&req).await
    }

    // -- Public API --

    pub async fn health_report(&self) -> Vec<ProviderHealth> {
        self.health.read().await.values().cloned().collect()
    }

    pub async fn stats_report(&self) -> HashMap<String, ProviderStats> {
        self.stats.read().await.clone()
    }

    pub async fn reset_provider(&self, name: &str) {
        if let Some(h) = self.health.write().await.get_mut(name) {
            h.is_healthy = true;
            h.consecutive_failures = 0;
            h.disabled_until = None;
        }
    }

    pub async fn set_strategy(&mut self, strategy: OrchestrationStrategy) {
        self.strategy = strategy;
        tracing::info!(strategy=?self.strategy, "Strategy changed");
    }

    pub fn mode(&self) -> &InferenceMode {
        &self.inference_mode
    }

    /// Get the streaming race engine for advanced parallel request handling.
    pub fn streaming_race(&self) -> &StreamingRace {
        &self.streaming_race
    }

    /// Get the health cache for persisted health check results.
    pub fn health_cache(&self) -> &Arc<RwLock<HashMap<String, HealthCacheEntry>>> {
        &self.health_cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_classification_code() {
        let cat = TaskCategory::classify("write a function to calculate fibonacci in Rust");
        assert_eq!(cat, TaskCategory::CodeGeneration);
    }

    #[test]
    fn test_task_classification_reasoning() {
        let cat = TaskCategory::classify("analyze why this algorithm has O(n log n) complexity");
        assert_eq!(cat, TaskCategory::ComplexReasoning);
    }

    #[test]
    fn test_task_classification_chat() {
        let cat = TaskCategory::classify("你好，今天天气怎么样？");
        assert_eq!(cat, TaskCategory::Chat);
    }

    #[test]
    fn test_upgrade_triggers_for_complexity() {
        let orchestrator = crate::config::SmartUpgradeConfig::default();
        // These keywords should all trigger upgrade
        assert!(orchestrator.complexity_keywords.contains(&"重构".to_string()));
        assert!(orchestrator.complexity_keywords.contains(&"architecture".to_string()));
        assert!(orchestrator.complexity_keywords.contains(&"分布式".to_string()));
    }
}
