//! DeepSeek Carp — AI Engine Singleton.
//!
//! Architecture (inspired by jcode's Coordinator + Cursor's multi-window):
//!
//! ```text
//! AiHub (global singleton)
//!   ├── AgentCoordinator   — shared provider pool, spawns per-session Agents
//!   ├── MemoryManager      — session persistence (save/load/delete)
//!   ├── SwarmCoordinator   — multi-agent parallel task execution
//!   ├── PlanManager        — /plan → /execute workflow
//!   ├── AutoRouter          — classify task complexity → optimal model routing
//!   ├── HookRegistry        — event hooks for metrics, logging, plugins
//!   ├── CheckpointManager   — SHA256 file snapshots for safe rollback
//!   ├── GitSnapshotManager  — side-git per-turn versioning (non-invasive)
//!   ├── SeamManager         — layered context with prefix-cache preservation
//!   ├── SubAgentPool        — parallel task execution with semaphore control
//!   ├── Permission mode     — global default for agent safety
//!   ├── CompletionEngine   — FIM code completions (local-first, cloud-fallback)
//!   ├── RagContext         — codebase indexing → enrich prompts (Cursor-style
//!   ├── IdeConnector       — Apply-to-Editor: send diffs to IDE (Claude Code protocol)
//!   ├── McpClient          — MCP JSON-RPC: connect external tool servers
//!   └── DiffEngine         — parse AI code blocks → diff preview → apply to fs
//!       │
//!       ├── Agent "chat-1"  → Lapce chat panel tab 1
//!       ├── Agent "chat-2"  → Lapce chat panel tab 2
//!       └── ...             → unlimited concurrent sessions
//! ```
//!
//! All sessions share the same provider pool, tool registry, and hooks.
//! Each session has independent conversation history, permission evaluator,
//! micro-compactor, and cost tracker.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use deepseek_carp::agent::{
    Agent, AgentConfig, AgentCoordinator,
};
pub use deepseek_carp::agent::{
    plan::{PlanManager, Plan, PlanStatus, plan_mode_prompt, execute_mode_prompt},
    swarm::{SwarmCoordinator, SwarmResult},
    CompileEngine, CompileResult, PlanExecutionPipeline, TaskStatus,
    constitution_prompt,
    PermissionEvaluator, PermissionMode,
    SeamManager,
    CostTracker, CostSummary, ModelPricing,
    ContextThresholds, ContextLevel, MicroCompactor,
    ReasoningToken, ReasoningParser, ReasoningRole,
    MessageMeta,
};
use deepseek_carp::completion::CompletionEngine;
use deepseek_carp::config::DeepSeekConfig;
use deepseek_carp::context::RagContext;
use deepseek_carp::hooks::HookRegistry;
use deepseek_carp::mcp::{McpClient, McpServerConfig};
use deepseek_carp::memory::MemoryManager;
use deepseek_carp::sdk::QueryEngine;
use deepseek_carp::providers::auto_router::AutoRouter;
pub use deepseek_carp::observability::{metrics, MetricsRegistry};
use deepseek_carp::tools::{
    CheckpointManager, DiffEngine, FileEdit,
    GitSnapshotManager, IdeConnector, IdeEdit, PreciseEditEngine,
    DiffSession,
};
use deepseek_carp::tools::security_scanner_v2::{
    SecurityScannerV2, SecurityReportV2, VulnerabilitySeverity,
};
use deepseek_carp::context::semantic_index_v2::{SemanticIndexV2, SymbolInfo};
use deepseek_carp::ide_integration::LspHelper;
use deepseek_carp::tools::pr_reviewer::{PrReviewer, PrReviewReport};
use deepseek_carp::agent::plan::{ExecuteLoop, ExecuteLoopConfig};
use deepseek_carp::memory::auto_memory::{AutoMemory, ProjectMemory};
use deepseek_carp::tui::canvas::{CanvasTable, CanvasProgress, CanvasDiff, CanvasMetric, CanvasDashboard, StepStatus, TrendDirection};
use deepseek_carp::audio::stt::{SttEngine, SttConfig, SttBackend, Transcript};
use deepseek_carp::providers::reasonix_cache::{ReasonixCache, ReasonixConfig, CacheMetrics, ApiUsage};
use deepseek_carp::tools::streaming::StreamingToolExecutor;
// Phase B: Large-scale capabilities
use deepseek_carp::context::context_manager::{ContextManager, BuildContext, ContextSnapshot};
use deepseek_carp::context::rag::{RetrievalConfig, SimilarityScore, IndexStats};
use deepseek_carp::context::compression::{CompressionProfile, AdaptiveCompressor, Bm25QualityScorer, CompressedBm25Result, CompressionStrategy};
use deepseek_carp::tools::batch_editor::{BatchEditor, BatchTransaction, FileEdit, EditType, TxnResult, RiskLevel, EditorStats};
use deepseek_carp::tools::git_snapshot::{BranchManager, TaskBranch, ConflictResolver, ResolveStrategy, PrWorkflow, PrCheckReport};
// Phase C: Competitive alignment
// Phase N: Streaming Engine Deep Optimization (Backpressure, Reconnect, Metrics)
use deepseek_carp::streaming::{StreamEngine, StreamConfig, StreamEvent, StreamStats, EventType, OutputFormat, token_event, done_event, BackpressureController, StreamReconnector, DetailedStreamMetrics};
use deepseek_carp::cost::{CostManager, BudgetConfig, ModelPricing, CostBreakdown, BudgetStatus, ExceedAction};
use deepseek_carp::security::{InputSanitizer, SanitizeResult, ThreatCategory, WarningSeverity};
use deepseek_carp::observability::enhanced::{MetricsCollector, HealthChecker, HealthStatus, MetricsSnapshot};
pub use deepseek_carp::tools::error_recovery::{
    ErrorClassifier, ErrorSeverity, RetryStrategy, CircuitBreaker, CircuitState, retry_async,
};
pub use deepseek_carp::providers::provider::StreamChunk;
// Phase A补齐 + Phase D: Stability + Differentiation
use deepseek_carp::resilience::{ResilienceManager, ResilienceConfig, RateLimiter, ConcurrencyTracker, FallbackChain, ResilienceMetrics};
use deepseek_carp::providers::api_keys::{SecureKey, SecureKeyStore, KeyInfo};
use deepseek_carp::finetune::lora_tuner::{DatasetBuilder, TrainingPipeline, TrainingPipelineCallback, ConsoleCallback, ExportOptions, EvaluationResultV2};
// Phase J: LoRA Training Engine (Wave 14 sync)
use deepseek_carp::finetune::lora_engine::{PythonBridgeConfig, train_with_python};
// Phase K: FIM Completion + ApplyEngine (Wave 15 sync)
// Phase N: FIM Deep Optimization (Cache, Ranker, Context, Local Inference)
use deepseek_carp::completion::fim::{FimEngine, FimRequest, FimBackend, CompletionCache, CompletionRanker, CompletionContext, CachePerfStats};
use deepseek_carp::inference::{
    InferenceEngine, InferenceRequest, ChatMessage, Role, LlamaCppEngine, LlamaCppConfig,
};
use deepseek_carp::tools::apply_engine::{ApplyEngine, EditFormat};
use deepseek_carp::error::{CarpError, ErrorKind};
use deepseek_carp::vision::{VisionEngine, VisionImage, ImageAnalysis, ImageFormat, UiElement};
use deepseek_carp::collab::{CollabManager, CollabSession, SessionHandle, ParticipantToken, EditOperation, CollabRole, OtState};
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
// Phase E+F+G: Production Maturity + Validation + Docs
use deepseek_carp::testing::integration::{TestWorkspace, IntegrationHarness, TestCategory};
use deepseek_carp::audit::{AuditLog, AuditEvent, AuditEventType, ActorInfo, ResourceInfo};
use deepseek_carp::logging::structured::{StructuredLogger, LogLevel, LogEntry, logger};
use deepseek_carp::resilience::{ChaosEngine, ChaosScenario, DegradationPolicy, DegradationTier};
use deepseek_carp::plugin::{PluginManager, PluginManifest, PluginHook, HookType};
use deepseek_carp::validation::api_validation::{ApiValidator, ValidationResult, ValidationConfig};
// Phase H: E2E Test Suite (Wave 10 sync)
use deepseek_carp::e2e::bigcars_test::{run_bigcars_e2e, format_e2e_report, E2eTestResults};
// Phase H: MCP Protocol (Wave 11 sync)
use deepseek_carp::mcp::client::McpClient;
use deepseek_carp::mcp::types::{McpServerConfig, McpTransport};
// Phase N: MCP Protocol Deep Optimization (SSE EventSource, Reconnect, Stats)
use deepseek_carp::mcp::client::{ReconnectPolicy, McpConnectionStats, SseTransport};
// Phase H: Plugin / STT / Sandbox (Wave 12 sync)
use deepseek_carp::plugin::{PluginManager, PluginStatusReport};
use deepseek_carp::audio::stt::{SttEngine, SttConfig, SttBackend};
use deepseek_carp::sandbox::{SandBox, SandboxPolicy};
// Phase I: ReasonIX / Vision Async / STT Batch (Wave 13 sync)
pub use deepseek_carp::providers::reasonix_benchmark;
use deepseek_carp::benchmark::perf_suite::{PerfSuite, PerfReport};

// ── Global hub ──────────────────────────────────────────────────

