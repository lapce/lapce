//! Conversation agent loop with trait-based tool execution.
//!
//! The Agent processes user messages through a multi-turn loop:
//! 1. Send conversation to provider orchestrator
//! 2. Parse tool_calls from response → execute via ToolExecutor trait → add results
//! 3. Return final text when no more tool calls
//!
//! ## Architecture
//!
//! For multi-session usage, use `AgentCoordinator` (inspired by Crush):
//! ```no_run
//! use deepseek_carp::agent::{AgentCoordinator, AgentConfig};
//! use deepseek_carp::config::DeepSeekConfig;
//!
//! # async {
//! let config = DeepSeekConfig::load().unwrap_or_default();
//! let coord = AgentCoordinator::new(config).expect("unwrap failed: mod.rs:17");
//! let mut agent1 = coord.spawn_agent(AgentConfig::default(), Some("chat-1")).expect("unwrap failed: mod.rs:18");
//! let mut agent2 = coord.spawn_agent(AgentConfig::default(), Some("chat-2")).expect("unwrap failed: mod.rs:19");
//! // Both agents share the same provider pool
//! # };
//! ```

pub mod coordinator;
pub mod permission;
pub mod compact;
pub mod cost;
pub mod plan;
pub mod compile_fix;
pub mod sub_agents;
pub mod swarm;
pub mod reasoning;
pub mod seam;
pub mod constitution;
pub mod task_queue;
pub mod context_optimizer;
pub mod local_prompt_optimizer;
pub mod fine_tuning;
pub mod planning_agent;
pub mod self_reflection;
pub mod tool_orchestrator;
pub mod project_finetune;
pub mod nlu_engine;
pub mod planning;
pub mod scheduler;
pub mod orchestrator;
pub mod code_agent;
pub mod bbon;

pub use coordinator::AgentCoordinator;
pub use coordinator::SessionId;
pub use coordinator::SessionInfo;
pub use coordinator::MessageMeta;
pub use permission::{Permission, PermissionEvaluator, PermissionMode};
pub use compact::{ContextThresholds, ContextLevel, MicroCompactor};
pub use cost::{CostTracker, CostSummary, ModelPricing};
pub use plan::{
    Plan, PlanManager, PlanStatus, plan_mode_prompt, execute_mode_prompt,
    ExecuteLoop, ExecuteLoopConfig, ExecuteLoopResult, RoundRecord, RoundStatus,
};
pub use compile_fix::{CompileEngine, CompileResult, CompileError, PlanExecutionPipeline};
pub use sub_agents::{SubAgentPool, SubAgentTask, SubAgentResult, TaskStatus};
pub use swarm::{SwarmCoordinator, SwarmAgent, SwarmMessage, AgentState, DecomposedTask, SwarmResult, SwarmStatus};
pub use reasoning::{ReasoningToken, ReasoningParser, reasoning_stream, ReasoningRole};
pub use constitution::{constitution_prompt, constitution_short};
pub use seam::SeamManager;
pub use task_queue::{TaskQueue, TaskRecord, TaskExecutor, TaskQueueStatus, spawn_workers};
pub use context_optimizer::{
    SmartSummarizer, SmartSummarizerConfig, SmartSummary,
    AttachmentTrimmer, AttachmentTrimConfig, TrimmedAttachment,
    ContextScorer, ContextScorerConfig, ScoredContext,
};
pub use local_prompt_optimizer::{
    LocalPromptOptimizer, PromptOptimizerConfig, local_model_system_prompt,
};
pub use fine_tuning::{
    FineTuningCollector, TrainingExample, ExampleType, ExampleMetadata, QualityFilter,
    CollectorStats, ProjectFineTuner,
};
pub use scheduler::{TaskScheduler, ScheduledTask, ScheduleKind, ScheduleStatus, TaskExecutionResult};
pub use bbon::{BbonConfig, BehaviorNarrative, TrajectoryStep, BbonOrchestrator, BbonResult, FactExtractor, BehaviorJudge};

use crate::config::DeepSeekConfig;
use crate::providers::provider::{ChatMessage, ProviderRequest, ToolCall, FunctionCall};
use crate::providers::orchestrator::ProviderOrchestrator;
use crate::tools::{ToolRegistry, ToolExecutor, ToolResult};
use crate::hooks::HookRegistry;
use crate::context::{resolve_references, RagContext};
use std::sync::Arc;

