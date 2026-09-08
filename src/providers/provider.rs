//! AI Provider trait and individual provider implementations.
//!
//! Supports:
//! - Official APIs: DeepSeek, GLM (Zhipu), Kimi (Moonshot), Minimax
//! - Local: Ollama / llama.cpp (OpenAI-compatible API)
//! - Custom OpenAI-compatible endpoints
//!
//! ## Streaming Support
//!
//! Both chat and FIM completion support SSE (Server-Sent Events) streaming
//! via the `stream_chat()` and `stream_complete()` methods.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ============================================================================
// Response & Request Types
// ============================================================================

/// Result of a provider call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub content: String,
    pub provider: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
    pub latency_ms: u64,
    pub is_local: bool,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// A streaming chunk from a provider.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub content: String,
    pub provider: String,
    pub is_done: bool,
    pub usage: Option<TokenUsage>,
}

/// Request to a provider. Wrapped in Arc to avoid deep-copying
/// the full message history on every clone.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderRequest {
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub stop: Option<Vec<String>>,
    pub tools: Option<Vec<ToolDef>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Per-message metadata (provider, model, tools_used, latency, tokens).
    /// Absorbed from deepseek-tui: the TUI showed provider+tools in every message footer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ============================================================================
// Provider Trait
// ============================================================================

#[async_trait]
pub trait AiProvider: Send + Sync {
    fn name(&self) -> &str;
    fn is_local(&self) -> bool;
    async fn health_check(&self) -> bool;
    async fn chat(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError>;
    fn model(&self) -> &str;
    fn endpoint(&self) -> &str;

    /// Stream chat via SSE, returning a channel receiver for zero-buffer streaming.
    /// Falls back to non-streaming chat wrapped as a single chunk if SSE fails.
    async fn stream_chat(
        &self,
        request: &ProviderRequest,
    ) -> Result<mpsc::Receiver<StreamChunk>, ProviderError>;
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Provider '{provider}' returned HTTP {status}: {body}")]
    HttpError { provider: String, status: u16, body: String },

    #[error("Provider '{provider}' timed out after {timeout_secs}s")]
    Timeout { provider: String, timeout_secs: u64 },

    #[error("Provider '{provider}' is not available: {reason}")]
    Unavailable { provider: String, reason: String },

    #[error("Provider '{provider}' rejected request: {reason}")]
    RateLimited { provider: String, reason: String },

    #[error("Provider '{provider}' returned invalid response: {detail}")]
    InvalidResponse { provider: String, detail: String },

    #[error("Network error for '{provider}': {source}")]
    Network { provider: String, #[source] source: reqwest::Error },

    #[error("{0}")]
    Other(String),
}

// ============================================================================
// OpenAI-Compatible Provider
// ============================================================================

use crate::config::ProviderEntry;

pub struct OpenAiCompatibleProvider {
    config: ProviderEntry,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: ProviderEntry, api_key: Option<String>) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| ProviderError::Other(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { config, api_key, client })
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.config.endpoint.trim_end_matches('/'))
    }

    /// Completion endpoint for FIM (Fill-In-the-Middle) requests.
    #[allow(dead_code)]
    fn completions_url(&self) -> String {
        format!("{}/v1/completions", self.config.endpoint.trim_end_matches('/'))
    }

    fn health_url(&self) -> String {
        if self.config.is_local {
            format!("{}/api/tags", self.config.endpoint.trim_end_matches('/'))
        } else {
            format!("{}/models", self.config.endpoint.trim_end_matches('/'))
        }
    }

    fn build_body(&self, request: &ProviderRequest) -> serde_json::Value {
        let mut messages = Vec::new();

        if let Some(ref system) = request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }

        for msg in &request.messages {
            let mut json_msg = serde_json::json!({"role": msg.role, "content": msg.content});
            if let Some(ref tc) = msg.tool_calls {
                json_msg["tool_calls"] = serde_json::to_value(tc).expect("unwrap failed: provider.rs:203");
            }
            if let Some(ref tci) = msg.tool_call_id {
                json_msg["tool_call_id"] = serde_json::json!(tci);
            }
            messages.push(json_msg);
        }

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "stream": request.stream,
        });

        if let Some(mt) = request.max_tokens { body["max_tokens"] = serde_json::json!(mt); }
        if let Some(t) = request.temperature { body["temperature"] = serde_json::json!(t); }
        if let Some(ref s) = request.stop { body["stop"] = serde_json::json!(s); }
        if let Some(ref tools) = request.tools { body["tools"] = serde_json::to_value(tools).expect("unwrap failed: provider.rs:220"); }

        body
    }

    fn parse_response(&self, body: &serde_json::Value, latency_ms: u64) -> Result<ProviderResponse, ProviderError> {
        let content = body["choices"].as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("").to_string();

        let usage = body.get("usage").map(|u| TokenUsage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        });

        let finish_reason = body["choices"].as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["finish_reason"].as_str())
            .map(|s| s.to_string());

        Ok(ProviderResponse {
            content,
            provider: self.config.name.clone(),
            model: self.config.model.clone(),
            usage,
            latency_ms,
            is_local: self.config.is_local,
            finish_reason,
        })
    }

    fn auth_headers(&self) -> Vec<(&str, String)> {
        let mut headers = vec![("Content-Type", "application/json".into())];
        if let Some(ref key) = self.api_key {
            headers.push(("Authorization", format!("Bearer {}", key)));
        }
        for (k, v) in &self.config.extra_headers {
            headers.push((k.as_str(), v.clone()));
        }
        headers
    }
}

