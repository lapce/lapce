//! GitHub Copilot API direct client.
//!
//! Uses GitHub's Copilot Chat API with OAuth token authentication.
//! Supports GPT-4o backend via Copilot.

use serde::{Deserialize, Serialize};

/// GitHub Copilot direct client.
pub struct CopilotClient {
    token: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotRequest {
    pub messages: Vec<CopilotMessage>,
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotChoice {
    pub index: u32,
    pub message: Option<CopilotMessage>,
    pub delta: Option<CopilotDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotDelta {
    pub role: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotResponse {
    pub id: String,
    pub object: String,
    pub model: Option<String>,
    pub choices: Vec<CopilotChoice>,
    pub usage: Option<CopilotUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl CopilotClient {
    /// Create a new Copilot client.
    ///
    /// # Arguments
    /// * `token` - GitHub OAuth token (with Copilot access)
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Get a Copilot access token from GitHub OAuth.
    async fn get_copilot_token(&self) -> Result<String, String> {
        let resp = self.client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .header("User-Agent", "deepseek-carp")
            .send()
            .await
            .map_err(|e| format!("GitHub token error: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Copilot auth error: {}", body));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
        json["token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No token in Copilot response".to_string())
    }

    /// Send a chat request via Copilot.
    pub async fn chat(&self, messages: Vec<CopilotMessage>) -> Result<String, String> {
        let copilot_token = self.get_copilot_token().await?;

        let req = CopilotRequest {
            messages,
            stream: None,
        };

        let resp = self.client
            .post("https://api.githubcopilot.com/chat/completions")
            .header("Authorization", format!("Bearer {}", copilot_token))
            .header("Content-Type", "application/json")
            .header("Editor-Version", "vscode/1.90.0")
            .header("User-Agent", "deepseek-carp")
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Copilot API error: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Copilot API error: {} - {}", status, body));
        }

        let copilot_resp: CopilotResponse = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
        Ok(copilot_resp.choices
            .first()
            .and_then(|c| c.message.as_ref().map(|m| m.content.clone()))
            .unwrap_or_default())
    }

    /// Helper: build a user message.
    pub fn user_message(content: impl Into<String>) -> CopilotMessage {
        CopilotMessage { role: "user".into(), content: content.into() }
    }

    /// Helper: build a system message.
    pub fn system_message(content: impl Into<String>) -> CopilotMessage {
        CopilotMessage { role: "system".into(), content: content.into() }
    }

    /// Check if the GitHub token has Copilot access.
    pub async fn check_access(&self) -> Result<bool, String> {
        let resp = self.client
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "deepseek-carp")
            .send()
            .await
            .map_err(|e| format!("GitHub API error: {}", e))?;

        Ok(resp.status().is_success())
    }
}
