//! Cloud API engine — DeepSeek/GLM/Kimi/Minimax unified adapter.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::engine::{
    InferenceEngine, InferenceRequest, InferenceResponse, InferenceError,
    AtomicEngineStats, Role,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    pub name: String,
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
    pub latency_target_ms: u64,
}

pub struct CloudEngine {
    config: CloudConfig,
    stats: AtomicEngineStats,
    client: reqwest::Client,
}

impl CloudEngine {
    pub fn new(config: CloudConfig) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(config.timeout_secs))
                .build()
                .expect("cloud engine client"),
            config,
            stats: AtomicEngineStats::default(),
        }
    }
}

#[async_trait]
impl InferenceEngine for CloudEngine {
    fn name(&self) -> &str { &self.config.name }

    async fn generate(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let start = Instant::now();

        let messages: Vec<serde_json::Value> = request.messages.iter().map(|m| {
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
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": false,
        });
        if let Some(tp) = request.top_p {
            payload["top_p"] = serde_json::Value::Number(serde_json::Number::from_f64(tp as f64).unwrap_or(serde_json::Number::from(1u32)));
        }

        let mut req = self.client.post(format!(
            "{}/chat/completions",
            self.config.endpoint.trim_end_matches('/')
        )).json(&payload);

        if let Some(key) = &self.config.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let total_input_chars: usize = request.messages.iter().map(|m| m.content.len()).sum();

        match req.send().await {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let content = body
                    .get("choices").and_then(|c| c.as_array()).and_then(|a| a.first())
                    .and_then(|c| c.get("message")).and_then(|m| m.get("content"))
                    .and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tokens = body.get("usage")
                    .and_then(|u| u.get("completion_tokens"))
                    .and_then(|v| v.as_u64()).unwrap_or(content.len() as u64 / 4) as u32;
                let latency = start.elapsed().as_millis() as u64;
                self.stats.record(total_input_chars as u32, tokens, latency, false);
                Ok(InferenceResponse {
                    content,
                    finish_reason: "stop".into(),
                    tokens_used: tokens,
                    latency_ms: latency,
                    model: self.config.model.clone(),
                    engine: self.config.name.clone(),
                    metadata: std::collections::HashMap::new(),
                })
            }
            Ok(r) => {
                let latency = start.elapsed().as_millis() as u64;
                let status = r.status().as_u16();
                let body = r.text().await.unwrap_or_default();
                self.stats.record(0, 0, latency, true);
                if status == 429 {
                    Err(InferenceError::RateLimited { engine: self.config.name.clone(), retry_after_secs: None })
                } else {
                    Err(InferenceError::Model(format!("HTTP {}: {}", status, body)))
                }
            }
            Err(e) => {
                let latency = start.elapsed().as_millis() as u64;
                self.stats.record(0, 0, latency, true);
                Err(InferenceError::Model(e.to_string()))
            }
        }
    }

    fn stats(&self) -> super::engine::EngineStats { self.stats.snapshot() }
    fn latency_target_ms(&self) -> u64 { self.config.latency_target_ms }
}
