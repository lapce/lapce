//! Agent Coordinator — session manager inspired by Crush's Coordinator pattern.
//!
//! The Coordinator owns shared resources (orchestrator, tool registry, hooks)
//! and spawns lightweight `SessionAgent` instances for each conversation.
//! This allows multiple concurrent sessions to share the same provider pool.
//!
//! ## Architecture
//!
//! ```text
//! AgentCoordinator (shared state)
//!   ├── ProviderOrchestrator (shared provider pool)
//!   ├── ToolRegistry (shared tool definitions)
//!   └── HookRegistry (shared event hooks)
//!       │
//!       ├── SessionAgent "cli-session"    → CLI user
//!       ├── SessionAgent "lapce-chat-1"   → Lapce chat panel
//!       └── SessionAgent "lapce-chat-2"   → Lapce second tab
//! ```
//!
//! Inspired by:
//! - Crush's `internal/agent/coordinator.go` (Coordinator + SessionAgent split)
//! - Claude Code's QueryEngine ↔ query() dual-interface pattern

use crate::agent::{Agent, AgentConfig};
use crate::config::{DeepSeekConfig, OrchestrationStrategy};
use crate::providers::orchestrator::{ProviderOrchestrator, ProviderHealth, ProviderStats};
use crate::tools::ToolRegistry;
use crate::hooks::HookRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Unique identifier for a spawned session.
pub type SessionId = String;

/// Metadata about an active session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: SessionId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub message_count: u32,
    pub provider: Option<String>,
}

/// Central coordinator that manages shared AI infrastructure.
///
/// Create one Coordinator per application, then call `spawn_agent()`
/// for each conversation. The coordinator handles:
/// - Provider pool lifecycle (health, stats, failover)
/// - Tool registry (register/unregister at runtime)
/// - Event hooks (subscribe/unsubscribe)
/// - Strategy switching (hot-reload orchestration strategy)
pub struct AgentCoordinator {
    config: DeepSeekConfig,
    orchestrator: Arc<RwLock<ProviderOrchestrator>>,
    tool_registry: Arc<ToolRegistry>,
    hooks: Arc<HookRegistry>,
    /// Track active sessions (lightweight metadata, not the agents themselves).
    sessions: Arc<RwLock<HashMap<SessionId, SessionInfo>>>,
    /// Per-message metadata: maps (session_id, message_index) → {provider, tools_used}
    /// Absorbed from deepseek-tui: the TUI showed provider+tools in message footers.
    message_meta: Arc<RwLock<HashMap<(SessionId, usize), MessageMeta>>>,
}

/// Metadata for a single message turn (absorbed from deepseek-tui footer).
#[derive(Debug, Clone, Default)]
pub struct MessageMeta {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tools_used: Vec<String>,
    pub tokens: u32,
    pub latency_ms: u64,
}

impl AgentCoordinator {
    /// Initialize the coordinator from application config.
    pub fn new(config: DeepSeekConfig) -> anyhow::Result<Self> {
        let orchestrator = ProviderOrchestrator::new(&config)?;
        Ok(Self {
            config,
            orchestrator: Arc::new(RwLock::new(orchestrator)),
            tool_registry: Arc::new(ToolRegistry::with_defaults()),
            hooks: Arc::new(HookRegistry::new()),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            message_meta: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    // ── Session management ──

    /// Spawn a new agent session backed by the shared orchestrator.
    /// Each session has independent conversation history.
    pub async fn spawn_agent(&self, agent_config: AgentConfig, session_id: Option<&str>) -> anyhow::Result<Agent> {
        let orchestrator = self.orchestrator.read().await.clone();
        let sid = session_id.unwrap_or("default").to_string();

        // Register the session
        let sessions = self.sessions.clone();
        let info = SessionInfo {
            id: sid.clone(),
            created_at: chrono::Utc::now(),
            message_count: 0,
            provider: None,
        };
        tokio::spawn(async move {
            sessions.write().await.insert(sid, info);
        });

        // Build the agent
        let agent = Agent::new(&self.config, agent_config, orchestrator.clone())?;
        Ok(agent)
    }

    /// List all active session IDs.
    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }

    /// Get info for a specific session.
    pub async fn session_info(&self, id: &str) -> Option<SessionInfo> {
        self.sessions.read().await.get(id).cloned()
    }

    // ── Provider management ──

    /// Get provider health report.
    pub async fn health_report(&self) -> Vec<ProviderHealth> {
        self.orchestrator.read().await.health_report().await
    }

    /// Get provider performance statistics.
    pub async fn stats_report(&self) -> HashMap<String, ProviderStats> {
        self.orchestrator.read().await.stats_report().await
    }

    /// Reset a failed provider (cooldown reset).
    pub async fn reset_provider(&self, name: &str) {
        self.orchestrator.read().await.reset_provider(name).await;
    }

    /// Hot-switch the orchestration strategy at runtime.
    pub async fn set_strategy(&self, strategy: OrchestrationStrategy) {
        self.orchestrator.write().await.set_strategy(strategy).await;
    }

    /// Get current active strategy.
    pub async fn current_strategy(&self) -> OrchestrationStrategy {
        self.config.orchestration.strategy.clone()
    }

    // ── Tool management ──

    /// Get reference to the shared tool registry.
    pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    /// Get reference to the shared hook registry.
    pub fn hooks(&self) -> &Arc<HookRegistry> {
        &self.hooks
    }

    // ── Config access ──

    pub fn config(&self) -> &DeepSeekConfig {
        &self.config
    }

    /// Get the inference mode.
    pub fn inference_mode(&self) -> crate::config::InferenceMode {
        self.config.inference_mode.clone()
    }

    // ── Message Metadata (absorbed from deepseek-tui message footers) ──

    /// Record per-message metadata after an agent turn completes.
    pub async fn record_message_meta(
        &self,
        session_id: &str,
        msg_index: usize,
        provider: &str,
        tools_used: &[String],
        tokens: u32,
        latency_ms: u64,
    ) {
        let mut meta = self.message_meta.write().await;
        meta.insert(
            (session_id.to_string(), msg_index),
            MessageMeta {
                provider: Some(provider.to_string()),
                model: None,
                tools_used: tools_used.to_vec(),
                tokens,
                latency_ms,
            },
        );
    }

    /// Get metadata for a specific message in a session.
    pub async fn get_message_meta(
        &self,
        session_id: &str,
        msg_index: usize,
    ) -> Option<MessageMeta> {
        let meta = self.message_meta.read().await;
        meta.get(&(session_id.to_string(), msg_index)).cloned()
    }

    /// Get all metadata for a session.
    pub async fn get_session_meta(
        &self,
        session_id: &str,
    ) -> Vec<(usize, MessageMeta)> {
        let meta = self.message_meta.read().await;
        let mut result: Vec<_> = meta
            .iter()
            .filter(|((sid, _idx), _)| sid == session_id)
            .map(|((_, idx), m)| (*idx, m.clone()))
            .collect();
        result.sort_by_key(|(idx, _)| *idx);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_creation() {
        let config = DeepSeekConfig::default();
        let coord = AgentCoordinator::new(config);
        assert!(coord.is_ok());
    }
}
