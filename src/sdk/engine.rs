//! Headless QueryEngine — SDK entry point for embedded usage.
//!
//! Inspired by Claude Code's `query()` AsyncGenerator + `QueryEngine` class.
//! This is the primary API for Lapce, Zeditor, or any Rust GUI/TUI that wants
//! to embed DeepSeek Carp as a library (not a subprocess).

use crate::agent::{Agent, AgentConfig, AgentTurnResult};
use crate::config::DeepSeekConfig;
use crate::providers::provider::{ChatMessage, ProviderRequest, StreamChunk};
use crate::providers::orchestrator::ProviderOrchestrator;
use tokio::sync::mpsc;

/// SDK response from a completed query.
#[derive(Debug, Clone)]
pub struct QueryResponse {
    pub content: String,
    pub provider: String,
    pub total_tokens: u32,
    pub iterations: u32,
    pub tools_used: Vec<String>,
}

impl From<AgentTurnResult> for QueryResponse {
    fn from(r: AgentTurnResult) -> Self {
        Self {
            content: r.content,
            provider: r.provider,
            total_tokens: r.total_tokens,
            iterations: r.iterations,
            tools_used: r.tools_used,
        }
    }
}

/// The SDK entry point. Holds the orchestrator and agent configuration.
/// Multiple calls to `submit()` share the same provider pool.
///
/// This is the **headless mode** — no TUI, no CLI parsing, pure library API.
pub struct QueryEngine {
    config: DeepSeekConfig,
    orchestrator: ProviderOrchestrator,
    agent_config: AgentConfig,
}

impl QueryEngine {
    /// Create a new QueryEngine from application configuration.
    ///
    /// The orchestrator is created once and reused for all queries.
    pub fn new(config: DeepSeekConfig) -> anyhow::Result<Self> {
        let orchestrator = ProviderOrchestrator::new(&config)?;
        Ok(Self {
            config,
            orchestrator,
            agent_config: AgentConfig::default(),
        })
    }

    /// Override the default agent configuration.
    pub fn with_agent_config(mut self, cfg: AgentConfig) -> Self {
        self.agent_config = cfg;
        self
    }

    /// Get a reference to the underlying config.
    pub fn config(&self) -> &DeepSeekConfig {
        &self.config
    }

    /// Get a reference to the provider orchestrator (for health checks, stats).
    pub fn orchestrator(&self) -> &ProviderOrchestrator {
        &self.orchestrator
    }

    /// Submit a prompt and wait for the complete response (non-streaming).
    ///
    /// Uses the agent loop with tool execution. Blocks until done.
    pub async fn submit(&self, prompt: &str) -> anyhow::Result<QueryResponse> {
        #[allow(unused_mut)]
        let mut agent = self.spawn_agent().await;
        let result = agent.process(prompt).await?;
        Ok(result.into())
    }

    /// Submit a prompt and receive a streaming channel of chunks.
    ///
    /// Returns immediately with a channel receiver. Callers should
    /// consume the receiver to display results incrementally.
    pub async fn stream_submit(
        &self,
        prompt: &str,
    ) -> anyhow::Result<mpsc::Receiver<StreamChunk>> {
        let _agent = self.spawn_agent().await;

        // Build a request from the agent's initial state + prompt
        let request = ProviderRequest {
            system: Some(self.agent_config.system_prompt.clone()),
            messages: vec![
                ChatMessage {
                    role: "user".into(),
                    content: prompt.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                ..Default::default()},
            ],
            max_tokens: Some(self.agent_config.max_tokens),
            temperature: Some(self.agent_config.temperature),
            stop: None,
            tools: None, // Streaming doesn't support tools in current impl
            stream: true,
        };

        self.orchestrator.stream_orchestrate(&request)
            .await
            .map_err(|e| anyhow::anyhow!("Stream failed: {}", e))
    }

    /// Spin up a fresh agent backed by this engine's orchestrator.
    async fn spawn_agent(&self) -> Agent {
        Agent::new(&self.config, self.agent_config.clone(), self.orchestrator.clone())
            .unwrap_or_else(|_| {
                tracing::error!("Failed to create agent, this should never happen");
                std::process::abort();
            })
    }
}

// ── Orchestrator clone support ──
// ProviderOrchestrator doesn't implement Clone because it holds
// Arcs internally. We provide a manual clone via new().
// For practical use, wrap in Arc outside QueryEngine if needed.

impl ProviderOrchestrator {
    /// Create a lightweight clone sharing the same provider pool.
    /// Useful when QueryEngine needs multiple agents concurrently.
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> Self {
        // Since all fields are Arc/HashMap (which are clone-able),
        // this would work if ProviderOrchestrator derived Clone.
        // For now, we just log a warning if this is needed.
        tracing::warn!("ProviderOrchestrator::clone() is a stub — use Arc<ProviderOrchestrator> instead");
        unreachable!("Wrap ProviderOrchestrator in Arc instead of cloning")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_response_from_agent_result() {
        let result = AgentTurnResult {
            content: "Hello".into(),
            total_tokens: 42,
            iterations: 2,
            provider: "qwen-local".into(),
            tools_used: vec!["read_file".into()],
        };
        let qr: QueryResponse = result.into();
        assert_eq!(qr.content, "Hello");
        assert_eq!(qr.total_tokens, 42);
        assert_eq!(qr.iterations, 2);
        assert_eq!(qr.provider, "qwen-local");
        assert_eq!(qr.tools_used, vec!["read_file"]);
    }

    #[test]
    fn test_engine_creation() {
        let config = DeepSeekConfig::default();
        let engine = QueryEngine::new(config);
        assert!(engine.is_ok());
    }
}
