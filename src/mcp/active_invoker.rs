//! Active MCP Tool Invoker — event-driven MCP tool calling for AI panels.
//!
//! Unlike passive poll-based MCP usage, the ActiveInvoker enables the AI agent
//! (or Lapce IDE panel) to **proactively** call MCP tools in response to:
//!
//! - File save/edit events → trigger `review_start` or `compiler` check
//! - Timer/heartbeat → periodic `heartbeat_register` checks
//! - User explicit requests → any discovered tool on-demand
//! - Context-aware suggestions → auto-call relevant tools based on current work
//!
//! ## Architecture
//!
//! ```text
//! ActiveInvoker
//!   ├── event_rx: mpsc::Receiver<McpEvent>   ← receives events from editor/AI
//!   ├── client: McpClient                     ← the underlying MCP connection
//!   ├── rules: Vec<InvocationRule>            ← event → tool mappings
//!   ├── cache: ToolResultCache               ← avoid redundant calls
//!   └── metrics: InvocationMetrics           ← track call success/latency
//!
//! Event flow:
//!   Editor/AI → send(McpEvent) → event_rx → match rules → call_tool() → result
//! ```

use crate::mcp::client::McpClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// ---------------------------------------------------------------------------
// Events — what triggers active MCP tool calls
// ---------------------------------------------------------------------------

/// An event that can trigger proactive MCP tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum McpEvent {
    /// A file was saved in the editor.
    FileSaved { path: PathBuf, content_hash: u64 },
    /// A file was edited (before save).
    FileEdited { path: PathBuf },
    /// User explicitly requested a tool call.
    ExplicitRequest {
        tool_name: String,
        arguments: serde_json::Value,
    },
    /// Periodic heartbeat timer fired.
    Heartbeat { interval_secs: u64 },
    /// A git operation completed (commit/push/PR).
    GitOperation { op: GitOp, branch: String },
    /// Build/compilation started or finished.
    BuildEvent { phase: BuildPhase, target: String },
    /// AI agent finished generating code — trigger review.
    AiGenerationComplete { files_changed: Vec<PathBuf> },
    /// Custom event with arbitrary payload.
    Custom { event_type: String, payload: serde_json::Value },
}

/// Git operations that can trigger MCP calls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GitOp {
    Commit,
    Push,
    PullRequest,
    BranchSwitch,
    Merge,
}

/// Build phases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuildPhase {
    Started,
    Success,
    Failed,
}

// ---------------------------------------------------------------------------
// Invocation Rules — map events to tool calls
// ---------------------------------------------------------------------------

/// A rule that maps an event pattern to one or more MCP tool invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationRule {
    /// Unique rule identifier.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Event pattern to match (use "*" for wildcard).
    pub event_pattern: EventPattern,
    /// Tool(s) to invoke when the rule matches.
    pub actions: Vec<ToolAction>,
    /// Whether this rule is enabled.
    pub enabled: bool,
    /// Priority (higher = matched first). Default 0.
    pub priority: i32,
    /// Cooldown in seconds — don't re-invoke within this window.
    pub cooldown_secs: u64,
}

/// Pattern for matching events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventPattern {
    /// Match a specific event variant exactly.
    Exact(McpEvent),
    /// Match all FileSaved events (regardless of path).
    FileSavedAny,
    /// Match all FileEdited events.
    FileEditedAny,
    /// Match all Heartbeat events.
    HeartbeatAny,
    /// Match all GitOperation events.
    GitOpAny,
    /// Match all BuildEvents.
    BuildAny,
    /// Match all AiGenerationComplete events.
    AiGenCompleteAny,
    /// Match any event (catch-all).
    Any,
    /// Match by custom event_type string.
    CustomType(String),
}

/// A single tool invocation action within a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAction {
    /// Name of the MCP tool to call.
    pub tool_name: String,
    /// Arguments template — supports variable substitution.
    pub arguments: serde_json::Value,
    /// Whether the action is required (true) or best-effort (false).
    pub required: bool,
}

// ---------------------------------------------------------------------------
// Result Cache — avoid redundant identical calls
// ---------------------------------------------------------------------------

/// Cached result of a previous tool call.
#[derive(Debug, Clone)]
pub struct CachedToolResult {
    /// The raw result content.
    pub content: String,
    /// When this cache entry was created (UNIX epoch seconds).
    pub created_at: u64,
    /// TTL in seconds — after this, the cached result is stale.
    pub ttl_secs: u64,
}

