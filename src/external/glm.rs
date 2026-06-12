//! Zhipu GLM (智谱清言) direct API client.
//!
//! Uses ZhipuAI's native Chat API (OpenAI-compatible).
//! Supports GLM-5.1, GLM-4-Air, GLM-4-Flash.

use serde::{Deserialize, Serialize};

/// Zhipu GLM direct client.
pub struct GlmClient {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlmRequest {
    pub model: String,
    pub messages: Vec<GlmMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<GlmChoice>,
    pub usage: Option<GlmUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmChoice {
    pub index: u32,
    pub message: GlmMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl GlmClient {
    /// Create a new GLM client.
    ///
    /// # Arguments
    /// * `api_key` - ZhipuAI API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Send a chat completion request.
    ///
    /// # Arguments
    /// * `model` - Model name: "glm-4-plus", "glm-4-air", "glm-4-flash"
    /// * `messages` - Conversation messages
    pub async fn chat(&self, model: &str, messages: Vec<GlmMessage>, max_tokens: Option<u32>, temperature: Option<f64>) -> Result<GlmResponse, String> {
        let req = GlmRequest {
            model: model.to_string(),
            messages,
            max_tokens,
            temperature,
            stream: None,
        };

        let resp = self.client
            .post("https://open.bigmodel.cn/api/paas/v4/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("GLM API error: {}", body));
        }

        resp.json::<GlmResponse>()
            .await
            .map_err(|e| format!("Parse error: {}", e))
    }

    pub fn user_message(content: impl Into<String>) -> GlmMessage {
        GlmMessage { role: "user".into(), content: content.into() }
    }

    pub fn system_message(content: impl Into<String>) -> GlmMessage {
        GlmMessage { role: "system".into(), content: content.into() }
    }

    pub fn assistant_message(content: impl Into<String>) -> GlmMessage {
        GlmMessage { role: "assistant".into(), content: content.into() }
    }

    /// Extract text from the response.
    pub fn extract_text(resp: &GlmResponse) -> Option<&str> {
        resp.choices.first().map(|c| c.message.content.as_str())
    }
}