/// Result of an agent turn.
#[derive(Debug)]
pub struct AgentTurnResult {
    pub content: String,
    pub total_tokens: u32,
    pub iterations: u32,
    pub provider: String,
    pub tools_used: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub system_prompt: String,
    pub max_iterations: u32,
    pub temperature: f64,
    pub max_tokens: u32,
    /// Token budget ceiling — agent aborts if cumulative usage exceeds this.
    /// Default: 128k (safe for most models); set lower for context-limited providers.
    pub token_budget: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: "You are DeepSeek Carp, an expert AI coding assistant. \
                Help users write, debug, and understand code. \
                Use tools when appropriate. Always be concise and accurate."
                .to_string(),
            max_iterations: 50,
            temperature: 0.7,
            max_tokens: 8192,
            token_budget: 128_000,  // 128k — safe default for most cloud models
        }
    }
}

/// The conversation agent with trait-based tool execution.
pub struct Agent {
    config: AgentConfig,
    orchestrator: ProviderOrchestrator,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_registry: Arc<ToolRegistry>,
    /// Event hooks for extensibility (metrics, logging, plugins).
    hooks: Arc<HookRegistry>,
    conversation_history: Vec<ChatMessage>,
    shutdown: Option<tokio::sync::watch::Receiver<bool>>,
    /// Permission evaluator for tool safety (Claude Code pattern).
    permission_evaluator: PermissionEvaluator,
    /// Micro-compactor for token management (Claude Code pattern).
    micro_compactor: MicroCompactor,
    /// Context thresholds for auto-compaction.
    thresholds: ContextThresholds,
    /// Cost tracker for API usage (Claude Code pattern).
    cost_tracker: CostTracker,
}

impl Agent {
    pub fn new(
        _app_config: &DeepSeekConfig,
        agent_config: AgentConfig,
        orchestrator: ProviderOrchestrator,
    ) -> anyhow::Result<Self> {
        let system_prompt = agent_config.system_prompt.clone();
        let tool_registry = Arc::new(ToolRegistry::with_defaults());
        let tool_executor = Arc::new(tool_registry.executor());
        let hooks = Arc::new(HookRegistry::new());

        // Inject Constitution into system prompt
        let enriched_system = format!("{}\n\n{}", system_prompt, constitution_short());

        Ok(Self {
            config: agent_config.clone(),
            orchestrator,
            tool_executor,
            tool_registry,
            hooks,
            conversation_history: vec![ChatMessage {
                role: "system".into(),
                content: enriched_system,
                tool_calls: None,
                tool_call_id: None,
            ..Default::default()}],
            shutdown: None,
            permission_evaluator: PermissionEvaluator::default(),
            micro_compactor: MicroCompactor::new(),
            thresholds: ContextThresholds::for_window(agent_config.token_budget as usize),
            cost_tracker: CostTracker::new(),
        })
    }

    /// Attach a shutdown signal for graceful termination.
    pub fn with_shutdown(mut self, rx: tokio::sync::watch::Receiver<bool>) -> Self {
        self.shutdown = Some(rx);
        self
    }

    /// Get a reference to the hook registry for external subscribers.
    pub fn hooks(&self) -> &Arc<HookRegistry> {
        &self.hooks
    }

    /// Check if shutdown has been requested.
    fn is_shutting_down(&self) -> bool {
        self.shutdown.as_ref().map(|rx| *rx.borrow()).unwrap_or(false)
    }