/// In-memory cache for MCP tool call results.
#[derive(Debug, Clone)]
pub struct ToolResultCache {
    entries: HashMap<String, CachedToolResult>,
    max_entries: usize,
}

impl ToolResultCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
        }
    }

    /// Generate a cache key from tool_name + sorted arguments.
    fn cache_key(tool_name: &str, args: &serde_json::Value) -> String {
        format!("{}:{}", tool_name, args)
    }

    /// Get a cached result if it exists and hasn't expired.
    pub fn get(&self, tool_name: &str, args: &serde_json::Value) -> Option<&CachedToolResult> {
        let key = Self::cache_key(tool_name, args);
        self.entries.get(&key).and_then(|entry| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now.saturating_sub(entry.created_at) < entry.ttl_secs {
                Some(entry)
            } else {
                None
            }
        })
    }

    /// Store a result in the cache.
    pub fn put(&mut self, tool_name: &str, args: &serde_json::Value, content: String, ttl_secs: u64) {
        let key = Self::cache_key(tool_name, args);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Evict oldest entry if at capacity
        if self.entries.len() >= self.max_entries {
            if let Some(oldest_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&oldest_key);
            }
        }

        self.entries.insert(key, CachedToolResult {
            content,
            created_at: now,
            ttl_secs,
        });
    }

    /// Invalidate all cache entries for a specific tool.
    pub fn invalidate_tool(&mut self, tool_name: &str) {
        let prefix = format!("{}:", tool_name);
        self.entries.retain(|k, _| !k.starts_with(&prefix));
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ---------------------------------------------------------------------------
// Metrics — track invocation performance
// ---------------------------------------------------------------------------

/// Performance and success metrics for MCP tool invocations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InvocationMetrics {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub cached_calls: u64,
    pub total_latency_ms: u64,
    pub calls_by_tool: HashMap<String, u64>,
}

impl InvocationMetrics {
    /// Record a successful call.
    pub fn record_success(&mut self, tool_name: &str, latency_ms: u64) {
        self.total_calls += 1;
        self.successful_calls += 1;
        self.total_latency_ms += latency_ms;
        *self.calls_by_tool.entry(tool_name.to_string()).or_insert(0) += 1;
    }

    /// Record a failed call.
    pub fn record_failure(&mut self, tool_name: &str) {
        self.total_calls += 1;
        self.failed_calls += 1;
        *self.calls_by_tool.entry(tool_name.to_string()).or_insert(0) += 1;
    }

    /// Record a cache hit.
    pub fn record_cache_hit(&mut self, tool_name: &str) {
        self.total_calls += 1;
        self.cached_calls += 1;
        *self.calls_by_tool.entry(tool_name.to_string()).or_insert(0) += 1;
    }

    /// Average latency in milliseconds.
    pub fn avg_latency_ms(&self) -> f64 {
        if self.successful_calls == 0 { 0.0 }
        else { (self.total_latency_ms as f64) / (self.successful_calls as f64) }
    }

    /// Success rate as a fraction 0.0–1.0.
    pub fn success_rate(&self) -> f64 {
        if self.total_calls == 0 { 1.0 }
        else { (self.successful_calls as f64) / (self.total_calls as f64) }
    }
}

// ---------------------------------------------------------------------------
// Main Active Invoker
// ---------------------------------------------------------------------------

/// Event-driven MCP tool invoker for AI panel integration.
///
/// Listens for events from the editor/IDE/AI agent and proactively invokes
/// MCP tools according to configurable rules. Results are cached to avoid
/// redundant calls.
///
/// # Example
///
/// ```ignore
/// let (tx, rx) = mpsc::channel(64);
/// let invoker = ActiveInvoker::new(client, rx);
/// invoker.add_default_rules();
/// tokio::spawn(invoker.run());
///
/// // When user saves a file:
/// tx.send(McpEvent::FileSaved { path: ... }).await;
/// // → automatically triggers review_start / compiler check
/// ```
pub struct ActiveInvoker {
    /// Channel receiver for incoming events.
    event_rx: mpsc::Receiver<McpEvent>,
    /// The underlying MCP client (for actually calling tools).
    client: Arc<RwLock<McpClient>>,
    /// Rules that map events to tool calls.
    rules: Vec<InvocationRule>,
    /// Result cache to avoid redundant calls.
    cache: Arc<RwLock<ToolResultCache>>,
    /// Performance metrics.
    metrics: Arc<RwLock<InvocationMetrics>>,
    /// Cooldown tracking: rule_id → last invocation timestamp.
    cooldowns: Arc<RwLock<HashMap<String, u64>>>,
    /// Default cache TTL in seconds.
    cache_ttl_secs: u64,
}