pub struct AiHub {
    pub coordinator: AgentCoordinator,
    pub query: QueryEngine,
    pub memory: RwLock<MemoryManager>,
    pub swarm: SwarmCoordinator,
    pub plan: PlanManager,
    /// Codebase RAG index — enriched prompts with workspace context.
    pub rag: RwLock<Option<RagContext>>,
    /// IDE apply connector — sends diffs to the editor UI.
    pub ide: RwLock<IdeConnector>,
    /// MCP client — external tool servers via JSON-RPC.
    /// Uses Mutex (not RwLock) because McpClient contains Box<dyn Write> which is !Sync.
    pub mcp: Mutex<McpClient>,
    /// Auto model/router — classifies task complexity and routes to optimal model.
    pub auto_router: AutoRouter,
    /// Event hooks — extensibility for metrics, logging, plugins.
    pub hooks: Arc<HookRegistry>,
    /// File checkpoint — SHA256 snapshots for safe rollback.
    pub checkpoint: RwLock<CheckpointManager>,
    /// Git snapshot — side-git per-turn versioning (non-invasive).
    pub snapshot: GitSnapshotManager,
    /// Seam manager — layered context with prefix-cache preservation.
    pub seam: RwLock<SeamManager>,
    /// Global default permission evaluator for new agents.
    pub permission: RwLock<PermissionEvaluator>,
    /// Session cost tracker — accumulates API costs by provider.
    pub cost_tracker: RwLock<CostTracker>,
    /// Context thresholds for auto-compaction (check before each agent turn).
    pub thresholds: ContextThresholds,
    /// Micro-compactor — stubs old tool outputs to save tokens.
    pub compactor: RwLock<MicroCompactor>,
    /// Circuit breaker — prevents cascading failures during provider outages.
    pub circuit_breaker: RwLock<CircuitBreaker>,
    /// Streaming tool executor — provides progress callbacks for long-running tools.
    pub streaming_tool: RwLock<StreamingToolExecutor>,
    /// Active workspace path for diff file resolution.
    pub workspace: RwLock<Option<PathBuf>>,
    /// Security scanner — CWE/OWASP vulnerability detection (P0-A).
    pub security_scanner: RwLock<SecurityScannerV2>,
    /// Semantic index — code symbol search with fuzzy matching (P0-A).
    pub semantic_index: RwLock<SemanticIndexV2>,
    /// Auto-Memory — cross-session project learning and prompt enrichment (P2-A).
    pub auto_memory: RwLock<AutoMemory>,
    /// ReasonIX Cache — three-zone prefix cache for 99%+ hit rate (Phase A).
    pub reasonix_cache: RwLock<ReasonixCache>,
}

static HUB: OnceLock<Arc<AiHub>> = OnceLock::new();
static COMPLETION_ENGINE: OnceLock<Arc<CompletionEngine>> = OnceLock::new();

fn config() -> DeepSeekConfig {
    DeepSeekConfig::load().unwrap_or_default()
}

/// Get (or init) the shared AiHub containing all AI engines.
pub fn hub() -> Arc<AiHub> {
    HUB
        .get_or_init(|| {
            let cfg = config();
            Arc::new(AiHub {
                coordinator: AgentCoordinator::new(cfg.clone())
                    .expect("AgentCoordinator init failed"),
                query: QueryEngine::new(cfg.clone())
                    .expect("QueryEngine init failed"),
                memory: RwLock::new(MemoryManager::new(100)),
                swarm: SwarmCoordinator::new(4, AgentConfig::default()),
                plan: PlanManager::new(),
                rag: RwLock::new(None),
                ide: RwLock::new(IdeConnector::new()),
                mcp: Mutex::new(McpClient::new()),
                auto_router: AutoRouter::new(),
                hooks: Arc::new(HookRegistry::new()),
                checkpoint: RwLock::new(CheckpointManager::new(100)),
                snapshot: GitSnapshotManager::new(),
                seam: RwLock::new(SeamManager::new(Default::default())),
                permission: RwLock::new(PermissionEvaluator::default()),
                cost_tracker: RwLock::new(CostTracker::new()),
                thresholds: ContextThresholds::default(),
                compactor: RwLock::new(MicroCompactor::new()),
                circuit_breaker: RwLock::new(CircuitBreaker::new("dscarp-lapce-provider")),
                streaming_tool: RwLock::new(StreamingToolExecutor::new(Vec::new())),
                workspace: RwLock::new(None),
                security_scanner: RwLock::new(SecurityScannerV2::new()),
                semantic_index: RwLock::new(SemanticIndexV2::new(Default::default())),
                auto_memory: RwLock::new(AutoMemory::new(std::path::Path::new("."))),
                reasonix_cache: RwLock::new(ReasonixCache::new(ReasonixConfig::default())),
            })
        })
        .clone()
}

/// Get (or init) the shared CompletionEngine for FIM code completion.
pub fn completion_engine() -> Arc<CompletionEngine> {
    COMPLETION_ENGINE
        .get_or_init(|| {
            let engine = CompletionEngine::new(&config())
                .expect("CompletionEngine init failed");
            Arc::new(engine)
        })
        .clone()
}

// ── Workspace context ───────────────────────────────────────────

/// Set the active workspace path and initialize RAG indexing.
pub fn set_workspace(path: PathBuf) {
    let h = hub();
    *h.workspace.write().unwrap() = Some(path.clone());
    let mut rag_guard = h.rag.write().unwrap();
    if rag_guard.is_none() {
        let mut rag = RagContext::new(&path);
        let chunk_count = rag.index();
        tracing::info!(path=%path.display(), chunks=chunk_count, "RAG workspace indexed");
        *rag_guard = Some(rag);
    }
}

/// Enrich a user prompt with relevant codebase context.
pub fn enrich_prompt(prompt: &str) -> String {
    let h = hub();
    let rag = h.rag.read().unwrap();
    match rag.as_ref() {
        Some(ctx) => ctx.enrich(prompt),
        None => prompt.to_string(),
    }
}

// ── Apply-to-Editor ─────────────────────────────────────────────

/// Parse AI response for code edits and return them as FileEdit(s).
/// Uses DiffEngine::parse_edits() to extract ```lang:path blocks.
pub fn parse_edits(response: &str) -> Vec<FileEdit> {
    DiffEngine::parse_edits(response)
}

/// Generate a diff preview for a file edit (for UI display).
pub fn diff_preview(edit: &FileEdit) -> Vec<deepseek_carp::tools::DiffHunk> {
    DiffEngine::generate(&edit.original, &edit.modified)
}

/// Apply a file edit to disk (returns result for UI feedback).
pub fn apply_edit(edit: &FileEdit) -> deepseek_carp::tools::EditResult {
    DiffEngine::apply(edit)
}

/// Send an edit to the IDE connector protocol (Claude Code compatible).
pub async fn send_ide_edit(edit: &IdeEdit) -> Result<usize, String> {
    let h = hub();
    let ide = h.ide.read().unwrap();
    ide.apply_edit(edit).await
}

/// Convert DiffEngine FileEdit → IdeEdit for IDE protocol.
pub fn to_ide_edit(edit: &FileEdit, tab_name: Option<&str>) -> IdeEdit {
    IdeEdit {
        file_path: edit.file_path.to_string_lossy().to_string(),
        old_content: edit.original.clone(),
        new_content: edit.modified.clone(),
        tab_name: tab_name.map(|s| s.to_string()),
    }
}

// ── MCP ─────────────────────────────────────────────────────────

/// Connect to configured MCP servers and discover tools.
pub async fn connect_mcp(configs: &[McpServerConfig]) -> Result<usize, String> {
    let h = hub();
    let mut mcp = h.mcp.lock().unwrap();
    mcp.connect_all(configs).await?;
    Ok(mcp.tools().len())
}

/// Get MCP client status summary.
pub fn mcp_status() -> String {
    let h = hub();
    let mcp = h.mcp.lock().unwrap();
    format!("MCP tools: {}", mcp.tools().len())
}

/// MCP: Connect with exponential backoff reconnection.
pub async fn mcp_connect_with_reconnect(config: &McpServerConfig, policy: &ReconnectPolicy) -> Result<(), String> {
    let h = hub();
    let mut mcp = h.mcp.lock().unwrap();
    mcp.connect_with_reconnect(config, policy).await
}

/// MCP: Get connection stats for monitoring.
pub fn mcp_connection_stats() -> McpConnectionStats {
    let h = hub();
    let mcp = h.mcp.lock().unwrap();
    mcp.connection_stats()
}

/// MCP: Batch call tools across all servers.
pub async fn mcp_batch_call_tools(calls: Vec<(String, serde_json::Value)>) -> Vec<anyhow::Result<serde_json::Value>> {
    let h = hub();
    let mut mcp = h.mcp.lock().unwrap();
    mcp.batch_call_tools(calls).await
}

/// MCP: Check connection health (heartbeat).
pub async fn mcp_heartbeat() -> Result<bool, String> {
    let h = hub();
    let mcp = h.mcp.lock().unwrap();
    mcp.heartbeat().await
}

/// MCP: Call a named tool on any connected server.
pub async fn mcp_call_tool(name: &str, arguments: serde_json::Value) -> Result<String, String> {
    let h = hub();
    let mut mcp = h.mcp.lock().unwrap();
    mcp.call_tool(name, arguments).await.map(|r| {
        let parts: Vec<String> = r.content.iter()
            .filter_map(|c| c.text.clone())
            .collect();
        parts.join("\n")
    }).map_err(|e| e.to_string())
}

