//! # DeepSeek Carp — AI Coding Assistant
//!
//! A multi-provider AI coding assistant that orchestrates local models
//! (Qwen, Kimi, GLM, DeepSeek) and official APIs with intelligent routing.
//!
//! ## Architecture
//!
//! ```text
//! CLI / TUI Layer
//!   ├── cli/           — Argument parsing & command dispatch
//!   └── tui/           — Terminal UI rendering
//!
//! Core Layer
//!   ├── config/        — Configuration (~/.deepseek-carp/config.toml)
//!   ├── providers/     — Multi-provider orchestration engine
//!   ├── completion/    — FIM code completion
//!   ├── agent/         — Conversation agent loop
//!   ├── tools/         — MCP & built-in tools
//!   └── memory/        — Conversation memory & context
//!
//! Integration Layer
//!   └── enterprise/    — CarpAI Enterprise compute node connector
//! ```

pub mod config;
pub mod error;
pub mod providers;
pub mod completion;
pub mod inference;
pub mod agent;
pub mod context;
pub mod sdk;
pub mod tools;
pub mod memory;
pub mod storage;
pub mod streaming;
pub mod mcp;
pub mod cli;
pub mod tui;
pub mod hooks;
pub mod setup;
pub mod external;
pub mod observability;
pub mod security;
pub mod ide_integration;
pub mod monitoring;
pub mod finetune;
pub mod skills;
pub mod collab;
pub mod cost;
pub mod benchmark;
pub mod audio;
pub mod resilience;
pub mod vision;
pub mod testing;
pub mod audit;
pub mod logging;
pub mod plugin;
pub mod validation;
pub mod e2e;
pub mod sandbox;
pub mod codegraph;
pub mod review;
pub mod rules;
pub mod knowledge;
pub mod company;
pub mod r#loop;
pub mod test;

#[cfg(feature = "enterprise")]
pub mod enterprise;

// Re-exports
pub use config::DeepSeekConfig;
pub use providers::{ProviderOrchestrator, ChatMessage};
pub use tools::{ToolExecutor, ToolRegistry};
pub use hooks::HookRegistry;
pub use agent::{
    Agent, AgentConfig, AgentCoordinator,
    Permission, PermissionEvaluator, PermissionMode,
    ContextThresholds, ContextLevel, MicroCompactor,
    CostTracker, CostSummary, ModelPricing,
    Plan, PlanManager, PlanStatus, plan_mode_prompt, execute_mode_prompt,
    SwarmCoordinator, SwarmResult, SwarmAgent, SwarmMessage, AgentState, DecomposedTask, SwarmStatus,
    CompileEngine, CompileResult, PlanExecutionPipeline,
    SessionId, SessionInfo,
    TaskQueue, TaskRecord, TaskExecutor, TaskQueueStatus, spawn_workers,
    code_agent::{
        CodeAgent, CodeAgentConfig, Workspace, RunResult, Artifact,
        ScriptGenerator, ScriptExecutor, SelfReflector,
        MAX_REFINEMENT_ROUNDS,
    },
};
pub use context::{resolve_references, FileReference, ReferenceResolver, RagContext};
pub use sdk::QueryEngine;
pub use memory::MemoryManager;
pub use observability::{metrics, metrics_report, MetricsRegistry};
pub use ide_integration::{
    IdeIntegration, ide_integration,
    SessionSyncManager, SharedSessionState, SyncMessage, SwarmSyncStatus,
    FileSyncManager, FileChangeEvent, FileChangeType, FileChangeSource,
    CursorSyncManager, CursorPosition,
    DiagnosticSyncManager, DiagnosticInfo, DiagnosticSeverity,
    WorkspaceSyncManager, WorkspaceState,
    VariableTrackingManager, VariableInfo, VariableTrackingSuggestion,
    LapceSyncState,
};