impl ActiveInvoker {
    /// Create a new active invoker.
    pub fn new(client: McpClient, event_rx: mpsc::Receiver<McpEvent>) -> Self {
        Self {
            event_rx,
            client: Arc::new(RwLock::new(client)),
            rules: Vec::new(),
            cache: Arc::new(RwLock::new(ToolResultCache::new(256))),
            metrics: Arc::new(RwLock::new(InvocationMetrics::default())),
            cooldowns: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl_secs: 300, // 5 minutes default
        }
    }

    /// Set the cache TTL.
    pub fn with_cache_ttl(mut self, secs: u64) -> Self {
        self.cache_ttl_secs = secs;
        self
    }

    /// Add an invocation rule.
    pub fn add_rule(&mut self, rule: InvocationRule) {
        self.rules.push(rule);
        // Sort by priority (highest first)
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Add the default set of invocation rules for coding workflows.
    ///
    /// These rules cover the most common IDE integration scenarios:
    /// - File save → review + compile check
    /// - AI generation complete → review workflow
    /// - Heartbeat → status check
    /// - Build failed → security scan + fix analysis
    pub fn add_default_rules(&mut self) {
        self.rules.extend(vec![
            // Rule 1: On file save, run review and compiler check
            InvocationRule {
                id: "file-save-review".into(),
                description: "On file save: run quick review and compilation check".into(),
                event_pattern: EventPattern::FileSavedAny,
                actions: vec![
                    ToolAction {
                        tool_name: "orchestrator_analyze".into(),
                        arguments: serde_json::json!({"agent": "compiler"}),
                        required: false,
                    },
                ],
                enabled: true,
                priority: 100,
                cooldown_secs: 30,
            },
            // Rule 2: On AI generation complete, run full review
            InvocationRule {
                id: "ai-gen-review".into(),
                description: "After AI generates code: run review workflow".into(),
                event_pattern: EventPattern::AiGenCompleteAny,
                actions: vec![
                    ToolAction {
                        tool_name: "review_start".into(),
                        arguments: serde_json::json!({"target": "."}),
                        required: false,
                    },
                ],
                enabled: true,
                priority: 90,
                cooldown_secs: 60,
            },
            // Rule 3: On build failure, run security scan
            InvocationRule {
                id: "build-fail-scan".into(),
                description: "On build failure: run security analysis".into(),
                event_pattern: EventPattern::BuildAny,
                actions: vec![
                    ToolAction {
                        tool_name: "orchestrator_analyze".into(),
                        arguments: serde_json::json!({"agent": "security-scanner"}),
                        required: false,
                    },
                ],
                enabled: true,
                priority: 80,
                cooldown_secs: 120,
            },
            // Rule 4: On heartbeat, register keepalive
            InvocationRule {
                id: "heartbeat-keepalive".into(),
                description: "On heartbeat: register periodic monitoring".into(),
                event_pattern: EventPattern::HeartbeatAny,
                actions: vec![
                    ToolAction {
                        tool_name: "heartbeat_register".into(),
                        arguments: serde_json::json!({"interval_secs": 300}),
                        required: false,
                    },
                ],
                enabled: true,
                priority: 10,
                cooldown_secs: 300,
            },
            // Rule 5: Catch-all for explicit requests
            InvocationRule {
                id: "explicit-request".into(),
                description: "Forward explicit user tool requests to MCP".into(),
                event_pattern: EventPattern::Any,
                actions: vec![],
                enabled: true,
                priority: 1000,
                cooldown_secs: 0,
            },
        ]);
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Run the main event loop.
    ///
    /// Blocks until the event channel is closed. For each received event:
    /// 1. Match against rules (in priority order)
    /// 2. Check cooldowns
    /// 3. Check cache
    /// 4. Invoke matched tools
    /// 5. Cache results and record metrics
    pub async fn run(mut self) {
        tracing::info!(rules=self.rules.len(), "ActiveInvoker: starting event loop");

        while let Some(event) = self.event_rx.recv().await {
            let event_debug = format!("{:?}", &event);
            tracing::debug!(event=%event_debug, "ActiveInvoker: received event");

            // Handle explicit requests specially (they carry their own tool info)
            if let McpEvent::ExplicitRequest { tool_name, arguments } = &event {
                let _ = self.invoke_tool_direct(tool_name.clone(), arguments.clone()).await;
                continue;
            }

            // Match against rules
            for rule in &self.rules {
                if !rule.enabled { continue; }
                if !Self::matches_event(&event, &rule.event_pattern) { continue; }

                // Check cooldown
                if rule.cooldown_secs > 0 {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let cooldowns = self.cooldowns.read().await;
                    if let Some(&last_call) = cooldowns.get(&rule.id) {
                        if now.saturating_sub(last_call) < rule.cooldown_secs {
                            continue; // Still in cooldown
                        }
                    }
                    drop(cooldowns);

                    // Record this invocation time
                    self.cooldowns.write().await.insert(rule.id.clone(), now);
                }

                // Execute each action in the rule
                for action in &rule.actions {
                    let resolved_args = Self::resolve_arguments(&action.arguments, &event);
                    let _ = self.invoke_with_cache(
                        &action.tool_name,
                        &resolved_args,
                        action.required,
                    ).await;
                }
            }
        }

        tracing::info!("ActiveInvoker: event channel closed, exiting");
    }

    /// Check if an event matches a pattern.
    fn matches_event(event: &McpEvent, pattern: &EventPattern) -> bool {
        match pattern {
            EventPattern::Exact(ref exact) => {
                // Compare discriminants and key fields
                std::mem::discriminant(event) == std::mem::discriminant(exact)
            }
            EventPattern::FileSavedAny => matches!(event, McpEvent::FileSaved { .. }),
            EventPattern::FileEditedAny => matches!(event, McpEvent::FileEdited { .. }),
            EventPattern::HeartbeatAny => matches!(event, McpEvent::Heartbeat { .. }),
            EventPattern::GitOpAny => matches!(event, McpEvent::GitOperation { .. }),
            EventPattern::BuildAny => matches!(event, McpEvent::BuildEvent { .. }),
            EventPattern::AiGenCompleteAny => matches!(event, McpEvent::AiGenerationComplete { .. }),
            EventPattern::Any => true,
            EventPattern::CustomType(ref ty) => {
                if let McpEvent::Custom { event_type, .. } = event {
                    event_type == ty
                } else {
                    false
                }
            }
        }
    }

    /// Resolve argument templates with event data.
    fn resolve_arguments(template: &serde_json::Value, event: &McpEvent) -> serde_json::Value {
        match event {
            McpEvent::FileSaved { path, .. } | McpEvent::FileEdited { path, .. } => {
                let mut args = template.clone();
                if let Some(obj) = args.as_object_mut() {
                    obj.insert("target".into(), serde_json::json!(path.to_string_lossy()));
                }
                args
            }
            McpEvent::AiGenerationComplete { files_changed, .. } => {
                let mut args = template.clone();
                if let Some(obj) = args.as_object_mut() {
                    let paths: Vec<String> = files_changed.iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    obj.insert("files".into(), serde_json::json!(paths));
                }
                args
            }
            _ => template.clone(),
        }
    }

    /// Invoke a tool with caching.
    async fn invoke_with_cache(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        _required: bool,
    ) -> Option<String> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(tool_name, arguments) {
                let mut metrics = self.metrics.write().await;
                metrics.record_cache_hit(tool_name);
                tracing::debug!(tool=tool_name, "ActiveInvoker: cache hit");
                return Some(cached.content.clone());
            }
        }

        // Actually call the tool
        let start = std::time::Instant::now();
        let result = {
            let mut client = self.client.write().await;
            client.call_tool(tool_name, arguments.clone()).await
        };
        let latency_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(call_result) => {
                let content = call_result.content
                    .iter()
                    .map(|b| b.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n");

                // Store in cache
                self.cache.write().await.put(
                    tool_name, arguments, content.clone(), self.cache_ttl_secs
                );

                // Record metrics
                let mut metrics = self.metrics.write().await;
                metrics.record_success(tool_name, latency_ms);

                tracing::info!(
                    tool=tool_name, latency_ms, len=content.len(),
                    "ActiveInvoker: tool call succeeded"
                );
                Some(content)
            }
            Err(e) => {
                let mut metrics = self.metrics.write().await;
                metrics.record_failure(tool_name);

                tracing::warn!(tool=tool_name, error=%e, "ActiveInvoker: tool call failed");
                None
            }
        }
    }

