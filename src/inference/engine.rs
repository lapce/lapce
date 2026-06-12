//! The InferenceEngine trait — unified API for all backends.

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Role of a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A single chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// A request to an inference engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub stop: Vec<String>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

fn default_max_tokens() -> u32 { 1024 }
fn default_temperature() -> f32 { 0.2 }

/// A response from an inference engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    pub content: String,
    pub finish_reason: String,
    pub tokens_used: u32,
    pub latency_ms: u64,
    pub model: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Errors specific to inference engines.
#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("engine '{name}' not available: {reason}")]
    EngineUnavailable { name: String, reason: String },
    #[error("request timeout after {ms}ms")]
    Timeout { ms: u64 },
    #[error("rate limit exceeded for {engine}")]
    RateLimited { engine: String, retry_after_secs: Option<u64> },
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("model error: {0}")]
    Model(String),
    #[error("cancelled")]
    Cancelled,
}

/// Per-engine running statistics.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EngineStats {
    pub total_requests: u64,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_latency_ms: u64,
    pub errors: u64,
}

/// An inference engine — produces text from messages.
#[async_trait]
pub trait InferenceEngine: Send + Sync {
    fn name(&self) -> &str;

    async fn generate(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError>;

    async fn health_check(&self) -> Result<bool, InferenceError> {
        Ok(true)
    }

    fn stats(&self) -> EngineStats {
        EngineStats::default()
    }

    fn max_context_tokens(&self) -> u32 { 32_768 }

    fn latency_target_ms(&self) -> u64 { 500 }
}

/// A thread-safe counter wrapper for engines that track stats.
#[derive(Default)]
pub struct AtomicEngineStats {
    requests: AtomicU64,
    tokens_in: AtomicU64,
    tokens_out: AtomicU64,
    latency_ms: AtomicU64,
    errors: AtomicU64,
}

impl AtomicEngineStats {
    pub fn record(&self, tokens_in: u32, tokens_out: u32, latency_ms: u64, error: bool) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.tokens_in.fetch_add(tokens_in as u64, Ordering::Relaxed);
        self.tokens_out.fetch_add(tokens_out as u64, Ordering::Relaxed);
        self.latency_ms.fetch_add(latency_ms, Ordering::Relaxed);
        if error {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> EngineStats {
        EngineStats {
            total_requests: self.requests.load(Ordering::Relaxed),
            total_tokens_in: self.tokens_in.load(Ordering::Relaxed),
            total_tokens_out: self.tokens_out.load(Ordering::Relaxed),
            total_latency_ms: self.latency_ms.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

impl std::fmt::Debug for dyn InferenceEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InferenceEngine({})", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockEngine { name: &'static str }
    #[async_trait]
    impl InferenceEngine for MockEngine {
        fn name(&self) -> &str { self.name }
        async fn generate(&self, _r: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
            Ok(InferenceResponse {
                content: "ok".into(),
                finish_reason: "stop".into(),
                tokens_used: 1,
                latency_ms: 10,
                model: "mock".into(),
                engine: self.name.to_string(),
                metadata: std::collections::HashMap::new(),
            })
        }
    }

    #[tokio::test]
    async fn test_engine_trait() {
        let e = MockEngine { name: "mock" };
        let req = InferenceRequest {
            messages: vec![ChatMessage { role: Role::User, content: "hi".into(), name: None }],
            ..Default::default()
        };
        let resp = e.generate(req).await.unwrap();
        assert_eq!(resp.content, "ok");
        assert_eq!(resp.engine, "mock");
    }

    #[test]
    fn test_atomic_stats() {
        let s = AtomicEngineStats::default();
        s.record(100, 50, 25, false);
        s.record(200, 75, 30, true);
        let snap = s.snapshot();
        assert_eq!(snap.total_requests, 2);
        assert_eq!(snap.total_tokens_in, 300);
        assert_eq!(snap.errors, 1);
    }
}

impl Default for InferenceRequest {
    fn default() -> Self {
        Self {
            messages: vec![],
            max_tokens: 1024,
            temperature: 0.2,
            top_p: None,
            stream: false,
            stop: vec![],
            metadata: std::collections::HashMap::new(),
        }
    }
}