    /// Process a user message through the tool-calling loop.
    pub async fn process(&mut self, user_message: &str) -> anyhow::Result<AgentTurnResult> {
        // ── Resolve @-file references & RAG context (Cursor/Claude Code pattern) ──
        let (mut enriched_prompt, _file_refs) = resolve_references(user_message, None);

        // Optionally enrich with codebase RAG context
        if let Ok(wd) = std::env::current_dir() {
            let mut rag = RagContext::new(wd);
            let chunk_count = rag.index();
            if chunk_count > 0 {
                let rag_ctx = rag.enrich(user_message);
                if !rag_ctx.is_empty() && rag_ctx != user_message {
                    enriched_prompt = rag_ctx;
                }
            }
        }

        self.conversation_history.push(ChatMessage {
            role: "user".into(),
            content: enriched_prompt,
            tool_calls: None,
            tool_call_id: None,
        ..Default::default()});

        let mut total_tokens: u32 = 0;
        let mut iterations: u32 = 0;
        let mut final_provider;
        let mut tools_used: Vec<String> = Vec::new();

        loop {
            if self.is_shutting_down() {
                return Err(anyhow::anyhow!("Agent shutdown requested"));
            }
            iterations += 1;
            if iterations > self.config.max_iterations {
                return Err(anyhow::anyhow!("Agent exceeded max iterations ({})", self.config.max_iterations));
            }

            // Token budget check — abort early if we're about to overflow context
            if total_tokens > self.config.token_budget {
                return Err(anyhow::anyhow!(
                    "Token budget exceeded ({} > {}). Consider upgrading context or reducing history.",
                    total_tokens,
                    self.config.token_budget
                ));
            }

            // ── Micro-compact: auto-stub old tool outputs to save tokens ──
            let _msg_count_before = self.conversation_history.len();
            let est_tokens = MicroCompactor::estimate_tokens(&self.conversation_history);
            let level = self.thresholds.check(est_tokens);
            if level != ContextLevel::Normal {
                let (compacted, stubbed) = self.micro_compactor.compact(
                    &self.conversation_history,
                    est_tokens,
                    &self.thresholds,
                );
                if stubbed > 0 {
                    self.conversation_history = compacted;
                    tracing::info!(level=?level, est_tokens, stubbed, "Micro-compaction applied");
                }
                if level == ContextLevel::Error {
                    tracing::error!("Context window critically full — consider starting a new session");
                }
            }

            let request = ProviderRequest {
                system: None,
                messages: self.conversation_history.clone(),
                max_tokens: Some(self.config.max_tokens),
                temperature: Some(self.config.temperature),
                stop: None,
                tools: Some(self.tool_registry.to_openai_format()),
                stream: false,
            };

            let response = self.orchestrator.orchestrate(&request).await?;

            if let Some(ref usage) = response.usage {
                total_tokens += usage.total_tokens;
                // Track API cost (Claude Code pattern)
                self.cost_tracker.record(
                    &response.provider,
                    &response.model,
                    usage.prompt_tokens,
                    usage.completion_tokens,
                );
            }
            final_provider = response.provider.clone();

            let tool_calls = self.extract_tool_calls(&response.content);

            if tool_calls.is_empty() {
                let mut meta = std::collections::HashMap::new();
                meta.insert("provider".to_string(), response.provider.clone());
                meta.insert("model".to_string(), response.model.clone());
                meta.insert("tokens".to_string(), total_tokens.to_string());
                meta.insert("iterations".to_string(), iterations.to_string());
                if !tools_used.is_empty() {
                    meta.insert("tools".to_string(), tools_used.join(", "));
                }
                self.conversation_history.push(ChatMessage {
                    role: "assistant".into(),
                    content: response.content.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                    metadata: Some(meta),
                });

                // Fire agent turn completed hook
                self.hooks.fire(crate::hooks::HookEvent::AgentTurnCompleted {
                    provider: final_provider.clone(),
                    total_tokens,
                    tools_used: tools_used.clone(),
                }).await;

                return Ok(AgentTurnResult {
                    content: response.content,
                    total_tokens,
                    iterations,
                    provider: final_provider,
                    tools_used,
                });
            }

            tracing::info!(
                tool_count = tool_calls.len(),
                tools = ?tool_calls.iter().map(|t| &t.function.name).collect::<Vec<_>>(),
                "Agent: executing tool calls (iteration {})",
                iterations
            );

            let mut tool_msg_meta = std::collections::HashMap::new();
            tool_msg_meta.insert("provider".to_string(), response.provider.clone());
            tool_msg_meta.insert("has_tool_calls".to_string(), "true".to_string());
            self.conversation_history.push(ChatMessage {
                role: "assistant".into(),
                content: response.content.clone(),
                tool_calls: Some(tool_calls.clone()),
                tool_call_id: None,
                metadata: Some(tool_msg_meta),
            });

            // Execute via trait — clean abstraction (permission-gated)
            for tc in &tool_calls {
                tools_used.push(tc.function.name.clone());
                let is_destructive = matches!(
                    tc.function.name.as_str(),
                    "execute_shell" | "delete_file" | "write_file"
                );
                let perm = self.permission_evaluator.evaluate(&tc.function.name, is_destructive);
                if matches!(perm, Permission::Deny) {
                    tracing::warn!(tool=%tc.function.name, "Tool denied by permission system");
                    let result = ToolResult::Error(format!("Permission denied: {}", tc.function.name));
                    self.conversation_history.push(ChatMessage {
                        role: "tool".into(),
                        content: result.to_json(),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                    ..Default::default()});
                    continue;
                }

                let result = self.tool_executor.execute(&tc.function.name, &tc.function.arguments).await;

                // Fire tool execution hook
                self.hooks.fire(crate::hooks::HookEvent::ToolExecuted {
                    tool_name: tc.function.name.clone(),
                    success: matches!(result, ToolResult::Success(_)),
                }).await;

                self.conversation_history.push(ChatMessage {
                    role: "tool".into(),
                    content: result.to_json(),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                ..Default::default()});
            }
        }
    }

    // ── Tool call extraction ──
    // Parses structured JSON tool calls from model output.
    // Handles: ```json blocks, bare JSON, and <function_call> tags.
    // Uses proper JSON deserialization — no heuristic substring hacking.

