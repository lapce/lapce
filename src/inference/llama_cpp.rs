//! llama.cpp / llama-server engine — low-latency local inference.
//!
//! llama-server exposes an OpenAI-compatible HTTP API on localhost.
//! This engine calls it directly, bypassing the cloud provider pipeline
//! for sub-500ms inline completions.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::engine::{
    InferenceEngine, InferenceRequest, InferenceResponse, InferenceError,
    AtomicEngineStats, Role,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlamaCppConfig {
    pub endpoint: String,
    pub model: String,
    pub context_window: u32,
    pub flash_attn: bool,
    pub n_gpu_layers: i32,
    pub timeout_secs: u64,
}

impl Default for LlamaCppConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8080/v1".into(),
            model: "qwen2.5-7b-instruct".into(),
            context_window: 131_072,
            flash_attn: true,
            n_gpu_layers: -1,
            timeout_secs: 120,
        }
    }
}

pub struct LlamaCppEngine {
    config: LlamaCppConfig,
    stats: AtomicEngineStats,
    client: reqwest::Client,
}

impl LlamaCppEngine {
    pub fn new(config: LlamaCppConfig) -> Self {
        let timeout_secs = config.timeout_secs;
        Self {
            config,
            stats: AtomicEngineStats::default(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .expect("llama.cpp client"),
        }
    }

    fn to_openai_payload(&self, req: &InferenceRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req.messages.iter().map(|m| {
            serde_json::json!({
                "role": match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                },
                "content": m.content,
            })
        }).collect();

        let mut payload = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "stream": false,
        });
        if let Some(tp) = req.top_p {
            payload["top_p"] = serde_json::Value::Number(serde_json::Number::from_f64(tp as f64).unwrap_or(serde_json::Number::from(1u32)));
        }
        payload
    }
}

#[async_trait]
impl InferenceEngine for LlamaCppEngine {
    fn name(&self) -> &str { "llama-cpp" }

    async fn generate(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let start = Instant::now();
        let payload = self.to_openai_payload(&request);
        let url = format!("{}/chat/completions", self.config.endpoint.trim_end_matches('/'));

        let total_input_chars: usize = request.messages.iter().map(|m| m.content.len()).sum();

        let resp = self.client.post(&url)
            .json(&payload)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let content = body
                    .get("choices").and_then(|c| c.as_array()).and_then(|a| a.first())
                    .and_then(|c| c.get("message")).and_then(|m| m.get("content"))
                    .and_then(|v| v.as_str()).unwrap_or("").to_string();
                let finish_reason = body
                    .get("choices").and_then(|c| c.as_array()).and_then(|a| a.first())
                    .and_then(|c| c.get("finish_reason")).and_then(|v| v.as_str())
                    .unwrap_or("stop").to_string();
                let tokens_out = content.split_whitespace().count() as u32;
                let latency = start.elapsed().as_millis() as u64;

                self.stats.record(total_input_chars as u32, tokens_out, latency, false);

                Ok(InferenceResponse {
                    content,
                    finish_reason,
                    tokens_used: tokens_out + (total_input_chars as u32 / 4),
                    latency_ms: latency,
                    model: self.config.model.clone(),
                    engine: "llama-cpp".into(),
                    metadata: std::collections::HashMap::new(),
                })
            }
            Ok(r) => {
                let latency = start.elapsed().as_millis() as u64;
                let err = format!("HTTP {}", r.status());
                self.stats.record(0, 0, latency, true);
                Err(InferenceError::Model(err))
            }
            Err(e) => {
                let latency = start.elapsed().as_millis() as u64;
                self.stats.record(0, 0, latency, true);
                Err(InferenceError::EngineUnavailable {
                    name: "llama-cpp".into(),
                    reason: e.to_string(),
                })
            }
        }
    }

    async fn health_check(&self) -> Result<bool, InferenceError> {
        let url = format!("{}/models", self.config.endpoint.trim_end_matches('/'));
        Ok(self.client.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false))
    }

    fn stats(&self) -> super::engine::EngineStats {
        self.stats.snapshot()
    }

    fn max_context_tokens(&self) -> u32 { self.config.context_window }
    fn latency_target_ms(&self) -> u64 { 400 }
}