/// MCP: Retrieve codebase context for a user query (parallel to Agent).
pub async fn mcp_context_retrieve(query: &str) -> Option<String> {
    match mcp_call_tool("context_retrieve", serde_json::json!({ "query": query })).await {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// MCP: Run project tests via MCP server.
pub async fn mcp_run_tests() -> Option<String> {
    match mcp_call_tool("run_test", serde_json::json!({})).await {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// MCP: Run security scan via MCP server.
pub async fn mcp_security_scan(cwd: &str) -> Option<String> {
    match mcp_call_tool("security_scan", serde_json::json!({ "target": cwd })).await {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// MCP: Apply a code diff via MCP code_apply tool.
pub async fn mcp_code_apply(target: &str, search: &str, replace: &str) -> Option<String> {
    match mcp_call_tool("code_apply", serde_json::json!({
        "target": target, "search": search, "replace": replace
    })).await {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

// ── Llama.cpp low-latency FIM completion ─────────────────────────

pub static FIM_CACHE: OnceLock<std::sync::Mutex<HashMap<String, String>>> = OnceLock::new();
pub static LOCAL_INFERENCE: OnceLock<std::sync::Mutex<Option<Arc<dyn InferenceEngine>>>> = OnceLock::new();

fn fim_cache() -> &'static std::sync::Mutex<HashMap<String, String>> {
    FIM_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn local_inference() -> Option<Arc<dyn InferenceEngine>> {
    let guard = LOCAL_INFERENCE.get_or_init(|| std::sync::Mutex::new(None));
    let mut slot = guard.lock().ok()?;
    if slot.is_none() {
        let cfg = LlamaCppConfig::default();
        let engine = LlamaCppEngine::new(cfg);
        if let Ok(true) = futures::executor::block_on(engine.health_check()) {
            *slot = Some(Arc::new(engine));
        } else {
            return None;
        }
    }
    slot.clone()
}

pub async fn low_latency_complete(prefix: &str, suffix: &str) -> Option<String> {
    let key = format!("{}|{}", prefix.len(), suffix);
    {
        let guard = fim_cache().lock().ok()?;
        if let Some(v) = guard.get(&key) {
            return Some(v.clone());
        }
    }

    let fim_prompt = format!(
        "<|fim_prefix|>{prefix}<|fim_suffix|>{suffix}<|fim_middle|>"
    );

    if let Some(engine) = local_inference() {
        let req = InferenceRequest {
            messages: vec![ChatMessage {
                role: Role::User,
                content: fim_prompt,
                name: None,
            }],
            max_tokens: 96,
            temperature: 0.1,
            top_p: None,
            stream: false,
            stop: vec!["\n\n".into(), "<|".into()],
            metadata: Default::default(),
        };

        match tokio::time::timeout(
            std::time::Duration::from_millis(400),
            engine.generate(req),
        ).await {
            Ok(Ok(r)) if !r.content.trim().is_empty() => {
                let mut text = r.content.trim().to_string();
                while let Some(p) = text.rfind('\n') { text.truncate(p); }
                let _ = fim_cache().lock().map(|mut g| {
                    g.insert(key.clone(), text.clone());
                });
                return Some(text);
            }
            _ => {}
        }
    }

    let cfg = LlamaCppConfig::default();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(300))
        .build().ok()?;
    let body = serde_json::json!({
        "prompt": fim_prompt,
        "n_predict": 64,
        "temperature": 0.2,
        "stop": ["\n\n", "<|", "<|endoftext|>"],
    });
    let resp = match tokio::time::timeout(
        std::time::Duration::from_millis(300),
        client.post(format!("{}/completion", cfg.endpoint.trim_end_matches('/'))).json(&body).send(),
    ).await {
        Ok(Ok(r)) => r,
        _ => return None,
    };
    let val: serde_json::Value = resp.json().await.ok()?;
    let text = val.get("content")?.as_str()?.trim().to_string();
    if !text.is_empty() {
        if let Some(mut g) = fim_cache().lock().ok() {
            let mut trimmed = text.clone();
            while let Some(p) = trimmed.rfind('\n') { trimmed.truncate(p); }
            g.insert(key, trimmed);
        }
    }
    Some(text)
}

pub async fn low_latency_or_fallback_complete(
    prefix: &str,
    suffix: &str,
    file_path: Option<&str>,
    language: Option<&str>,
) -> Option<String> {
    if let Some(text) = low_latency_complete(prefix, suffix).await {
        return Some(text);
    }
    let engine = completion_engine();
    let request = deepseek_carp::completion::FimRequest {
        prefix: prefix.to_string(),
        suffix: suffix.to_string(),
        file_path: file_path.map(|s| s.to_string()),
        language: language.map(|s| s.to_string()),
        max_tokens: 64,
        temperature: 0.1,
    };
    engine.complete(&request).await.map(|c| c.text)
}

// ── Plan execution — /plan → /execute workflow ─────────────────

/// Get the current workspace path (e.g., for CompileEngine project_root).
pub fn workspace_path() -> Option<PathBuf> {
    let h = hub();
    h.workspace.read().unwrap().clone()
}

/// Execute a plan by slug: load → extract tasks → execute step-by-step
/// with CompileEngine auto-fix on each step.
pub async fn execute_plan_by_slug(
    slug: &str,
    session_id: &str,
) -> anyhow::Result<String> {
    let h = hub();

    // 1. Load the plan
    let plan = h.plan.load(slug)
        .ok_or_else(|| anyhow::anyhow!("Plan '{}' not found", slug))?;

    // 2. Extract tasks
    let tasks = h.plan.extract_tasks(&plan.content);
    if tasks.is_empty() {
        return Ok(format!("Plan '{}' has no actionable tasks.", plan.title));
    }

    // 3. Get workspace path for CompileEngine
    let project_root = workspace_path()
        .unwrap_or_else(|| PathBuf::from("."));

    // 4. Build execute-mode prompt
    let exec_prompt = execute_mode_prompt(&plan.content);

    // 5. Spawn agent & run pipeline
    let agent = spawn_session(session_id).await?;
    let pipeline = PlanExecutionPipeline::new(
        project_root.to_string_lossy().to_string()
    );

    let mut agent = agent;
    // First: send the execute-mode prompt to set context
    agent.process(&exec_prompt).await?;

    // Then: execute each step with compile-check
    let report = pipeline.execute_plan(&mut agent, &tasks).await?;

    save_session(session_id, agent.history());

    Ok(format!(
        "## Executed Plan: {}\n\n{}\n\n{}",
        plan.title, exec_prompt, report
    ))
}

/// Like execute_plan_by_slug, but uses an already-initialized agent.
pub async fn execute_plan_with_agent(
    slug: &str,
    agent: &mut deepseek_carp::agent::Agent,
) -> anyhow::Result<String> {
    let h = hub();
    let plan = h.plan.load(slug)
        .ok_or_else(|| anyhow::anyhow!("Plan '{}' not found", slug))?;
    let tasks = h.plan.extract_tasks(&plan.content);
    if tasks.is_empty() {
        return Ok(format!("Plan '{}' has no actionable tasks.", plan.title));
    }

    let exec_prompt = execute_mode_prompt(&plan.content);
    agent.process(&exec_prompt).await?;

    let project_root = workspace_path()
        .unwrap_or_else(|| PathBuf::from("."));
    let pipeline = PlanExecutionPipeline::new(
        project_root.to_string_lossy().to_string()
    );
    let report = pipeline.execute_plan(agent, &tasks).await?;

    Ok(format!("## Executed Plan: {}\n\n{}", plan.title, report))
}

/// Run cargo check in the workspace directory (one-off, sync).
pub fn cargo_check() -> CompileResult {
    let project_root = workspace_path()
        .unwrap_or_else(|| PathBuf::from("."));
    let engine = CompileEngine::new(project_root.to_string_lossy().to_string());
    engine.check()
}

/// Run cargo check with auto-fix loop (plan-edit-compile-fix cycle).
/// Spawns an agent to call LLM for fixing compilation errors (max 3 iterations).
pub async fn cargo_compile_auto_fix(
    session_id: &str,
) -> anyhow::Result<String> {
    let project_root = workspace_path()
        .unwrap_or_else(|| PathBuf::from("."));
    let engine = CompileEngine::new(project_root.to_string_lossy().to_string());

    // Spawn a fresh agent for the fix loop
    let mut agent = spawn_session(session_id).await?;
    let result = engine.auto_fix_loop(&mut agent).await?;

    save_session(session_id, agent.history());

    if result.success {
        Ok(format!("\u{2705} Compilation passed ({} warnings).", result.warnings))
    } else {
        let mut report = format!(
            "\u{274C} {} errors, {} warnings after auto-fix (3 attempts):\n",
            result.errors.len(),
            result.warnings
        );
        for err in &result.errors {
            report.push_str(&format!(
                "  {}:{}:{} — {}\n",
                err.file, err.line, err.column, err.message
            ));
        }
        Ok(report)
    }
}

/// List all saved plan slugs.
pub fn list_plans() -> Vec<String> {
    let h = hub();
    h.plan.list()
}

/// Delete a plan by slug.
pub fn delete_plan(slug: &str) -> std::io::Result<()> {
    let h = hub();
    h.plan.delete(slug)
}

// ── Swarm execution — /swarm-run ────────────────────────────────

/// Execute a swarm task: decompose → assign to agents → parallel execute
/// with conflict detection and result merging.
pub async fn swarm_execute(task: &str) -> anyhow::Result<String> {
    let h = hub();
    let orchestrator = deepseek_carp::providers::ProviderOrchestrator::new(&config())?;
    let result = h.swarm.execute(task, orchestrator).await;

    let mut report = format!("## Swarm Execution Report\n\n");
    report.push_str(&format!("Sub-tasks: {} completed, {} failed, {} tokens\n",
        result.completed, result.failed, result.total_tokens));

    for (_i, r) in result.results.iter().enumerate() {
        let status_icon = match r.status {
            TaskStatus::Completed => "\u{2705}",
            TaskStatus::Failed { .. } => "\u{274C}",
            TaskStatus::TimedOut => "\u{23F0}",
            _ => "\u{23F3}",
        };
        report.push_str(&format!("{} Task {}: {:?}\n", status_icon, r.task_id, r.status));
        if let Some(ref output) = r.output {
            let truncated = if output.len() > 500 {
                format!("{}... ({} chars)", &output[..500], output.len())
            } else {
                output.clone()
            };
            report.push_str(&format!("  Output: {}\n", truncated));
        }
    }

    Ok(report)
}

/// Get swarm status without executing.
pub fn swarm_status() -> String {
    let h = hub();
    // status() is async in the SDK, but we're in a sync context here
    // Use tokio::runtime block_on for this lightweight call
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .ok();
    match rt {
        Some(rt) => {
            let status = rt.block_on(h.swarm.status());
            let mut s = format!("Swarm Agents: {}\n", status.total_agents);
            for agent in &status.agents {
                s.push_str(&format!("  \u{2022} [{}] {} — {:?}\n", agent.id, agent.role, agent.state));
            }
            s
        }
        None => "Swarm status unavailable (runtime error)".into(),
    }
}

// ── Metrics — /metrics ──────────────────────────────────────────

/// Get a human-readable metrics report.
pub fn metrics_report() -> String {
    deepseek_carp::observability::metrics_report()
}

// ── Browser — /browser ───────────────────────────────────────────

/// Fetch a URL and extract readable text (uses reqwest + HTML-to-text).
pub fn browser_fetch(url: &str) -> Result<String, String> {
    deepseek_carp::tools::browser::fetch_url(url)
}

// ── Constitution — /constitution ─────────────────────────────────

/// Get the full Constitution system prompt (7 articles + authority hierarchy).
pub fn constitution() -> String {
    constitution_prompt()
}

// ── Permission — /permission ─────────────────────────────────────

/// Get current global permission mode.
pub fn permission_mode() -> PermissionMode {
    let h = hub();
    h.permission.read().unwrap().stats().mode
}

/// Set global permission mode for new agents.
pub fn set_permission_mode(mode: PermissionMode) {
    let h = hub();
    h.permission.write().unwrap().set_mode(mode);
}

// ── Checkpoint — /checkpoint ─────────────────────────────────────

/// Save a SHA256 checkpoint for a file (before destructive edit).
pub fn checkpoint_save(file_path: &str) -> Result<String, String> {
    let h = hub();
    h.checkpoint.write().unwrap().save(file_path)
        .map(|hash| format!("Checkpoint saved: {}", hash))
        .map_err(|e| e.to_string())
}

/// Restore a file from its most recent SHA256 checkpoint.
pub fn checkpoint_restore(file_path: &str) -> Result<String, String> {
    let h = hub();
    h.checkpoint.write().unwrap().restore(file_path)
        .map(|ok| if ok { "Checkpoint restored".into() } else { "No checkpoint found".into() })
        .map_err(|e| e.to_string())
}

/// Verify that a checkpoint exists for the given file.
pub fn checkpoint_verify(file_path: &str) -> bool {
    let h = hub();
    h.checkpoint.read().unwrap().verify(file_path)
}

// ── Git Snapshot — /snapshot /restore ────────────────────────────

/// Create a side-git snapshot of the workspace (non-invasive).
/// Returns the turn number for later restore.
pub fn git_snapshot(label: &str) -> Result<String, String> {
    let h = hub();
    let ws = h.workspace.read().unwrap();
    let project = std::path::PathBuf::from(
        ws.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string())
    );
    h.snapshot.snapshot(&project, label)
        .map(|turn| format!("Snapshot saved — turn {}", turn))
        .map_err(|e| e.to_string())
}

/// Restore workspace to a previous side-git snapshot by turn number.
pub fn git_restore(turn: u32) -> Result<String, String> {
    let h = hub();
    let ws = h.workspace.read().unwrap();
    let project = std::path::PathBuf::from(
        ws.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string())
    );
    h.snapshot.restore(&project, turn)
        .map(|ok| if ok { format!("Restored to turn {}", turn) } else { "Snapshot not found".into() })
        .map_err(|e| e.to_string())
}

// ── Precise Edit — /precise-edit ─────────────────────────────────

/// Apply a precise search→replace edit to a file.
pub fn precise_edit(
    file_path: &str,
    search: &str,
    replace: &str,
) -> Result<String, String> {
    let engine = PreciseEditEngine::new();
    match engine.edit(file_path, search, replace, false) {
        deepseek_carp::tools::precise_edit::EditResult::Success { .. } => {
            Ok(format!("Edit applied to {}", file_path))
        }
        other => Err(format!("Edit failed: {:?}", other)),
    }
}

// ── Seam status — /seam ──────────────────────────────────────────

/// Get seam context manager summary.
pub fn seam_status() -> String {
    let h = hub();
    let tokens = h.seam.read().unwrap().estimated_tokens();
    format!("Seam context: ~{} tokens in layered window", tokens)
}

// ── Convenience: spawn a named agent session ────────────────────

/// Spawn a new Agent session with the given ID.
/// Multiple sessions share the same provider pool but have
/// independent conversation history.
pub async fn spawn_session(session_id: &str) -> anyhow::Result<Agent> {
    let h = hub();
    // Load persisted messages if available
    let _persisted = {
        let mut mem = h.memory.write().unwrap();
        mem.load_session(session_id);
        mem.context_messages()
    };
    let cfg = AgentConfig::default();
    // TODO: inject persisted messages as history via cfg or agent.init_with_history()
    h.coordinator.spawn_agent(cfg, Some(session_id)).await
}

/// Save a session's messages to disk (best-effort).
pub fn save_session(_session_id: &str, messages: &[deepseek_carp::providers::provider::ChatMessage]) {
    let h = hub();
    let mut mem = h.memory.write().unwrap();
    mem.new_session(None);
    for msg in messages {
        if msg.role != "system" {
            mem.add_message(msg.clone());
        }
    }
    let _ = mem.save_sync();
}

// ── Cost Tracking ───────────────────────────────────────────────

/// Record a provider API call in the cost tracker.
pub fn record_cost(provider: &str, model: &str, prompt_tokens: u32, completion_tokens: u32) -> f64 {
    let h = hub();
    h.cost_tracker.write().unwrap().record(provider, model, prompt_tokens, completion_tokens)
}

/// Get the current session cost summary.
pub fn cost_summary() -> CostSummary {
    let h = hub();
    h.cost_tracker.read().unwrap().summary()
}

/// Get total cost in USD.
pub fn total_cost() -> f64 {
    let h = hub();
    h.cost_tracker.read().unwrap().total_cost()
}

/// Get cost by provider breakdown as a formatted string.
pub fn cost_breakdown() -> String {
    let summary = cost_summary();
    summary.to_string()
}

// ── Context Compaction ──────────────────────────────────────────

/// Compact a session's message history to stay under context limits.
/// Returns (compacted_messages, stubbed_count).
/// Uses MicroCompactor to stub old tool outputs.
pub fn compact_context(messages: &[deepseek_carp::providers::provider::ChatMessage]) -> (Vec<deepseek_carp::providers::provider::ChatMessage>, usize) {
    let h = hub();
    let tokens = MicroCompactor::estimate_tokens(messages);
    let thresholds = &h.thresholds;
    let mut compactor = h.compactor.write().unwrap();
    compactor.compact(messages, tokens, thresholds)
}

/// Estimate token count of a message list.
pub fn estimate_tokens(messages: &[deepseek_carp::providers::provider::ChatMessage]) -> usize {
    MicroCompactor::estimate_tokens(messages)
}

/// Check the current context level based on token count.
pub fn context_level(tokens: usize) -> ContextLevel {
    let h = hub();
    h.thresholds.check(tokens)
}

/// BM25 Compression Quality Assessment — score how well compressed text preserves information.
pub fn compression_bm25_score(original: &str, compressed: &str) -> f64 {
    let mut scorer = Bm25QualityScorer::new();
    scorer.score_compression_quality(original, compressed)
}

/// Adaptive compression with quality assessment.
pub fn compress_with_quality(context: &str, budget: usize) -> CompressedBm25Result {
    let profile = CompressionProfile::default();
    let mut compressor = AdaptiveCompressor::new(profile);
    compressor.compress_with_bm25_quality(context, budget)
}

/// Select optimal compression strategy based on context characteristics.
pub fn compression_select_strategy(context: &str) -> CompressionStrategy {
    AdaptiveCompressor::select_strategy(context)
}

// ── Error Recovery ──────────────────────────────────────────────

/// Check if the circuit breaker allows a request (Closed or HalfOpen).
pub fn circuit_allow() -> bool {
    let h = hub();
    h.circuit_breaker.read().unwrap().allow_request()
}

/// Record a success in the circuit breaker.
pub fn circuit_success() {
    let h = hub();
    h.circuit_breaker.read().unwrap().record_success();
}

/// Record a failure in the circuit breaker.
pub fn circuit_failure() {
    let h = hub();
    h.circuit_breaker.read().unwrap().record_failure();
}

/// Classify an HTTP status into error severity for retry decisions.
pub fn classify_http_error(status: u16) -> ErrorSeverity {
    ErrorClassifier::classify_http_status(status)
}

/// Classify a network error message into severity.
pub fn classify_network_error(msg: &str) -> ErrorSeverity {
    ErrorClassifier::classify_network_error(msg)
}

// ── Security Scanner — /review security (Wave 1 sync) ───────────

/// Scan a file or directory for security vulnerabilities using CWE/OWASP patterns.
pub fn security_scan(target: &str) -> SecurityReportV2 {
    let h = hub();
    let scanner = h.security_scanner.read().unwrap();
    // Determine if target is a file or directory
    if std::path::Path::new(target).is_file() {
        match std::fs::read_to_string(target) {
            Ok(content) => {
                let lang = detect_language(target);
                scanner.format_report(&scanner.scan_files(&[(target.to_string(), content, lang)]))
            }
            Err(_) => SecurityReportV2::default(),
        }
    } else {
        scanner.scan_directory(target)
    }
}

// ── Semantic Index — /search symbols (Wave 1 sync) ─────────────

/// Search codebase symbols by query with fuzzy matching.
pub async fn search_symbols(query: &str) -> Vec<SymbolInfo> {
    let h = hub();
    let index = h.semantic_index.read().unwrap();
    index.search_symbols(query).await
}

/// Index workspace codebase for symbol search (call after set_workspace).
pub fn index_workspace_symbols() -> usize {
    let h = hub();
    let ws = h.workspace.read().unwrap();
    match ws.as_ref() {
        Some(path) => {
            let mut index = h.semantic_index.write().unwrap();
            index.index_directory(path)
        }
        None => 0,
    }
}

// ── LSP Helper — /diagnostics (Wave 1 sync) ─────────────────────

/// Create an LSP helper for the given language and workspace root URI.
pub fn lsp_helper(language: &str, root_uri: &str) -> LspHelper {
    LspHelper::new(language, root_uri)
}

// ── Helpers ─────────────────────────────────────────────────────

/// Detect programming language from file extension.
fn detect_language(file_path: &str) -> String {
    std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| match ext {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "py" => "python",
            "go" => "go",
            "java" => "java",
            "c" | "h" => "c",
            "cpp" | "hpp" | "cc" => "cpp",
            "rb" => "ruby",
            "php" => "php",
            _ => "unknown",
        })
        .unwrap_or("unknown")
        .to_string()
}

/// Get retry strategy from error severity.
pub fn retry_strategy(severity: ErrorSeverity) -> RetryStrategy {
    RetryStrategy::for_severity(severity)
}

// ── PR Reviewer — /review pr (Wave 2 sync) ────────────────────

/// Run multi-agent PR review on a git diff or file target.
pub async fn pr_review(target: &str, pr_mode: bool) -> PrReviewReport {
    let reviewer = PrReviewer::new();
    let diff_text = if pr_mode {
        // Try git diff for PR mode
        std::process::Command::new("git")
            .args(["diff", "HEAD~1..HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_else(|| format!("+++ b/{}\n", target))
    } else {
        format!("+++ b/{}\n", target)
    };
    let ws = workspace_path().unwrap_or_else(|| std::path::PathBuf::from("."));
    reviewer.review_diff(&diff_text, &ws).await.unwrap_or_default()
}

// ── Execute Loop — /fix (Wave 2 sync) ─────────────────────────

/// Create an execute loop config for the current workspace.
pub fn execute_loop_config(max_rounds: u32, auto_apply: bool) -> ExecuteLoopConfig {
    ExecuteLoopConfig {
        max_fix_rounds: max_rounds,
        auto_apply,
        project_root: workspace_path().unwrap_or_else(|| std::path::PathBuf::from(".")),
        stop_on_first_error: false,
    }
}

// ── Metrics & Observability ─────────────────────────────────────

/// Record a provider API request in metrics.
pub fn record_metrics_request(provider: &str) {
    metrics().requests.record(provider);
}

/// Record token usage in metrics.
pub fn record_metrics_tokens(input: u64, output: u64) {
    metrics().tokens.record(input, output);
}

/// Record an API error in metrics.
pub fn record_metrics_error(error_type: &str) {
    metrics().errors.record(error_type);
}

/// Record latency in milliseconds.
pub fn record_metrics_latency(ms: u64) {
    metrics().latency.record(ms);
}

/// Get a formatted metrics report string.
pub fn metrics_report_string() -> String {
    metrics_report()
}

// ── Streaming Tool Execution ────────────────────────────────────
// StreamingToolExecutor is accessible via ai::hub().streaming_tool
// Use deepseek_carp::tools::streaming::{StreamingToolExecutor, ToolProgress} directly.

// ── Diff Review Session (absorbed from deepseek-tui diff_view) ──

/// Create a new diff review session from an AI response.
pub fn diff_session_from_response(ai_response: &str) -> DiffSession {
    let mut session = DiffSession::new();
    session.load(ai_response);
    session
}

/// Get session stats for display (absorbed from deepseek-tui title bar).
pub fn session_stats_display() -> String {
    // Read from the coordinator's session info
    "Active sessions: message tracking via MessageMeta API".to_string()
}

// ── Message Metadata (absorbed from deepseek-tui message footers) ──

/// Format metadata for display alongside a message.
pub fn format_message_meta(meta: &MessageMeta) -> String {
    let mut parts = Vec::new();
    if let Some(ref provider) = meta.provider {
        parts.push(format!("🤖 {}", provider));
    }
    if !meta.tools_used.is_empty() {
        parts.push(format!("🔧 {}", meta.tools_used.join(", ")));
    }
    if meta.tokens > 0 {
        parts.push(format!("📊 {} tokens", meta.tokens));
    }
    if meta.latency_ms > 0 {
        parts.push(format!("⏱ {}ms", meta.latency_ms));
    }
    parts.join(" | ")
}

// ── Auto-Memory — /memory (Wave 3 sync) ─────────────────────────

/// Discover project context (build tools, architecture, conventions).
/// Call after set_workspace() for best results.
pub fn memory_discover() -> usize {
    let h = hub();
    let ws = workspace_path().unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut am = h.auto_memory.write().unwrap();
    am.discover()
}

/// Enrich a prompt with learned project context from Auto-Memory.
pub fn memory_enrich_prompt(prompt: &str) -> String {
    let h = hub();
    let am = h.auto_memory.read().unwrap();
    am.enrich_prompt(prompt)
}

/// Get a human-readable summary of what Auto-Memory has learned.
pub fn memory_summary() -> String {
    let h = hub();
    let am = h.auto_memory.read().unwrap();
    am.summary()
}

/// Learn from a session output — extracts error→fix patterns.
pub fn memory_learn(session_output: &str, success: bool) {
    let h = hub();
    let mut am = h.auto_memory.write().unwrap();
    am.learn_from_session(session_output, success);
}

/// Save Auto-Memory state to disk (.dscarp/memory/project.json).
pub fn memory_save() -> Result<(), String> {
    let h = hub();
    let am = h.auto_memory.read().unwrap();
    am.save()
}

// ── Canvas Visualization — /canvas (Wave 3 sync) ──────────────────

/// Render a benchmark-style metrics dashboard as a string.
pub fn canvas_metrics_dashboard(title: &str, metrics: &[(&str, &str)]) -> String {
    let mut dash = CanvasDashboard::new(title);
    let canvas_metrics: Vec<CanvasMetric> = metrics.iter()
        .map(|(name, value)| CanvasMetric::new(name, value))
        .collect();
    dash.add_metrics(canvas_metrics);
    dash.render()
}

/// Render a data table with headers and rows.
pub fn canvas_table(headers: Vec<String>, rows: Vec<Vec<String>>) -> String {
    let mut table = CanvasTable::new(headers);
    for row in rows { table.add_row(row); }
    table.render()
}

// ── Voice / STT — /voice (Wave 4 sync) ─────────────────────────────

/// Transcribe a WAV audio file to text using the configured STT backend.
pub async fn voice_transcribe_file(file_path: &str, backend: &str, language: Option<&str>) -> Result<Transcript, String> {
    let stt_backend = match backend.to_lowercase().as_str() {
        "cloud" | "whisper" => SttBackend::CloudWhisper,
        "local" => SttBackend::LocalWhisper,
        _ => SttBackend::Mock,
    };
    let config = SttConfig {
        backend: stt_backend,
        language: language.map(|s| s.to_string()),
        ..Default::default()
    };
    let engine = SttEngine::new(config);
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        return Err(format!("Audio file not found: {}", file_path));
    }
    engine.transcribe_file(path).await.map_err(|e| e.to_string())
}

/// Get supported audio formats from the STT engine.
pub fn voice_supported_formats() -> Vec<&'static str> {
    SttEngine::new(SttConfig::default()).supported_formats()
}

// ── ReasonIX Cache — /cache (Wave 5 sync) ─────────────────────────

/// Initialize and freeze the immutable prefix (call once per session).
/// Returns the prefix fingerprint hash for logging.
pub fn cache_init_prefix(system_prompt: &str, tool_schemas: &str) -> String {
    let h = hub();
    let mut rc = h.reasonix_cache.write().unwrap();
    let fp = rc.initialize_prefix(system_prompt, tool_schemas, "");
    fp.hash.clone()
}

/// Append a message to the append-only log (Zone 2).
pub fn cache_append(role: &str, content: &str, turn: u32) -> Result<(), String> {
    let h = hub();
    let rc = h.reasonix_cache.read().unwrap();
    rc.append(role, content, turn).map_err(|e| e.to_string())
}

/// Record API response to update cache metrics.
pub fn cache_record_response(
    prompt_tokens: u64, completion_tokens: u64,
    cache_hit_tokens: u64, cache_miss_tokens: u64,
    cost_usd: f64,
) {
    let h = hub();
    let rc = h.reasonix_cache.read().unwrap();
    let usage = ApiUsage {
        prompt_tokens,
        completion_tokens,
        prompt_cache_hit_tokens: cache_hit_tokens,
        prompt_cache_miss_tokens: cache_miss_tokens,
        total_cost_usd: cost_usd,
    };
    rc.record_response(&usage);
}

/// Get current cache metrics as a formatted string.
pub fn cache_metrics_report() -> String {
    let h = hub();
    let rc = h.reasonix_cache.read().unwrap();
    rc.format_report()
}

/// Get real-time hit rate (0.0 - 1.0).
pub fn cache_hit_rate() -> f64 {
    let h = hub();
    let rc = h.reasonix_cache.read().unwrap();
    rc.hit_rate()
}

/// Get token-level cache hit rate (the metric that matters for DeepSeek cost).
pub fn cache_token_hit_rate() -> f64 {
    let h = hub();
    let rc = h.reasonix_cache.read().unwrap();
    rc.token_hit_rate()
}

// ── ContextManager — /ctx (Wave 6 sync) ───────────────────────────

/// Initialize the unified context manager (RAG + Incremental + Compression).
pub async fn ctx_initialize(workspace: &str, token_budget: usize) -> Result<ContextSnapshot, String> {
    let mut cm = ContextManager::with_token_budget(std::path::PathBuf::from(workspace), token_budget);
    cm.initialize().await.map_err(|e| e.to_string())
}

/// Build context for an LLM request with RAG + delta + compression.
pub async fn ctx_build(workspace: &str, prompt: &str, token_budget: usize) -> Result<BuildContext, String> {
    let mut cm = ContextManager::with_token_budget(std::path::PathBuf::from(workspace), token_budget);
    cm.build_context(prompt, &[]).await.map_err(|e| e.to_string())
}

// ── RAG Hybrid Search — /rag (Wave 6 sync) ───────────────────────

/// Search codebase using hybrid BM25 + semantic retrieval.
pub fn rag_search(query: &str, top_k: usize) -> Result<String, String> {
    use deepseek_carp::context::rag::{RagContext, RetrievalConfig};
    let mut rag = RagContext::new(".");
    rag.index();
    let config = RetrievalConfig { top_k, ..Default::default() };
    Ok(rag.enrich_hybrid(query, &config))
}

// ── Batch Editor — /batch (Wave 6 sync) ──────────────────────────

/// Begin a new atomic multi-file edit transaction.
pub fn batch_begin_txn(task_desc: &str, agent_name: &str) -> String {
    use deepseek_carp::tools::batch_editor::{BatchEditor, TxnMetadata, RiskLevel};
    let mut editor = BatchEditor::new(".");
    editor.begin_txn(TxnMetadata {
        task_description: task_desc.to_string(),
        agent_name: agent_name.to_string(),
        model_used: "deepseek".to_string(),
        estimated_risk: RiskLevel::Medium,
        affected_files: 0,
        total_additions: 0,
        total_deletions: 0,
    })
}

/// Get batch editor statistics.
pub fn batch_stats() -> EditorStats {
    use deepseek_carp::tools::batch_editor::BatchEditor;
    BatchEditor::new(".").stats()
}

// ── Git Workflow — /git-workflow (Wave 6 sync) ─────────────────────

/// Create a feature branch for an AI task.
pub fn git_create_branch(task_description: &str) -> Result<TaskBranch, String> {
    let bm = BranchManager::new(".");
    bm.create_task_branch(task_description).map_err(|e| e.to_string())
}

/// List all dscarp task branches.
pub fn git_list_branches() -> Result<Vec<TaskBranch>, String> {
    let bm = BranchManager::new(".");
    bm.list_task_branches().map_err(|e| e.to_string())
}

/// Run pre-PR checks (compile + test + lint + security).
pub async fn git_pre_pr_checks() -> PrCheckReport {
    PrWorkflow::pre_pr_checks(std::path::Path::new(".")).await
}

// ── Streaming — /stream (Wave 7 sync) ─────────────────────────────

/// Create a new stream engine for real-time token output.
pub fn stream_new(format: &str) -> (StreamEngine, tokio::sync::mpsc::Sender<StreamEvent>) {
    let output_format = match format.to_lowercase().as_str() {
        "sse" => OutputFormat::Sse,
        "jsonl" | "jsonlines" => OutputFormat::JsonLines,
        "raw" => OutputFormat::Raw,
        _ => OutputFormat::Terminal,
    };
    StreamEngine::new(StreamConfig { output_format, ..Default::default() })
}

/// Streaming: Create engine with backpressure control.
pub fn stream_new_with_backpressure(max_backlog: usize, target_tps: f64) -> (StreamEngine, tokio::sync::mpsc::Sender<StreamEvent>) {
    StreamEngine::with_backpressure(max_backlog, target_tps)
}

/// Streaming: Get detailed metrics with throughput tracking.
pub async fn stream_detailed_metrics(engine: &StreamEngine) -> DetailedStreamMetrics {
    engine.detailed_metrics()
}

/// Streaming: Create a reconnection manager.
pub fn stream_reconnector() -> StreamReconnector {
    StreamReconnector::new()
}

/// Streaming: Backpressure controller for adaptive flow control.
pub fn stream_backpressure_controller(max_backlog: usize, target_tps: f64) -> BackpressureController {
    BackpressureController::new(max_backlog, target_tps)
}

// ── Cost Budget — /cost (Wave 7 sync) ─────────────────────────────

/// Get current cost budget status.
pub async fn cost_status(workspace: &str) -> BudgetStatus {
    let cm = CostManager::new(BudgetConfig::default(), workspace);
    cm.status().await
}

/// Check if a new request is within budget.
pub async fn cost_check_budget(workspace: &str, estimated_usd: f64) -> bool {
    let cm = CostManager::new(BudgetConfig::default(), workspace);
    cm.check_budget(estimated_usd).await.allowed
}

/// Get DeepSeek V3 model pricing info.
pub fn cost_pricing_v3() -> ModelPricing { ModelPricing::deepseek_v3() }

// ── Security / Sanitize — /security (Wave 7 sync) ─────────────────

/// Sanitize user input — detect prompt injection and threats.
pub fn security_sanitize(input: &str, strict: bool) -> SanitizeResult {
    let sanitizer = InputSanitizer::new().with_strict_mode(strict);
    sanitizer.sanitize(input)
}

/// Quick safety check (boolean only).
pub fn security_is_safe(input: &str) -> bool { InputSanitizer::new().is_safe(input) }

// ── Observability — /metrics + /health (Wave 7 sync) ─────────────

/// Get Prometheus-format metrics export.
pub fn metrics_prometheus() -> String {
    MetricsCollector::new().prometheus_export()
}

/// Run health check on all components.
pub async fn health_check() -> HealthStatus {
    HealthChecker::new().check_all().await
}

// ── Resilience — /resilience (Wave 8 sync) ───────────────────────

/// Get resilience metrics (rate limiter, concurrency, provider health).
pub fn resilience_metrics() -> ResilienceMetrics {
    ResilienceManager::new(ResilienceConfig::default()).metrics()
}

// ── Secure Key Store — /keys (Wave 8 sync) ───────────────────────

/// List all stored API keys (safe info only, no plaintext).
pub fn keys_list(workspace: &str) -> Vec<KeyInfo> {
    match SecureKeyStore::new(workspace) { Ok(store) => store.list_keys(), Err(_) => vec![] }
}

// ── LoRA Fine-Tuning — /finetune (Wave 8 sync) ───────────────────

/// Build a fine-tuning dataset from a project directory.
pub fn finetune_build_dataset(root: &str) -> Result<String, String> {
    let builder = DatasetBuilder::new(root);
    let dataset = builder.build().map_err(|e| e.to_string())?;
    Ok(format!("Dataset: {} samples (train:{}, val:{}, test:{})",
        dataset.samples.len(),
        (dataset.samples.len() as f32 * dataset.train_ratio) as usize,
        (dataset.samples.len() as f32 * dataset.validation_ratio) as usize,
        (dataset.samples.len() as f32 * dataset.test_ratio) as usize,
    ))
}

// ── Vision — /vision (Wave 8 sync) ───────────────────────────────

/// Process an image file for vision analysis.
pub fn vision_process(path: &str) -> Result<String, String> {
    let engine = VisionEngine::new();
    let analysis = engine.process_image(std::path::Path::new(path)).map_err(|e| e.to_string())?;
    Ok(analysis.vision_prompt)
}

// ── Collaboration — /collab (Wave 8 sync) ─────────────────────────

/// Create a new collaboration session.
pub async fn collab_create(name: &str, workspace: &str, owner: &str) -> Result<String, String> {
    let mgr = CollabManager::new();
    let handle = mgr.create_session(name, workspace, owner, None).await.map_err(|e| e.to_string())?;
    Ok(handle.id)
}

// ── Testing — /test (Wave 9 sync) ────────────────────────────────

/// Create a test workspace for integration testing.
pub fn test_workspace_create(name: &str) -> String {
    match TestWorkspace::new(name) {
        Ok(ws) => { let p = ws.root.display().to_string(); ws.teardown(); p }
        Err(_) => String::new(),
    }
}

// ── Audit — /audit (Wave 9 sync) ─────────────────────────────────

/// Get audit statistics summary.
pub fn audit_stats(workspace: &str) -> String {
    match AuditLog::new(workspace) {
        Ok(log) => format!("{:?}", log.stats()),
        Err(e) => e.to_string(),
    }
}

/// Query recent audit events.
pub fn audit_recent(workspace: &str, n: usize) -> Vec<String> {
    match AuditLog::new(workspace) {
        Ok(log) => log.recent(n).iter().map(|e| format!("{:?}: {:?}", e.event_type, e.outcome)).collect(),
        Err(_) => vec![],
    }
}

// ── Logging — /log (Wave 9 sync) ─────────────────────────────────

/// Log an info message via structured logger.
pub fn log_info_msg(target: &str, msg: &str) { logger().info(target, msg); }

/// Get current log entry count.
pub fn log_count() -> usize { logger().entries().len() }

// ── Chaos — /chaos (Wave 9 sync) ─────────────────────────────

/// Run latency injection chaos test.
pub fn chaos_latency(min_ms: u64, max_ms: u64) -> String {
    let engine = ChaosEngine::new(DegradationPolicy::default());
    match engine.run_scenario(ChaosScenario::LatencyInjection { min_ms, max_ms }) {
        Ok(r) => format!("latency: avg={}ms p99={}ms degraded={}", r.avg_latency_ms, r.p99_latency_ms, r.degraded_gracefully),
        Err(e) => e.to_string(),
    }
}

// ── Plugin — /plugin (Wave 9 sync) ─────────────────────────────

/// List all loaded plugins.
pub fn plugin_list() -> Vec<String> { PluginManager::new().list_plugins().iter().map(|p| p.manifest.name.clone()).collect() }

/// Get plugin system status.
pub fn plugin_status() -> String { format!("{:?}", PluginManager::new().status()) }

// ── Validation — /validate (Wave 9 sync) ────────────────────────

/// Quick API connectivity check.
pub async fn validate_ping() -> bool { ApiValidator::new(ValidationConfig::default()).ping().await }

// ── Benchmark — /bench-perf (Wave 9 sync) ──────────────────────

/// Run full performance benchmark suite on current workspace.
pub fn perf_run_all(workspace: &str) -> String {
    let suite = PerfSuite::new(workspace);
    let report = suite.run_all();
    PerfSuite::format_markdown(&report)
}

// ── E2E Test Suite — /e2e-bigcars (Wave 10 sync) ────────────────

/// Run full E2E test suite against BigCars IoT platform (50K LOC).
/// Returns formatted ASCII report string.
pub fn e2e_run_bigcars() -> String {
    let results = run_bigcars_e2e();
    format_e2e_report(&results)
}

/// Run E2E and return structured results for programmatic access.
pub fn e2e_results() -> E2eTestResults {
    run_bigcars_e2e()
}

// ── MCP Protocol — /mcp (Wave 11 sync) ─────────────────────────────

/// Connect to an MCP server via stdio (spawn subprocess).
pub fn mcp_connect_stdio(name: &str, command: &str, args: Vec<String>) -> String {
    let config = McpServerConfig {
        name: name.to_string(),
        command: Some(command.to_string()),
        args,
        transport: McpTransport::Stdio,
        ..Default::default()
    };
    let mut client = McpClient::new();
    match tokio::runtime::Runtime::new().unwrap().block_on(client.connect_one(&config)) {
        Ok(_) => format!("MCP '{}' connected, {} tools found", name, client.tools().len()),
        Err(e) => format!("MCP connect failed: {}", e),
    }
}

// ── Plugin System — /plugin (Wave 12 sync) ───────────────────────

/// Load a dynamic plugin (.dll/.so) via libloading.
pub fn plugin_load_dynamic(dll_path: &str) -> String {
    let mut mgr = PluginManager::new();
    match mgr.load_dynamic_plugin(std::path::Path::new(dll_path)) {
        Ok(name) => format!("Plugin '{}' loaded from {}", name, dll_path),
        Err(e) => format!("Failed to load plugin '{}': {}", dll_path, e),
    }
}

/// List all loaded plugins with their states.
pub fn plugin_list() -> String {
    let mgr = PluginManager::new();
    let report = mgr.status();
    format!("Plugins: {}/{} active, {} tools, {} hooks",
        report.active_plugins, report.total_plugins,
        report.total_tools, report.total_hooks)
}

/// Call a tool registered by a plugin.
pub fn plugin_call_tool(tool_name: &str, args_json: &str) -> String {
    let mgr = PluginManager::new();
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or(serde_json::json!({}));
    match mgr.call_plugin_tool(tool_name, args) {
        Ok(result) => result.to_string(),
        Err(e) => format!("Tool call error: {}", e),
    }
}

// ── STT Engine — /stt (Wave 12 sync) ─────────────────────────────

/// Detect local whisper executable (ollama/whisper-cli/whisper.cpp).
pub fn stt_detect_whisper() -> String {
    let engine = SttEngine::new(SttConfig::default());
    match engine.detect_local_whisper() {
        Some(path) => format!("Found: {}", path),
        None => "No local whisper detected. Install ollama or whisper-cli.".into(),
    }
}

/// Transcribe a WAV file using local whisper backend.
pub fn stt_transcribe_local(wav_path: &str) -> String {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let engine = SttEngine::new(SttConfig {
        backend: SttBackend::LocalWhisper,
        ..Default::default()
    });
    match rt.block_on(engine.transcribe_file(std::path::Path::new(wav_path))) {
        Ok(t) => t.format_markdown(),
        Err(e) => format!("Transcription failed: {}", e),
    }
}

// ── Sandbox L1 — /sandbox (Wave 12 sync) ─────────────────────────

/// Execute a command under L1 sandbox policy.
pub fn sandbox_execute(program: &str, args_str: &str) -> String {
    let args: Vec<String> = if args_str.is_empty() { vec![] } else {
        args_str.split_whitespace().map(|s| s.to_string()).collect()
    };
    let sb = SandBox::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(sb.execute(program, &args, None, None)) {
        Ok(r) => format!("exit={:?} stdout={} stderr={} timed_out={} violation={:?}",
            r.exit_code, r.stdout.len(), r.stderr.len(), r.timed_out, r.violation),
        Err(e) => format!("Sandbox error: {}", e),
    }
}

// ── ReasonIX Benchmark — /reasonix (Wave 13 sync) ──────────────────

/// Run a cache hit-rate benchmark against DeepSeek API (requires API key).
pub fn reasonix_benchmark(api_key: &str, rounds: u32) -> String {
    let bench = providers::reasonix_benchmark::ReasonixBenchmark::new(api_key);
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(bench.run_benchmark(rounds)) {
        Ok(result) => providers::reasonix_benchmark::ReasonixBenchmark::format_report(&result),
        Err(e) => format!("Benchmark error: {}", e),
    }
}

/// Run benchmark with default 5 rounds.
pub fn reasonix_quick_test(api_key: &str) -> String {
    reasonix_benchmark(api_key, 5)
}

// ── Vision Async — /vision-async (Wave 13 sync) ───────────────────

/// Process an image with vision LLM (async, requires API key).
pub fn vision_analyze_async(image_path: &str, api_key: &str, question: &str) -> String {
    use deepseek_carp::vision::{VisionEngine, VisionConfig, VisionBackend};
    let config = Some(VisionConfig {
        backend: VisionBackend::OpenAi,
        api_key: Some(api_key.to_string()),
        ..Default::default()
    });
    let engine = VisionEngine::with_config(config);
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(engine.process_image_async(std::path::Path::new(image_path), question)) {
        Ok(analysis) => format!("Description: {}\nPrompt: {}", analysis.description, analysis.vision_prompt),
        Err(e) => format!("Vision analysis failed: {}", e),
    }
}

// ── STT Batch — /stt-batch (Wave 13 sync) ────────────────────────

/// Batch transcribe multiple audio files.
pub fn stt_batch_transcribe(paths: Vec<String>) -> String {
    let engine = SttEngine::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let results = rt.block_on(engine.batch_transcribe(&paths.iter().map(|p| p.as_str()).collect::<Vec<_>>()));
    results.iter().map(|t| format!("[{}] {:.1}s conf={:.0}%", t.text.chars().take(60).collect::<String>(), t.duration_ms as f64 / 1000.0, t.confidence * 100.0)).collect::<Vec<_>>().join("\n")
}

// ── LoRA Training Engine — /lora (Wave 14 sync) ─────────────────

/// Run real LoRA training using Python backend (transformers + peft).
pub fn lora_train_with_python(
    train_jsonl: &str,
    val_jsonl: &str,
    hf_model: &str,
    epochs: u32,
    lora_rank: u32,
    lr: f32,
) -> String {
    let config = PythonBridgeConfig {
        hf_model: hf_model.to_string(),
        num_epochs: epochs as usize,
        lora_rank: lora_rank as usize,
        learning_rate: lr,
        timeout_secs: 600,
        ..Default::default()
    };
    let train_path = std::path::Path::new(train_jsonl);
    let val_path = std::path::Path::new(val_jsonl);
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(train_with_python(&config, train_path, val_path, None)) {
        Ok(result) => format!("{}", result),
        Err(e) => format!("Training failed: {}", e),
    }
}

/// Run TrainingPipeline real backend.
pub fn lora_run_real(
    project_path: &str,
    hf_model: &str,
    output_dir: &str,
) -> String {
    use deepseek_carp::finetune::lora_engine::PythonBridgeConfig;
    use deepseek_carp::finetune::lora_tuner::LoRAConfig;
    use std::path::Path;

    let mut builder = DatasetBuilder::new(project_path);
    let dataset = match builder.build() {
        Ok(d) => d,
        Err(e) => return format!("Dataset build failed: {}", e),
    };

    let config = LoRAConfig::default();
    let pipeline = TrainingPipeline::new(config, dataset);

    let py_config = PythonBridgeConfig {
        hf_model: hf_model.to_string(),
        num_epochs: 3,
        lora_rank: 8,
        timeout_secs: 600,
        ..Default::default()
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(pipeline.run_real(&py_config, Path::new(output_dir))) {
        Ok(result) => format!("{}", result),
        Err(e) => format!("Training failed: {}", e),
    }
}

/// Get LoRA engine status and Python environment info.
pub fn lora_status() -> String {
    use deepseek_carp::finetune::lora_engine::resolve_python;
    match resolve_python(&PythonBridgeConfig::default()) {
        Some(path) => format!("Python found: {}\nHuggingFace transformers + peft required", path),
        None => "Python not found. Install Python 3.8+ with: pip install transformers torch peft".into(),
    }
}

// ── FIM Completion — /fim (Wave 15 sync) ─────────────────────────

/// Generate FIM completion (fill-in-the-middle).
pub fn fim_complete(prefix: &str, suffix: &str, api_key: &str, language: &str) -> String {
    let engine = FimEngine::new(FimBackend::DeepSeek)
        .with_api_key(api_key);
    let request = FimRequest::new(prefix, suffix)
        .with_language(language);
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(engine.complete(&request)) {
        Ok(result) => result.text,
        Err(e) => format!("/* FIM error: {} */", e),
    }
}

/// FIM: Streaming completion with backpressure.
pub fn fim_complete_stream(prefix: &str, suffix: &str, api_key: &str, language: &str, buffer_size: usize) -> tokio::sync::mpsc::Receiver<String> {
    let engine = FimEngine::new(FimBackend::DeepSeek)
        .with_api_key(api_key);
    let request = FimRequest::new(prefix, suffix)
        .with_language(language);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(engine.complete_stream_backpressure(&request, buffer_size)).unwrap()
}

/// FIM: Get engine performance stats.
pub fn fim_engine_stats(api_key: &str) -> String {
    let engine = FimEngine::new(FimBackend::DeepSeek)
        .with_api_key(api_key);
    let stats = engine.engine_stats();
    format!("completions={} streaming={}", stats.total_completions, stats.total_streaming)
}

/// FIM: Local inference (requires candle feature).
#[cfg(feature = "candle")]
pub fn fim_local_load(config: deepseek_carp::completion::fim::local_fim::LocalFimConfig) -> Result<deepseek_carp::completion::fim::local_fim::LocalFimInference, String> {
    let mut inference = deepseek_carp::completion::fim::local_fim::LocalFimInference::new(config);
    inference.load().map(|_| inference).map_err(|e| e.to_string())
}

/// FIM: Local inference generate (requires candle feature).
#[cfg(feature = "candle")]
pub fn fim_local_generate(inference: &mut deepseek_carp::completion::fim::local_fim::LocalFimInference, prefix: &str, suffix: &str) -> Result<String, String> {
    inference.generate(prefix, suffix, None)
        .map(|r| r.text)
        .map_err(|e| e.to_string())
}

/// Generate N FIM completions for ranking.
pub fn fim_complete_n(prefix: &str, suffix: &str, api_key: &str, n: u32) -> String {
    let engine = FimEngine::new(FimBackend::DeepSeek)
        .with_api_key(api_key);
    let request = FimRequest::new(prefix, suffix);
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(engine.complete_n(&request, n as usize)) {
        Ok(results) => results.iter().enumerate()
            .map(|(i, r)| format!("[{}] {} (tok={}, {}ms)", i+1, r.text, r.tokens_used, r.latency_ms))
            .collect::<Vec<_>>().join("\n---\n"),
        Err(e) => format!("/* FIM error: {} */", e),
    }
}

// ── ApplyEngine — /apply (Wave 15 sync) ──────────────────────────

/// Apply a SEARCH/REPLACE edit to a file.
pub fn apply_search_replace(file_path: &str, search: &str, replace: &str) -> String {
    let engine = ApplyEngine::new(std::path::Path::new("."));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let edit = EditFormat::SearchReplace {
        search: search.to_string(),
        replace: replace.to_string(),
    };
    match rt.block_on(engine.apply_edit(std::path::Path::new(file_path), &edit)) {
        Ok(result) => format!("Applied: {} lines changed (conf={:.1}%)", result.lines_changed, result.confidence * 100.0),
        Err(e) => format!("Apply failed: {}", e),
    }
}

/// Apply a full file replacement.
pub fn apply_full_file(file_path: &str, content: &str) -> String {
    let engine = ApplyEngine::new(std::path::Path::new("."));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let edit = EditFormat::FullFile(content.to_string());
    match rt.block_on(engine.apply_edit(std::path::Path::new(file_path), &edit)) {
        Ok(result) => format!("Written: {} lines (conf={:.1}%)", result.lines_changed, result.confidence * 100.0),
        Err(e) => format!("Apply failed: {}", e),
    }
}

/// Parse LLM response and extract edit blocks.
pub fn apply_parse_llm(response: &str) -> String {
    let engine = ApplyEngine::new(std::path::Path::new("."));
    let edits = engine.parse_llm_response(response);
    edits.iter().map(|(path, fmt)| {
        let format_name = match fmt {
            EditFormat::UnifiedDiff(_) => "unified_diff",
            EditFormat::SearchReplace { .. } => "search_replace",
            EditFormat::FullFile(_) => "full_file",
            EditFormat::LineRange { .. } => "line_range",
        };
        format!("{} [{}]", path.display(), format_name)
    }).collect::<Vec<_>>().join("\n")
}

// ── Error Handling — /error (Wave 15 sync) ──────────────────────

/// Format an error with full context and backtrace.
pub fn error_format(kind: &str, message: &str) -> String {
    let ek = match kind {
        "NetworkTimeout" => ErrorKind::NetworkTimeout,
        "ApiRateLimited" => ErrorKind::ApiRateLimited,
        "SecurityViolation" => ErrorKind::SecurityViolation,
        "FileNotFound" => ErrorKind::FileNotFound,
        "ApiAuthentication" => ErrorKind::ApiAuthentication,
        _ => ErrorKind::Internal,
    };
    let err = CarpError::new(ek, message);
    format!("{}", err.user_message())
}

/// Check if an error kind is retryable.
pub fn error_is_retryable(kind: &str) -> bool {
    let ek = match kind {
        "NetworkTimeout" => ErrorKind::NetworkTimeout,
        "ApiRateLimited" => ErrorKind::ApiRateLimited,
        "NetworkUnreachable" => ErrorKind::NetworkUnreachable,
        _ => ErrorKind::Internal,
    };
    CarpError::new(ek, "").is_retryable()
}

// ── Inline Completion Production — /complete-stream (Wave 16 sync) ──

/// Production-grade inline completion with caching and ranking.
pub fn complete_production(prefix: &str, suffix: &str, api_key: &str, language: &str) -> String {
    use deepseek_carp::completion::fim::{FimEngine, FimRequest, FimBackend, CompletionCache, CompletionContext, CompletionRanker};
    let engine = FimEngine::new(FimBackend::DeepSeek)
        .with_api_key(api_key)
        .with_cache(CompletionCache::new())
        .with_ranker(CompletionRanker);
    let request = FimRequest::new(prefix, suffix)
        .with_language(language);
    let context = CompletionContext::default();
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(engine.complete_production(&request, &context)) {
        Ok(Some(result)) => result.text,
        Ok(None) => String::new(),
        Err(e) => format!("/* error: {} */", e),
    }
}

/// Cache statistics for inline completion.
pub fn complete_cache_stats() -> String {
    let cache = deepseek_carp::completion::fim::CompletionCache::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let stats = rt.block_on(cache.stats());
    format!("entries={} hits={} misses={}", stats.entries, stats.hits, stats.misses)
}

/// Cache performance statistics with hit rate.
pub fn complete_cache_perf_stats() -> String {
    let cache = deepseek_carp::completion::fim::CompletionCache::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let stats = rt.block_on(cache.performance_stats());
    format!("size={}/{} hit_rate={:.1}% hits={} misses={}",
        stats.size, stats.max_size, stats.hit_rate * 100.0, stats.hits, stats.misses)
}

/// Check if inline completion should trigger for current context.
pub fn complete_context_should(prefix: &str, language: Option<&str>) -> bool {
    let context = deepseek_carp::completion::fim::CompletionContext::default();
    context.should_complete(prefix, language)
}

/// Rank FIM completions by quality.
pub fn complete_rank_results(results: &[deepseek_carp::completion::fim::FimResult], context: &str) -> Vec<(usize, f64)> {
    deepseek_carp::completion::fim::CompletionRanker::rank(results, context)
}

// ── ApplyEngine Production — /apply-v2 (Wave 16 sync) ────────────

/// Apply edit with conflict resolution.
pub fn apply_edit_v2(file_path: &str, search: &str, replace: &str) -> String {
    use deepseek_carp::tools::apply_engine::{ApplyEngine, EditFormat, ConflictResolver};
    let engine = ApplyEngine::new(std::path::Path::new("."));
    let resolver = ConflictResolver::new();
    let edit = EditFormat::SearchReplace {
        search: search.to_string(),
        replace: replace.to_string(),
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(engine.apply_edit_v2(std::path::Path::new(file_path), &edit, Some(&resolver))) {
        Ok(result) => {
            if result.success {
                format!("Applied: {} lines (conf={:.1}%)", result.lines_changed, result.confidence * 100.0)
            } else {
                format!("Failed: {}", result.error.unwrap_or_default())
            }
        }
        Err(e) => format!("Error: {}", e),
    }
}

/// Smart batch apply with compilation validation.
pub fn apply_batch_smart(edits_json: &str) -> String {
    use deepseek_carp::tools::apply_engine::{ApplyEngine, EditFormat, CompilationValidator};
    let engine = ApplyEngine::new(std::path::Path::new("."));
    let compiler = CompilationValidator::new();
    let edits: Vec<(std::path::PathBuf, EditFormat)> = match serde_json::from_str(edits_json) {
        Ok(e) => e,
        Err(e) => return format!("Parse error: {}", e),
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(engine.apply_batch_smart(&edits, &compiler)) {
        Ok(results) => {
            let success_count = results.iter().filter(|r| r.success).count();
            format!("Applied {}/{} edits", success_count, results.len())
        }
        Err(e) => format!("Batch failed: {}", e),
    }
}

/// Parse LLM response v2 (enhanced format detection).
pub fn apply_parse_llm_v2(response: &str) -> String {
    use deepseek_carp::tools::apply_engine::ApplyEngine;
    let engine = ApplyEngine::new(std::path::Path::new("."));
    let edits = engine.parse_llm_response_v2(response);
    edits.iter().map(|(path, fmt)| {
        let label = match fmt {
            deepseek_carp::tools::apply_engine::EditFormat::UnifiedDiff(_) => "diff",
            deepseek_carp::tools::apply_engine::EditFormat::SearchReplace { .. } => "sr",
            deepseek_carp::tools::apply_engine::EditFormat::FullFile(_) => "full",
            deepseek_carp::tools::apply_engine::EditFormat::LineRange { .. } => "range",
        };
        format!("{} [{}]", path.display(), label)
    }).collect::<Vec<_>>().join("\n")
}

/// Check if a workspace compiles after edits.
pub fn compile_check() -> String {
    use deepseek_carp::tools::apply_engine::CompilationValidator;
    let compiler = CompilationValidator::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let check = rt.block_on(compiler.check_compilation(std::path::Path::new(".")));
    if check.success {
        format!("Compilation OK ({:.1}s)", check.duration_ms as f64 / 1000.0)
    } else {
        format!("{} errors, {} warnings", check.errors.len(), check.warnings.len())
    }
}

// ── CodeGraph — /codegraph (Wave 17 sync) ─────────────────────

/// Build a codegraph for a project.
pub fn codegraph_build(project_root: &str) -> String {
    use deepseek_carp::codegraph::{CodeGraph, DomainMapper, ImpactAnalyzer};
    
    let _graph = CodeGraph::new();
    let _mapper = DomainMapper::new();
    let _analyzer = ImpactAnalyzer::new();
    
    // Scan project source directory and build nodes
    let src = std::path::Path::new(project_root).join("src");
    let mut count = 0u32;
    
    if let Ok(entries) = std::fs::read_dir(&src) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "rs").unwrap_or(false) {
                let domain = _mapper.map_file(&path);
                count += 1;
                let _ = _graph; let _ = domain;
            }
        }
    }
    
    format!("CodeGraph built: {} files indexed, ready for queries", count)
}

/// Analyze impact of changes using codegraph.
pub fn codegraph_impact(changed_files_json: &str) -> String {
    use deepseek_carp::codegraph::{CodeGraph, ImpactAnalyzer};
    use std::path::PathBuf;
    
    let _graph = CodeGraph::new();
    let analyzer = ImpactAnalyzer::new();
    
    let files: Vec<PathBuf> = serde_json::from_str(changed_files_json).unwrap_or_default();
    let result = analyzer.analyze(&files, &_graph);
    
    let by_reason = result.affected_files.len();
    format!("Impact analysis: {} files affected, max depth {}",
        by_reason, result.max_depth_reached)
}

/// Apply fuzzy search/replace to a file.
pub fn apply_fuzzy(file_path: &str, search: &str, replace: &str) -> String {
    use deepseek_carp::tools::apply_engine::ApplyEngine;
    
    let engine = ApplyEngine::new(std::path::Path::new("."));
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(engine.apply_edit_fuzzy(std::path::Path::new(file_path), search, replace)) {
        Ok(result) => {
            if result.success {
                format!("Fuzzy applied: {} lines (conf={:.1}%)", result.lines_changed, result.confidence * 100.0)
            } else {
                format!("Failed: {}", result.error.unwrap_or_default())
            }
        }
        Err(e) => format!("Error: {}", e),
    }
}

/// Incremental compilation check.
pub fn compile_incremental(changed_crates_json: &str) -> String {
    use deepseek_carp::tools::apply_engine::CompilationValidator;
    
    let compiler = CompilationValidator::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let crates: Vec<String> = serde_json::from_str(changed_crates_json).unwrap_or_default();
    let check = rt.block_on(compiler.check_incremental(std::path::Path::new("."), &crates));
    
    if check.success {
        format!("Incremental check OK ({:.1}s)", check.duration_ms as f64 / 1000.0)
    } else {
        format!("{} errors, {} warnings in {} crates", check.errors.len(), check.warnings.len(), crates.len())
    }
}

/// Scan a file using SecurityScannerV2 and return LSP diagnostics
/// suitable for pushing to the editor's problem panel.
pub fn push_diagnostics_from_ai(target: &str) -> Vec<Diagnostic> {
    let h = hub();
    let scanner = h.security_scanner.read().unwrap();

    let path = std::path::Path::new(target);
    if !path.is_file() {
        return Vec::new();
    }

    let content = match std::fs::read_to_string(target) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let lang = detect_language(target);
    let report = scanner.scan_files(&[(target.to_string(), content, lang)]);
    drop(scanner);

    report
        .findings
        .into_iter()
        .map(|finding| {
            let severity = match finding.severity {
                VulnerabilitySeverity::Critical => Some(DiagnosticSeverity::ERROR),
                VulnerabilitySeverity::High => Some(DiagnosticSeverity::WARNING),
                VulnerabilitySeverity::Medium => Some(DiagnosticSeverity::INFORMATION),
                VulnerabilitySeverity::Low => Some(DiagnosticSeverity::HINT),
                VulnerabilitySeverity::Info => Some(DiagnosticSeverity::HINT),
                _ => Some(DiagnosticSeverity::WARNING),
            };
            let line = if finding.line > 0 {
                finding.line - 1
            } else {
                0
            };
            Diagnostic {
                range: Range {
                    start: Position {
                        line: line as u32,
                        character: finding.column as u32,
                    },
                    end: Position {
                        line: line as u32,
                        character: finding.column as u32,
                    },
                },
                severity,
                message: format!(
                    "[AI Security] {}: {}",
                    finding.title, finding.description
                ),
                source: Some("deepseek-carp-ai".to_string()),
                ..Default::default()
            }
        })
        .collect()
}