    fn extract_tool_calls(&self, content: &str) -> Vec<ToolCall> {
        let mut calls = Vec::new();

        // Strategy 1: Parse ```json code blocks (structured JSON array or object)
        let mut search_idx = 0;
        while let Some(fc) = content[search_idx..].find("```json") {
            let json_start = search_idx + fc + 7;
            if let Some(rest) = content[json_start..].find("```") {
                let json_str = content[json_start..json_start + rest].trim();
                search_idx = json_start + rest + 3;
                calls.extend(Self::parse_tool_calls_from_json(json_str));
            } else {
                break;
            }
        }

        // Strategy 2: Parse <function_call> XML-like tags (some models)
        let mut search_idx = 0;
        while let Some(fc) = content[search_idx..].find("<function_call>") {
            let inner_start = search_idx + fc + 15;
            if let Some(rest) = content[inner_start..].find("</function_call>") {
                let json_str = content[inner_start..inner_start + rest].trim();
                search_idx = inner_start + rest + 16;
                calls.extend(Self::parse_tool_calls_from_json(json_str));
            } else {
                break;
            }
        }

        // Deduplicate by call id
        let mut seen = std::collections::HashSet::new();
        calls.retain(|c| seen.insert(c.id.clone()));
        calls
    }

    /// Parse one or more tool calls from a JSON string.
    /// Accepts both a single object `{"name":...}` and an array `[{...}, ...]`.
    fn parse_tool_calls_from_json(json_str: &str) -> Vec<ToolCall> {
        let v: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        // Array of tool calls: [{name, arguments}, ...]
        if let Some(arr) = v.as_array() {
            return arr.iter().filter_map(Self::json_to_tool_call).collect();
        }

        // Single tool call object: {name, arguments}
        Self::json_to_tool_call(&v).into_iter().collect()
    }

    fn json_to_tool_call(v: &serde_json::Value) -> Option<ToolCall> {
        let name = v.get("name")
            .and_then(|n| n.as_str())
            .filter(|s| !s.is_empty())?;

        let args = v.get("arguments")
            .map(|a| a.to_string())
            .unwrap_or_else(|| "{}".to_string());

        Some(ToolCall {
            id: format!("call_{}", uuid::Uuid::new_v4().to_string().replace('-', "").chars().take(8).collect::<String>()),
            call_type: "function".into(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: args,
            },
        })
    }

    // ── Accessors ──

    pub fn history(&self) -> &[ChatMessage] { &self.conversation_history }
    pub fn clear_history(&mut self) { self.conversation_history.truncate(1); }
    pub fn set_system_prompt(&mut self, prompt: String) {
        if let Some(first) = self.conversation_history.first_mut() { first.content = prompt; }
    }
    /// Set permission mode (Claude Code pattern).
    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_evaluator.set_mode(mode);
    }
    /// Get current permission mode.
    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_evaluator.stats().mode
    }
    /// Manually trigger compaction (for /compact command).
    pub fn force_compact(&mut self) -> usize {
        let est = MicroCompactor::estimate_tokens(&self.conversation_history);
        let (compacted, stubbed) = self.micro_compactor.compact(&self.conversation_history, est, &self.thresholds);
        self.conversation_history = compacted;
        stubbed
    }
    /// Estimated token usage of current history.
    pub fn estimated_tokens(&self) -> usize {
        MicroCompactor::estimate_tokens(&self.conversation_history)
    }
    /// Get cumulative cost for this session.
    pub fn session_cost(&self) -> CostSummary {
        self.cost_tracker.summary()
    }

    /// Get session statistics (absorbed from deepseek-tui title bar message count).
    /// Returns (user_message_count, assistant_message_count, total_tokens).
    pub fn session_stats(&self) -> (usize, usize, usize) {
        let user = self.conversation_history.iter().filter(|m| m.role == "user").count();
        let assistant = self.conversation_history.iter().filter(|m| m.role == "assistant").count();
        (user, assistant, self.estimated_tokens())
    }

    /// Get unique providers used in this session.
    pub fn session_providers(&self) -> Vec<String> {
        let mut providers: Vec<String> = Vec::new();
        for msg in &self.conversation_history {
            // Provider info is tracked externally via MessageMeta in the coordinator
            // This is a best-effort: extract from content footers if present
            if msg.role == "assistant" {
                // TUI pattern: "-- provider_name" at end of message
                for line in msg.content.lines().rev().take(3) {
                    if let Some(prov) = line.trim().strip_prefix("-- ") {
                        let name = prov.split_whitespace().next().unwrap_or("");
                        if !providers.contains(&name.to_string()) {
                            providers.push(name.to_string());
                        }
                    }
                }
            }
        }
        providers
    }
}
