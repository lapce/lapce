//! OpenAI API direct client.
//!
//! Uses OpenAI's native Chat Completions API.
//! Supports GPT-4o, GPT-4o-mini, o3-mini, etc.

use serde::{Deserialize, Serialize};

/// OpenAI direct client.
pub struct OpenAiClient {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiResponse {
    pub id: String,
    pub object: String,
    pub model: String,
    pub choices: Vec<OpenAiChoice>,
    pub usage: Option<OpenAiUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiChoice {
    pub index: u32,
    pub message: OpenAiMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl OpenAiClient {
    /// Create a new OpenAI client.
    ///
    /// # Arguments
    /// * `api_key` - OpenAI API key (sk-...)
    /// * `model` - Model name, e.g. "gpt-4o"
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
            base_url: "https://api.openai.com/v1".into(),
            model: model.into(),
        }
    }

    /// Create a client with a custom base URL (for Azure, proxies, etc.).
    pub fn with_base_url(api_key: impl Into<String>, model: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            model: model.into(),
        }
    }

    /// Send a chat completion request.
    pub async fn chat(&self, messages: Vec<OpenAiMessage>, max_tokens: Option<u32>, temperature: Option<f64>) -> Result<OpenAiResponse, String> {
        let req = OpenAiRequest {
            model: self.model.clone(),
            messages,
            max_tokens,
            temperature,
            stream: None,
        };

        let resp = self.client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("OpenAI API error: {}", body));
        }

        resp.json::<OpenAiResponse>()
            .await
            .map_err(|e| format!("Parse error: {}", e))
    }

    /// Helper: build a user message.
    pub fn user_message(content: impl Into<String>) -> OpenAiMessage {
        OpenAiMessage { role: "user".into(), content: content.into() }
    }

    /// Helper: build a system message.
    pub fn system_message(content: impl Into<String>) -> OpenAiMessage {
        OpenAiMessage { role: "system".into(), content: content.into() }
    }

    /// Helper: build an assistant message.
    pub fn assistant_message(content: impl Into<String>) -> OpenAiMessage {
        OpenAiMessage { role: "assistant".into(), content: content.into() }
    }

    /// Extract text from the first choice in the response.
    pub fn extract_text(resp: &OpenAiResponse) -> Option<&str> {
        resp.choices.first().map(|c| c.message.content.as_str())
    }
}
