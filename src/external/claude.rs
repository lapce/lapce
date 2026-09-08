//! Anthropic Claude API direct client.
//!
//! Uses Anthropic's native Messages API (non-OpenAI format).
//! Supports Claude 3.5 Sonnet, Claude 3 Opus, Claude 3 Haiku.

use serde::{Deserialize, Serialize};

/// Anthropic Claude direct client.
pub struct ClaudeClient {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
    model: String,
}

/// Claude-specific message format (different from OpenAI).
#[derive(Debug, Clone, Serialize)]
pub struct ClaudeMessage {
    pub role: String,
    pub content: Vec<ClaudeContent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ClaudeContent {
    #[serde(rename = "text")]
    Text { text: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub resp_type: String,
    pub role: String,
    pub content: Vec<ClaudeResponseContent>,
    pub model: String,
    pub usage: Option<ClaudeUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeResponseContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl ClaudeClient {
    /// Create a new Claude client.
    ///
    /// # Arguments
    /// * `api_key` - Anthropic API key (sk-ant-...)
    /// * `model` - Model name, e.g. "claude-sonnet-4-20250514"
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
            base_url: "https://api.anthropic.com/v1".into(),
            model: model.into(),
        }
    }

    /// Send a chat completion request.
    pub async fn chat(&self, messages: Vec<ClaudeMessage>, system: Option<String>, max_tokens: u32) -> Result<ClaudeResponse, String> {
        let req = ClaudeRequest {
            model: self.model.clone(),
            messages,
            max_tokens,
            system,
            temperature: Some(0.7),
            stream: None,
        };

        let resp = self.client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Claude API error: {}", body));
        }

        resp.json::<ClaudeResponse>()
            .await
            .map_err(|e| format!("Parse error: {}", e))
    }

    /// Helper: build a simple user text message.
    pub fn user_message(text: impl Into<String>) -> ClaudeMessage {
        ClaudeMessage {
            role: "user".into(),
            content: vec![ClaudeContent::Text { text: text.into() }],
        }
    }

    /// Helper: build a simple assistant text message.
    pub fn assistant_message(text: impl Into<String>) -> ClaudeMessage {
        ClaudeMessage {
            role: "assistant".into(),
            content: vec![ClaudeContent::Text { text: text.into() }],
        }
    }

    /// Get the extracted text from a Claude response.
    pub fn extract_text(resp: &ClaudeResponse) -> String {
        resp.content
            .iter()
            .filter_map(|c| {
                if c.content_type == "text" { Some(c.text.as_str()) } else { None }
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
