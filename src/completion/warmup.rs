//! Model warmup — pre-prime local models for low-latency first inference.
//!
//! When a local model (Qwen2.5 / DeepSeek-R1) is first loaded, the first
//! inference request is significantly slower because:
//! - CUDA kernels need to be JIT-compiled
//! - KV cache is empty (cold start)
//! - Model weights may not be fully in GPU memory
//!
//! This module sends a small "warmup" prompt to the local model on startup,
//! so the first real user request hits a warm model. Typical improvement:
//! first-token latency drops from 2-3s to 200-500ms.
//!
//! ## Warmup prompts
//!
//! A minimal prompt that exercises the model without producing visible output.
//! The warmup request is sent with `max_tokens=1` so the model processes
//! the prompt but generates almost nothing.

use crate::providers::provider::{AiProvider, ProviderRequest, ChatMessage};
use std::sync::Arc;
use std::time::Instant;

/// Warmup configuration.
#[derive(Debug, Clone)]
pub struct WarmupConfig {
    /// Whether warmup is enabled.
    pub enabled: bool,
    /// Maximum time to wait for warmup to complete.
    pub timeout_secs: u64,
    /// Number of warmup prompts to send (for multi-model setups).
    pub warmup_prompts: usize,
}

impl Default for WarmupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_secs: 10,
            warmup_prompts: 1,
        }
    }
}

/// A pre-built warmup prompt that exercises the model's code generation.
const WARMUP_PROMPT: &str = "fn main() { println!(\"hello\"); }";
const WARMUP_SYSTEM: &str = "You are a code completion assistant. Respond with 'ok'.";

/// Results of the warmup process.
#[derive(Debug, Clone)]
pub struct WarmupResult {
    /// Whether warmup completed successfully.
    pub success: bool,
    /// Total time spent on warmup.
    pub elapsed_ms: u64,
    /// Per-provider warmup results.
    pub providers: Vec<ProviderWarmupResult>,
}

#[derive(Debug, Clone)]
pub struct ProviderWarmupResult {
    pub provider_name: String,
    pub success: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// Run warmup for all local providers in parallel.
///
/// Sends a tiny prompt to each local model to prime the CUDA kernels
/// and KV cache. Returns when all warmups complete or timeout.
pub async fn warmup_local_models(
    providers: &[Arc<dyn AiProvider>],
    config: &WarmupConfig,
) -> WarmupResult {
    if !config.enabled || providers.is_empty() {
        tracing::info!("Model warmup: skipped (disabled or no local providers)");
        return WarmupResult {
            success: true,
            elapsed_ms: 0,
            providers: Vec::new(),
        };
    }

    let start = Instant::now();
    tracing::info!(
        provider_count=providers.len(),
        timeout_secs=config.timeout_secs,
        "Model warmup: starting"
    );

    let warmup_futures: Vec<_> = providers
        .iter()
        .map(|provider| warmup_single_provider(provider.clone(), config))
        .collect();

    let results = futures::future::join_all(warmup_futures).await;

    let elapsed = start.elapsed().as_millis() as u64;
    let all_success = results.iter().all(|r| r.success);

    if all_success {
        tracing::info!(
            elapsed_ms=elapsed,
            provider_count=results.len(),
            "Model warmup: completed successfully"
        );
    } else {
        let failures: Vec<_> = results.iter().filter(|r| !r.success).collect();
        tracing::warn!(
            failure_count=failures.len(),
            "Model warmup: some providers failed to warm up"
        );
    }

    WarmupResult {
        success: all_success,
        elapsed_ms: elapsed,
        providers: results,
    }
}

/// Warm up a single provider with a minimal prompt.
async fn warmup_single_provider(
    provider: Arc<dyn AiProvider>,
    config: &WarmupConfig,
) -> ProviderWarmupResult {
    let provider_name = provider.name().to_string();
    let start = Instant::now();

    let request = ProviderRequest {
        system: Some(WARMUP_SYSTEM.to_string()),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: WARMUP_PROMPT.to_string(),
            tool_calls: None,
            tool_call_id: None,
            ..Default::default()
        }],
        max_tokens: Some(1),
        temperature: Some(0.0),
        stop: None,
        tools: None,
        stream: false,
    };

    tracing::debug!(provider=%provider_name, "Warmup: sending warmup prompt");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(config.timeout_secs),
        provider.chat(&request),
    )
    .await;

    let latency = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(response)) => {
            tracing::info!(
                provider=%provider_name,
                latency_ms=latency,
                "Warmup: provider ready"
            );
            ProviderWarmupResult {
                provider_name: response.provider,
                success: true,
                latency_ms: latency,
                error: None,
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(
                provider=%provider_name,
                error=%e,
                latency_ms=latency,
                "Warmup: provider failed"
            );
            ProviderWarmupResult {
                provider_name,
                success: false,
                latency_ms: latency,
                error: Some(e.to_string()),
            }
        }
        Err(_elapsed) => {
            tracing::warn!(
                provider=%provider_name,
                timeout_secs=config.timeout_secs,
                "Warmup: provider timed out"
            );
            ProviderWarmupResult {
                provider_name,
                success: false,
                latency_ms: latency,
                error: Some(format!("timed out after {}s", config.timeout_secs)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warmup_config_defaults() {
        let config = WarmupConfig::default();
        assert!(config.enabled);
        assert_eq!(config.timeout_secs, 10);
        assert_eq!(config.warmup_prompts, 1);
    }

    #[test]
    fn test_warmup_result_empty_providers() {
        let result = WarmupResult {
            success: true,
            elapsed_ms: 0,
            providers: Vec::new(),
        };
        assert!(result.success);
        assert_eq!(result.elapsed_ms, 0);
    }
}