#[async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &str { &self.config.name }
    fn is_local(&self) -> bool { self.config.is_local }
    fn model(&self) -> &str { &self.config.model }
    fn endpoint(&self) -> &str { &self.config.endpoint }

    async fn health_check(&self) -> bool {
        let mut req = self.client.get(self.health_url());
        for (k, v) in self.auth_headers() { req = req.header(k, v); }
        match req.timeout(std::time::Duration::from_secs(5)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn chat(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        self.chat_with_retry(request, 3, 100).await
    }

    /// Stream via SSE with channel-based output — zero intermediate buffer.
    async fn stream_chat(&self, request: &ProviderRequest) -> Result<mpsc::Receiver<StreamChunk>, ProviderError> {
        let body = self.build_body(request);
        let mut req = self.client.post(self.chat_url()).json(&body);
        for (k, v) in self.auth_headers() { req = req.header(k, v); }

        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() { ProviderError::Timeout { provider: self.config.name.clone(), timeout_secs: self.config.timeout_secs } }
            else { ProviderError::Network { provider: self.config.name.clone(), source: e } }
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body: serde_json::Value = resp.json().await.map_err(|e| ProviderError::InvalidResponse { provider: self.config.name.clone(), detail: e.to_string() })?;
            let msg = body["error"]["message"].as_str().unwrap_or("unknown error").to_string();
            return Err(ProviderError::HttpError { provider: self.config.name.clone(), status: status.as_u16(), body: msg });
        }

        let (tx, rx) = mpsc::channel(64);
        let provider_name = self.config.name.clone();
        let nonstream_fallback = request.clone();
        let api_key_fallback = self.api_key.clone();
        let endpoint_fallback = self.config.endpoint.clone();

        tokio::spawn(async move {
            let mut stream = resp.bytes_stream();
            let mut buf = String::new();
            let mut any_chunk = false;

            use futures::StreamExt;
            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(_) => break,
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(line_end) = buf.find('\n') {
                    let line = buf[..line_end].trim().to_string();
                    buf = buf[line_end + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') { continue; }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let _ = tx.send(StreamChunk { content: String::new(), provider: provider_name, is_done: true, usage: None }).await;
                            return;
                        }
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            let content = json["choices"].as_array().and_then(|a| a.first()).and_then(|c| c["delta"]["content"].as_str()).unwrap_or("").to_string();
                            if !content.is_empty() {
                                any_chunk = true;
                                let _ = tx.send(StreamChunk { content, provider: provider_name.clone(), is_done: false, usage: None }).await;
                            }
                            if json["choices"].as_array().and_then(|a| a.first()).and_then(|c| c["finish_reason"].as_str()).is_some() {
                                let _ = tx.send(StreamChunk { content: String::new(), provider: provider_name, is_done: true, usage: None }).await;
                                return;
                            }
                        }
                    }
                }
            }

            // Fallback: if SSE yielded nothing, try non-streaming
            if !any_chunk {
                let client = reqwest::Client::new();
                let url = format!("{}/chat/completions", endpoint_fallback.trim_end_matches('/'));
                let mut r = client.post(&url).json(&nonstream_fallback);
                if let Some(ref key) = api_key_fallback { r = r.header("Authorization", format!("Bearer {}", key)); }
                if let Ok(resp) = r.send().await {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        let content = json["choices"].as_array().and_then(|a| a.first()).and_then(|c| c["message"]["content"].as_str()).unwrap_or("").to_string();
                        let _ = tx.send(StreamChunk { content, provider: provider_name, is_done: true, usage: None }).await;
                        return;
                    }
                }
            }

            let _ = tx.send(StreamChunk { content: String::new(), provider: provider_name, is_done: true, usage: None }).await;
        });

        Ok(rx)
    }
}

// ── Retry wrapper (not part of AiProvider trait) ──

impl OpenAiCompatibleProvider {
    async fn chat_with_retry(
        &self,
        request: &ProviderRequest,
        max_retries: u32,
        initial_delay_ms: u64,
    ) -> Result<ProviderResponse, ProviderError> {
        let mut delay = initial_delay_ms;

        for attempt in 0..=max_retries {
            match self.chat_once(request).await {
                Ok(resp) => return Ok(resp),
                Err(ProviderError::RateLimited { .. }) | Err(ProviderError::Network { .. }) if attempt < max_retries => {
                    tracing::warn!(provider=%self.config.name, attempt=attempt+1, delay_ms=delay, "Retrying after transient error");
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    delay = (delay * 2).min(5000);
                }
                Err(e) => return Err(e),
            }
        }

        Err(ProviderError::Other("All retries exhausted".into()))
    }

    async fn chat_once(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let body = self.build_body(request);
        let start = std::time::Instant::now();
        let mut req = self.client.post(self.chat_url()).json(&body);
        for (k, v) in self.auth_headers() { req = req.header(k, v); }

        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() { ProviderError::Timeout { provider: self.config.name.clone(), timeout_secs: self.config.timeout_secs } }
            else { ProviderError::Network { provider: self.config.name.clone(), source: e } }
        })?;

        let latency = start.elapsed().as_millis() as u64;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.map_err(|e| ProviderError::InvalidResponse { provider: self.config.name.clone(), detail: e.to_string() })?;

        if status.is_success() {
            self.parse_response(&body, latency)
        } else if status.as_u16() == 429 {
            let msg = body["error"]["message"].as_str().unwrap_or("rate limited").to_string();
            Err(ProviderError::RateLimited { provider: self.config.name.clone(), reason: msg })
        } else {
            let msg = body["error"]["message"].as_str().unwrap_or("unknown error").to_string();
            Err(ProviderError::HttpError { provider: self.config.name.clone(), status: status.as_u16(), body: msg })
        }
    }
}
