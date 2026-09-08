//! Moonshot Kimi (月之暗面) direct API client.
//!
//! Uses Moonshot's native Chat API (OpenAI-compatible).
//! Supports kimi-2.6, kimi-latest.

use serde::{Deserialize, Serialize};

/// Moonshot Kimi direct client.
pub struct KimiClient {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KimiRequest {
    pub model: String,
    pub messages: Vec<KimiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KimiResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<KimiChoice>,
    pub usage: Option<KimiUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KimiChoice {
    pub index: u32,
    pub message: KimiMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KimiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl KimiClient {
    /// Create a new Kimi client.
    ///
    /// # Arguments
    /// * `api_key` - Moonshot API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Send a chat completion request.
    ///
    /// # Arguments
    /// * `model` - Model: "moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"
    /// * `messages` - Conversation messages
    pub async fn chat(&self, model: &str, messages: Vec<KimiMessage>, max_tokens: Option<u32>, temperature: Option<f64>) -> Result<KimiResponse, String> {
        let req = KimiRequest {
            model: model.to_string(),
            messages,
            max_tokens,
            temperature,
            stream: None,
        };

        let resp = self.client
            .post("https://api.moonshot.cn/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Kimi API error: {}", body));
        }

        resp.json::<KimiResponse>()
            .await
            .map_err(|e| format!("Parse error: {}", e))
    }

    pub fn user_message(content: impl Into<String>) -> KimiMessage {
        KimiMessage { role: "user".into(), content: content.into() }
    }

    pub fn system_message(content: impl Into<String>) -> KimiMessage {
        KimiMessage { role: "system".into(), content: content.into() }
    }

    pub fn assistant_message(content: impl Into<String>) -> KimiMessage {
        KimiMessage { role: "assistant".into(), content: content.into() }
    }

    /// Extract text from the response.
    pub fn extract_text(resp: &KimiResponse) -> Option<&str> {
        resp.choices.first().map(|c| c.message.content.as_str())
    }
}
