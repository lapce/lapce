//! Minimax direct API client.
//!
//! Uses Minimax's native Chat API (OpenAI-compatible).
//! Supports M2.7, abab7-chat.

use serde::{Deserialize, Serialize};

/// Minimax direct client.
pub struct MinimaxClient {
    api_key: String,
    group_id: Option<String>,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinimaxMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MinimaxRequest {
    pub model: String,
    pub messages: Vec<MinimaxMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinimaxResponse {
    pub id: Option<String>,
    pub choices: Vec<MinimaxChoice>,
    pub usage: Option<MinimaxUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinimaxChoice {
    pub index: u32,
    pub message: MinimaxMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinimaxUsage {
    pub total_tokens: u32,
}

impl MinimaxClient {
    /// Create a new Minimax client.
    ///
    /// # Arguments
    /// * `api_key` - Minimax API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            group_id: None,
            client: reqwest::Client::new(),
        }
    }

    /// Set the group ID (required by Minimax API).
    pub fn with_group_id(mut self, group_id: impl Into<String>) -> Self {
        self.group_id = Some(group_id.into());
        self
    }

    /// Send a chat completion request.
    ///
    /// # Arguments
    /// * `model` - Model: "abab6.5s-chat", "abab7-chat"
    /// * `messages` - Conversation messages
    pub async fn chat(&self, model: &str, messages: Vec<MinimaxMessage>, max_tokens: Option<u32>, temperature: Option<f64>) -> Result<MinimaxResponse, String> {
        let req = MinimaxRequest {
            model: model.to_string(),
            messages,
            max_tokens,
            temperature,
        };

        let mut builder = self.client
            .post("https://api.minimax.chat/v1/text/chatcompletion_v2")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        if let Some(ref gid) = self.group_id {
            builder = builder.header("group_id", gid);
        }

        let resp = builder.json(&req).send().await.map_err(|e| format!("HTTP error: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Minimax API error: {}", body));
        }

        resp.json::<MinimaxResponse>()
            .await
            .map_err(|e| format!("Parse error: {}", e))
    }

    pub fn user_message(content: impl Into<String>) -> MinimaxMessage {
        MinimaxMessage { role: "user".into(), content: content.into() }
    }

    pub fn system_message(content: impl Into<String>) -> MinimaxMessage {
        MinimaxMessage { role: "system".into(), content: content.into() }
    }

    pub fn assistant_message(content: impl Into<String>) -> MinimaxMessage {
        MinimaxMessage { role: "assistant".into(), content: content.into() }
    }

    /// Extract text from the response.
    pub fn extract_text(resp: &MinimaxResponse) -> Option<&str> {
        resp.choices.first().map(|c| c.message.content.as_str())
    }
}