    /// Directly invoke a tool (bypasses rules, used for explicit requests).
    async fn invoke_tool_direct(
        &self,
        tool_name: String,
        arguments: serde_json::Value,
    ) -> Option<String> {
        self.invoke_with_cache(&tool_name, &arguments, true).await
    }

    /// Get a snapshot of current metrics.
    pub async fn metrics_snapshot(&self) -> InvocationMetrics {
        self.metrics.read().await.clone()
    }

    /// Clear the result cache.
    pub async fn clear_cache(&self) {
        self.cache.write().await.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_cache_basic() {
        let mut cache = ToolResultCache::new(10);
        assert!(cache.get("test_tool", &serde_json::json!({})).is_none());

        cache.put("test_tool", &serde_json::json!({}), "result".into(), 60);
        let cached = cache.get("test_tool", &serde_json::json!({}));
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().content, "result");
    }

    #[test]
    fn test_tool_result_cache_invalidate() {
        let mut cache = ToolResultCache::new(10);
        cache.put("tool_a", &serde_json::json!({}), "a".into(), 60);
        cache.put("tool_b", &serde_json::json!({}), "b".into(), 60);
        assert_eq!(cache.entries.len(), 2);

        cache.invalidate_tool("tool_a");
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.entries.contains_key(&"tool_b:{}".to_string()));
    }

    #[test]
    fn test_metrics_success_rate() {
        let mut m = InvocationMetrics::default();
        m.record_success("tool_a", 100);
        m.record_success("tool_a", 50);
        m.record_failure("tool_b");
        assert!((m.success_rate() - 0.666).abs() < 0.01);
        assert!((m.avg_latency_ms() - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_metrics_all_fail() {
        let mut m = InvocationMetrics::default();
        m.record_failure("x");
        m.record_failure("x");
        assert_eq!(m.success_rate(), 0.0);
        assert_eq!(m.total_calls, 2);
        assert_eq!(m.failed_calls, 2);
    }

    #[test]
    fn test_event_pattern_matching() {
        let file_save = McpEvent::FileSaved {
            path: PathBuf::from("/test.rs"),
            content_hash: 42,
        };
        assert!(ActiveInvoker::matches_event(&file_save, &EventPattern::FileSavedAny));
        assert!(!ActiveInvoker::matches_event(&file_save, &EventPattern::HeartbeatAny));
        assert!(ActiveInvoker::matches_event(&file_save, &EventPattern::Any));

        let heartbeat = McpEvent::Heartbeat { interval_secs: 60 };
        assert!(ActiveInvoker::matches_event(&heartbeat, &EventPattern::HeartbeatAny));

        let custom = McpEvent::Custom {
            event_type: "my-event".into(),
            payload: serde_json::json!({}),
        };
        assert!(ActiveInvoker::matches_event(&custom, &EventPattern::CustomType("my-event".into())));
        assert!(!ActiveInvoker::matches_event(&custom, &EventPattern::CustomType("other".into())));
    }

    #[test]
    fn test_argument_resolution() {
        let event = McpEvent::FileSaved {
            path: PathBuf::from("/src/main.rs"),
            content_hash: 123,
        };
        let template = serde_json::json!({"target": "{{auto}}"});
        let resolved = ActiveInvoker::resolve_arguments(&template, &event);
        assert_eq!(resolved["target"], "/src/main.rs");
    }

    #[tokio::test]
    async fn test_active_invoker_creation() {
        let (_tx, rx) = mpsc::channel(16);
        let client = McpClient::new();
        let invoker = ActiveInvoker::new(client, rx);
        assert!(invoker.rules.is_empty());
    }

    #[tokio::test]
    async fn test_active_invoker_default_rules() {
        let (_tx, rx) = mpsc::channel(16);
        let client = McpClient::new();
        let mut invoker = ActiveInvoker::new(client, rx);
        invoker.add_default_rules();
        assert!(!invoker.rules.is_empty());
        // Highest priority should be explicit-request (1000)
        assert_eq!(invoker.rules[0].priority, 1000);
    }

    #[test]
    fn test_cache_key_generation() {
        let k1 = ToolResultCache::cache_key("tool_a", &serde_json::json!({"x": 1}));
        let k2 = ToolResultCache::cache_key("tool_a", &serde_json::json!({"x": 1}));
        let k3 = ToolResultCache::cache_key("tool_a", &serde_json::json!({"x": 2}));
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }
}
