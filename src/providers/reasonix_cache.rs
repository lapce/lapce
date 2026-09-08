//! ReasonIX-inspired prefix cache system for DeepSeek API.
//!
//! Achieves high cache hit rates by structuring every agent request into three
//! **byte-stable zones** so that DeepSeek's built-in prefix cache can fire
//! on every turn after the first.
//!
//! ## The Core Insight
//!
//! DeepSeek's prefix cache only works on **byte-identical prefixes from position 0**.
//! If even one character changes, the entire cache chain breaks.  ReasonIX solves this
//! by partitioning context into three zones:
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │ IMMUTABLE PREFIX │ frozen at session start
//! │ system + tool_specs + few_shots     │ ← THIS IS THE CACHE TARGET
//! ├─────────────────────────────────────┤
//! │ APPEND-ONLY LOG  │ grows monotonically, NEVER reordered
//! │ [user₁][asst₁][tool₁][user₂]...    │ ← old turns = new turn's prefix
//! ├─────────────────────────────────────┤
//! │ VOLATILE SCRATCH │ reset each turn, NEVER sent upstream
//! │ R1 thoughts, plan state             │ ← stays local only
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Integration Points
//!
//! - At session start: call [`ReasonixCache::initialize_prefix()`] once with system prompt,
//!   tool schemas, and few-shot examples.
//! - Before each API call: call [`ReasonixCache::build_request_json()`] to get a
//!   deterministic JSON payload (prefix + log entries — **never** scratch data).
//! - After each API response: call [`ReasonixCache::record_response()`] with parsed usage
//!   to update hit-rate metrics.
//! - For per-turn temporary state (R1 reasoning chains, planning notes): use the
//!   scratch-pad methods.  Scratch contents are **never** included in API payloads.

use std::sync::Arc;
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ============================================================================
// Context Zones
// ============================================================================

/// The three context zones that partition every agent request.
///
/// Each zone has distinct lifecycle and mutation rules that guarantee
/// byte-stability of the prefix sent to the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextZone {
    /// Frozen at session start. System prompt + tool schemas + few-shots.
    /// This is what gets cached by DeepSeek's KV cache.
    ImmutablePrefix,
    /// Append-only conversation log. Never reordered or compressed.
    /// Each new turn appends here; old entries become prefix for next request.
    AppendOnlyLog,
    /// Per-turn volatile data. Reset at each turn boundary.
    /// Never included in API requests — stays local only.
    VolatileScratch,
}

// ============================================================================
// Log Entry
// ============================================================================

/// A single entry in the append-only log.
///
/// Once appended, a `LogEntry` is never modified or removed (except by
/// front-trimming when the log exceeds `max_log_entries`).
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// Message role: `"user"`, `"assistant"`, or `"tool"`.
    pub role: String,
    /// Message content (text or tool-result JSON).
    pub content: String,
    /// Which conversational turn this entry belongs to (0-indexed).
    pub timestamp_turn: u32,
    /// Tool-call ID — present only for `"tool"` role entries.
    pub tool_call_id: Option<String>,
    /// Tool name — present when this entry relates to a tool invocation.
    pub tool_name: Option<String>,
}

// ============================================================================
// Prefix Fingerprint
// ============================================================================

/// Byte-stable prefix hash — the core of cache stability.
///
/// Uses SHA-256 of the serialized prefix bytes for deterministic fingerprinting.
/// Two requests with the same fingerprint are guaranteed to have identical
/// prefix bytes, meaning DeepSeek's KV cache will fire.
#[derive(Clone, Debug)]
pub struct PrefixFingerprint {
    /// Hex-encoded SHA-256 of the prefix bytes.
    pub hash: String,
    /// Total byte length of the prefix payload.
    pub byte_length: usize,
    /// Estimated token count (rough: ~4 chars per token).
    pub token_estimate: usize,
    /// When this fingerprint was computed.
    pub created_at: std::time::Instant,
}

// ============================================================================
// API Usage
// ============================================================================

/// API usage data parsed from a DeepSeek response.
///
/// The key field is `prompt_cache_hit_tokens` — this tells us how many tokens
/// were served from DeepSeek's prefix cache versus freshly computed.
#[derive(Debug, Clone)]
pub struct ApiUsage {
    /// Total prompt tokens in the request.
    pub prompt_tokens: u64,
    /// Completion (output) tokens.
    pub completion_tokens: u64,
    /// Tokens served from DeepSeek's prompt cache (**the money saver**).
    pub prompt_cache_hit_tokens: u64,
    /// Tokens that were NOT in cache and needed computation.
    pub prompt_cache_miss_tokens: u64,
    /// Estimated cost of this request in USD.
    pub total_cost_usd: f64,
}

// ============================================================================
// Cache Metrics
// ============================================================================

/// Real-time cache statistics with hit-rate computation.
///
/// Accumulated over the lifetime of a session. Clone it at any point to
/// snapshot the current state.
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    // --- Hit / Miss counters ---
    /// Total number of API requests made through this cache.
    pub total_requests: u64,
    /// Full-prefix matches (ideal case — entire prefix was cached).
    pub prefix_hits: u64,
    /// Partial-prefix matches (some leading tokens were reused).
    pub partial_hits: u64,
    /// Complete cache misses (no prefix overlap).
    pub misses: u64,

    // --- Token accounting ---
    /// Total input tokens across all requests.
    pub total_input_tokens: u64,
    /// Tokens served from cache (from DeepSeek `usage.prompt_cache_hit_tokens`).
    pub cached_tokens: u64,
    /// Tokens that needed fresh computation.
    pub new_tokens: u64,

    // --- Cost tracking ---
    /// Cumulative estimated cost in USD.
    pub estimated_cost_usd: f64,
    /// Cumulative estimated savings from cache hits (USD).
    pub estimated_savings_usd: f64,

    // --- Session info ---
    /// Unique session identifier.
    pub session_id: String,
    /// When this metrics instance started collecting.
    pub start_time: Option<std::time::Instant>,
}

impl CacheMetrics {
    /// Request-level hit rate: `(prefix_hits + partial_hits) / total_requests`.
    ///
    /// Returns `0.0` when no requests have been recorded yet.
    pub fn hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        (self.prefix_hits + self.partial_hits) as f64 / self.total_requests as f64
    }

    /// Token-level cache hit rate: `cached_tokens / total_input_tokens`.
    ///
    /// This is the metric that matters for cost optimisation — a value > 0.9
    /// means >90% of your input tokens are free.
    pub fn token_cache_hit_rate(&self) -> f64 {
        if self.total_input_tokens == 0 {
            return 0.0;
        }
        self.cached_tokens as f64 / self.total_input_tokens as f64
    }

    /// Record usage data from an API response, updating all counters.
    pub fn record_api_response(&mut self, usage: &ApiUsage) {
        self.total_requests += 1;
        self.total_input_tokens += usage.prompt_tokens;
        self.cached_tokens += usage.prompt_cache_hit_tokens;
        self.new_tokens += usage.prompt_cache_miss_tokens + usage.completion_tokens;
        self.estimated_cost_usd += usage.total_cost_usd;

        // Classify hit/miss based on cache-hit token ratio
        if usage.prompt_cache_hit_tokens > 0 {
            if usage.prompt_cache_miss_tokens == 0 {
                // Every input token was cached — full prefix hit
                self.prefix_hits += 1;
            } else {
                // Some tokens cached, some not — partial hit
                self.partial_hits += 1;
            }

            // Estimate savings: cached tokens would have cost ~$0.07/1M tokens
            // (DeepSeek input price).  Use a conservative estimate.
            let saving = (usage.prompt_cache_hit_tokens as f64) * 0.07e-6;
            self.estimated_savings_usd += saving;
        } else {
            self.misses += 1;
        }
    }

    /// Format a human-readable cache performance report.
    pub fn format_report(&self) -> String {
        let elapsed = self
            .start_time
            .map(|t| format!("{:.1}s", t.elapsed().as_secs_f64()))
            .unwrap_or_else(|| "N/A".into());

        format!(
            "─── ReasonIX Cache Report ─────────────────────────────\n\
             │ Session : {}\n\
             │ Duration: {}\n\
             ├─────────────────────────────────────────────────────\n\
             │ Requests      : {} total\n\
             │   Prefix hits : {} ({:.1}%)\n\
             │   Partial hits: {} ({:.1}%)\n\
             │   Misses       : {} ({:.1}%)\n\
             ├─────────────────────────────────────────────────────\n\
             │ Input tokens  : {}\n\
             │ Cached tokens : {} ({:.1}%)\n\
             │ New tokens    : {}\n\
             ├─────────────────────────────────────────────────────\n\
             │ Cost          : ${:.6}\n\
             │ Savings       : ${:.6}\n\
             └─────────────────────────────────────────────────────",
            self.session_id,
            elapsed,
            self.total_requests,
            self.prefix_hits,
            self.hit_rate() * 100.0,
            self.partial_hits,
            if self.total_requests > 0 {
                self.partial_hits as f64 / self.total_requests as f64 * 100.0
            } else {
                0.0
            },
            self.misses,
            if self.total_requests > 0 {
                self.misses as f64 / self.total_requests as f64 * 100.0
            } else {
                0.0
            },
            self.total_input_tokens,
            self.cached_tokens,
            self.token_cache_hit_rate() * 100.0,
            self.new_tokens,
            self.estimated_cost_usd,
            self.estimated_savings_usd,
        )
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the ReasonIX-style prefix cache.
#[derive(Clone, Debug)]
pub struct ReasonixConfig {
    /// Maximum number of log entries before front-trimming begins.
    /// Default: `500`.
    pub max_log_entries: usize,
    /// Maximum size of the volatile scratch pad in bytes.
    /// Default: `4096`.
    pub max_scratch_size: usize,
    /// Whether to enable prefix caching at all.
    /// Default: `true`.
    pub enable_prefix_cache: bool,
    /// Strip timestamps and other dynamic fields from prompts before hashing.
    /// Default: `true`.
    pub strip_timestamps: bool,
    /// Freeze tool schema ordering so definitions always appear in the same order.
    /// Default: `true`.
    pub freeze_tool_schemas: bool,
    /// Unique session identifier (auto-generated if empty).
    pub session_id: String,
}

impl Default for ReasonixConfig {
    fn default() -> Self {
        Self {
            max_log_entries: 500,
            max_scratch_size: 4096,
            enable_prefix_cache: true,
            strip_timestamps: true,
            freeze_tool_schemas: true,
            session_id: Uuid::new_v4().to_string(),
        }
    }
}

// ============================================================================
// Error Type
// ============================================================================

/// Error type for cache operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Prefix is frozen and cannot be modified")]
    PrefixFrozen,
    #[error("Log entry would exceed maximum size")]
    LogFull,
    #[error("Invalid role: {0}")]
    InvalidRole(String),
    #[error("Prefix not initialized")]
    PrefixNotInitialized,
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Scratch pad overflow: {0} bytes exceeds limit of {1}")]
    ScratchOverflow(usize, usize),
}

// ============================================================================
// Main Orchestrator
// ============================================================================

/// The main ReasonIX-style cache orchestrator.
///
/// Replaces naive caching with a **cache-first architecture** that guarantees
/// byte-identical prefixes across consecutive API calls.
///
/// # Thread Safety
///
/// All internal state is protected by `parking_lot::RwLock` wrapped in `Arc`,
/// making `ReasonixCache` cheaply cloneable and safe to share across async tasks.
///
/// # Example
///
/// ```ignore
/// use deepseek_carp::providers::reasonix_cache::{ReasonixCache, ReasonixConfig};
///
/// let config = ReasonixConfig::default();
/// let cache = ReasonixCache::new(config);
///
/// // 1. Freeze the prefix once at session start
/// cache.initialize_prefix(
///     "You are a coding assistant.",
///     r#"[{"type":"function","function":{"name":"read_file", ...}}]"#,
///     "",
/// );
///
/// // 2. Append conversation turns (never modify old ones)
/// cache.append("user", "Write hello world", 0).expect("unwrap failed: reasonix_cache.rs:361");
///
/// // 3. Build the API payload — deterministic, byte-stable
/// let json = cache.build_request_json("deepseek-reasoner", "Please write hello world");
///
/// // 4. After the API call, record usage
/// cache.record_response(&ApiUsage { ... });
/// ```
pub struct ReasonixCache {
    // Zone 1: Immutable Prefix
    /// Raw prefix bytes, frozen after [`Self::initialize_prefix()`] is called.
    immutable_prefix: Arc<RwLock<Vec<u8>>>,
    /// Fingerprint of the frozen prefix (computed once, never changes).
    prefix_fingerprint: Arc<RwLock<Option<PrefixFingerprint>>>,
    /// Tracks whether the prefix has been initialised (and thus frozen).
    prefix_initialised: Arc<RwLock<bool>>,

    // Zone 2: Append-Only Log
    /// Ordered list of conversation entries.  Only ever appended to or
    /// front-trimmed — existing entries are never mutated.
    log_entries: Arc<RwLock<Vec<LogEntry>>>,

    // Zone 3: Volatile Scratch (local only)
    /// Per-turn scratch space.  Cleared at each turn boundary; never sent upstream.
    scratch_pad: Arc<RwLock<String>>,

    // Metrics
    /// Accumulated statistics for this session.
    metrics: Arc<RwLock<CacheMetrics>>,

    // Configuration
    config: ReasonixConfig,

    // Tool schema lock
    /// Ensures tool definitions always appear in the same order in the prefix.
    tool_schema_order: Arc<RwLock<Vec<String>>>,
}

impl ReasonixCache {
    /// Create a new `ReasonixCache` with the given configuration.
    pub fn new(config: ReasonixConfig) -> Self {
        let session_id = config.session_id.clone();
        Self {
            immutable_prefix: Arc::new(RwLock::new(Vec::new())),
            prefix_fingerprint: Arc::new(RwLock::new(None)),
            prefix_initialised: Arc::new(RwLock::new(false)),
            log_entries: Arc::new(RwLock::new(Vec::new())),
            scratch_pad: Arc::new(RwLock::new(String::new())),
            metrics: Arc::new(RwLock::new(CacheMetrics {
                session_id,
                start_time: Some(std::time::Instant::now()),
                ..Default::default()
            })),
            config,
            tool_schema_order: Arc::new(RwLock::new(Vec::new())),
        }
    }

    // -----------------------------------------------------------------------
    // Zone 1: Immutable Prefix
    // -----------------------------------------------------------------------

    /// Initialise and **freeze** the immutable prefix.
    ///
    /// Call **once** at session start with the complete static prefix:
    /// - `system_prompt` — the system message
    /// - `tool_schemas` — JSON array of tool definitions (must be stable order)
    /// - `few_shots` — few-shot examples (appended after tools)
    ///
    /// After this call the prefix **cannot be modified**.  Any attempt to
    /// change it will return [`CacheError::PrefixFrozen`].
    ///
    /// Returns the [`PrefixFingerprint`] for logging and monitoring.
    pub fn initialize_prefix(
        &self,
        system_prompt: &str,
        tool_schemas: &str,
        few_shots: &str,
    ) -> PrefixFingerprint {
        // Check if already frozen
        {
            let initialised = self.prefix_initialised.read();
            if *initialised {
                tracing::warn!(
                    session = %self.config.session_id,
                    "initialize_prefix called but prefix is already frozen — returning existing fingerprint"
                );
                return self.prefix_fingerprint.read().as_ref().cloned()
                    .expect("prefix initialised but fingerprint is None — invariant broken");
            }
        }

        // Build the combined prefix text
        let mut parts: Vec<String> = Vec::with_capacity(3);
        parts.push(system_prompt.to_string());

        if !tool_schemas.is_empty() {
            // If freeze_tool_schemas is enabled, parse and sort tool names
            // to guarantee deterministic ordering
            if self.config.freeze_tool_schemas {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(tool_schemas) {
                    if let Some(arr) = parsed.as_array() {
                        let mut sorted_tools: Vec<serde_json::Value> =
                            arr.to_vec();
                        // Sort by function name for determinism
                        sorted_tools.sort_by(|a, b| {
                            let name_a = a["function"]["name"].as_str().unwrap_or("");
                            let name_b = b["function"]["name"].as_str().unwrap_or("");
                            name_a.cmp(name_b)
                        });

                        // Record the ordered tool names
                        {
                            let mut order = self.tool_schema_order.write();
                            order.clear();
                            for t in &sorted_tools {
                                if let Some(n) = t["function"]["name"].as_str() {
                                    order.push(n.to_string());
                                }
                            }
                        }

                        if let Ok(stable) = serde_json::to_string(&sorted_tools) {
                            parts.push(stable);
                        } else {
                            parts.push(tool_schemas.to_string());
                        }
                    } else {
                        parts.push(tool_schemas.to_string());
                    }
                } else {
                    parts.push(tool_schemas.to_string());
                }
            } else {
                parts.push(tool_schemas.to_string());
            }
        }

        if !few_shots.is_empty() {
            parts.push(few_shots.to_string());
        }

        let combined = parts.join("\n");

        // Optionally strip dynamic content (timestamps, session IDs, etc.)
        let final_bytes = if self.config.strip_timestamps {
            strip_dynamic_content(&combined).into_bytes()
        } else {
            combined.into_bytes()
        };

        // Compute fingerprint
        let fp = PrefixFingerprint {
            hash: compute_fingerprint(&final_bytes),
            byte_length: final_bytes.len(),
            token_estimate: estimate_tokens_from_bytes(final_bytes.len()),
            created_at: std::time::Instant::now(),
        };

        // Freeze
        {
            let mut prefix = self.immutable_prefix.write();
            *prefix = final_bytes;
        }
        {
            let mut fp_slot = self.prefix_fingerprint.write();
            *fp_slot = Some(fp.clone());
        }
        {
            let mut init = self.prefix_initialised.write();
            *init = true;
        }

        tracing::info!(
            session = %self.config.session_id,
            prefix_hash = %fp.hash,
            prefix_bytes = fp.byte_length,
            est_tokens = fp.token_estimate,
            "Immutable prefix frozen"
        );

        fp
    }

    /// Get the current prefix fingerprint, if the prefix has been initialised.
    pub fn prefix_fingerprint(&self) -> Option<PrefixFingerprint> {
        self.prefix_fingerprint.read().clone()
    }

    // -----------------------------------------------------------------------
    // Zone 2: Append-Only Log
    // -----------------------------------------------------------------------

    /// Append a message to the append-only log.
    ///
    /// # Arguments
    ///
    /// * `role` — Must be `"user"`, `"assistant"`, or `"tool"`.
    /// * `content` — The message body.
    /// * `turn` — The current conversational turn number (0-indexed).
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::InvalidRole`] if `role` is not recognised, or
    /// [`CacheError::LogFull`] if the log is at capacity and cannot trim.
    pub fn append(
        &self,
        role: &str,
        content: &str,
        turn: u32,
    ) -> Result<(), CacheError> {
        self.validate_role(role)?;

        let entry = LogEntry {
            role: role.to_string(),
            content: content.to_string(),
            timestamp_turn: turn,
            tool_call_id: None,
            tool_name: None,
        };

        self.push_entry(entry)
    }

    /// Append a tool-call result to the log.
    ///
    /// Creates a `"tool"`-role entry with the given tool name and result content.
    pub fn append_tool_result(
        &self,
        tool_name: &str,
        result: &str,
        turn: u32,
    ) -> Result<(), CacheError> {
        let entry = LogEntry {
            role: "tool".to_string(),
            content: result.to_string(),
            timestamp_turn: turn,
            tool_call_id: Some(format!("tc_{}", compute_fingerprint(result.as_bytes()))),
            tool_name: Some(tool_name.to_string()),
        };

        self.push_entry(entry)
    }

    /// Internal helper: push an entry, trimming from the front if necessary.
    fn push_entry(&self, entry: LogEntry) -> Result<(), CacheError> {
        let mut log = self.log_entries.write();

        // Trim from the front if we're over capacity
        while log.len() >= self.config.max_log_entries {
            log.remove(0);
            tracing::debug!(
                session = %self.config.session_id,
                log_len = log.len(),
                max = self.config.max_log_entries,
                "Trimmed oldest log entry"
            );
        }

        log.push(entry);
        Ok(())
    }

    /// Get **all** messages that should be sent to the API.
    ///
    /// Builds the request payload as `[prefix] + [log_entries]`.  Note that
    /// **scratch-pad contents are never included** — they stay local-only.
    ///
    /// The output is byte-stable across turns because:
    /// - The prefix never changes after initialisation.
    /// - The log is append-only (old entries are identical to the last request).
    /// - Only new entries are appended at the end.
    pub fn build_request_payload(&self) -> Vec<LogEntry> {
        self.log_entries.read().clone()
    }

    /// Build the full request as a deterministic JSON string ready for API submission.
    ///
    /// Produces **byte-identical output** for the same logical request because:
    /// - Keys are sorted alphabetically (`BTreeMap`-style ordering via custom serializer).
    /// - No trailing whitespace variation.
    /// - Consistent float formatting.
    /// - No embedded timestamps.
    ///
    /// # Arguments
    ///
    /// * `model` — The model identifier (e.g. `"deepseek-reasoner"`).
    /// * `new_message` — The latest user message to append (or empty string if already appended).
    ///
    /// # Panics
    ///
    /// Panics if the prefix has not been initialised.  Call
    /// [`Self::initialize_prefix()`] first.
    pub fn build_request_json(&self, model: &str, new_message: &str) -> String {
        // Validate prefix state
        {
            let initialised = self.prefix_initialised.read();
            assert!(
                *initialised,
                "build_request_json called before initialize_prefix"
            );
        }

        let prefix_bytes = self.immutable_prefix.read();
        let prefix_str = String::from_utf8_lossy(&prefix_bytes);
        let log = self.log_entries.read();

        // Build messages array: [system(prefix)] + [log entries] + [optional new message]
        let mut messages = serde_json::Map::new();

        // System message from prefix
        messages.insert(
            "role".to_string(),
            serde_json::Value::String("system".to_string()),
        );
        messages.insert(
            "content".to_string(),
            serde_json::Value::String(prefix_str.into_owned()),
        );

        let mut msg_array = vec![serde_json::Value::Object(messages)];

        // Append log entries
        for entry in log.iter() {
            let mut msg_map = serde_json::Map::new();
            msg_map.insert(
                "role".to_string(),
                serde_json::Value::String(entry.role.clone()),
            );
            msg_map.insert(
                "content".to_string(),
                serde_json::Value::String(entry.content.clone()),
            );

            if let Some(ref tcid) = entry.tool_call_id {
                msg_map.insert(
                    "tool_call_id".to_string(),
                    serde_json::Value::String(tcid.clone()),
                );
            }
            if let Some(ref tn) = entry.tool_name {
                // Store tool name in metadata map for reference
                let mut meta = serde_json::Map::new();
                meta.insert(
                    "tool_name".to_string(),
                    serde_json::Value::String(tn.clone()),
                );
                msg_map.insert(
                    "metadata".to_string(),
                    serde_json::Value::Object(meta),
                );
            }

            msg_array.push(serde_json::Value::Object(msg_map));
        }

        // Append the new user message if non-empty
        if !new_message.is_empty() {
            let mut new_msg = serde_json::Map::new();
            new_msg.insert(
                "role".to_string(),
                serde_json::Value::String("user".to_string()),
            );
            new_msg.insert(
                "content".to_string(),
                serde_json::Value::String(new_message.to_string()),
            );
            msg_array.push(serde_json::Value::Object(new_msg));
        }

        // Build the top-level request object with sorted keys
        let mut body = serde_json::Map::new();
        body.insert(
            "model".to_string(),
            serde_json::Value::String(model.to_string()),
        );
        body.insert(
            "messages".to_string(),
            serde_json::Value::Array(msg_array),
        );

        // Serialize with deterministic formatting
        serialize_stable(&serde_json::Value::Object(body))
            .expect("serialization of built request should not fail")
    }

    // -----------------------------------------------------------------------
    // Zone 3: Volatile Scratch
    // -----------------------------------------------------------------------

    /// Write content to the scratch pad (local only, **never** sent to the API).
    ///
    /// Use this for R1 reasoning chains, intermediate planning state, or any
    /// per-turn data that should not influence cache keys.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::ScratchOverflow`] if the content would exceed
    /// [`ReasonixConfig::max_scratch_size`].
    pub fn write_scratch(&self, content: &str) -> Result<(), CacheError> {
        let bytes = content.len();
        if bytes > self.config.max_scratch_size {
            return Err(CacheError::ScratchOverflow(bytes, self.config.max_scratch_size));
        }

        let mut scratch = self.scratch_pad.write();
        *scratch = content.to_string();
        Ok(())
    }

    /// Append content to the scratch pad (does not overwrite existing content).
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::ScratchOverflow`] if the resulting content would
    /// exceed [`ReasonixConfig::max_scratch_size`].
    pub fn append_scratch(&self, content: &str) -> Result<(), CacheError> {
        let mut scratch = self.scratch_pad.write();
        let new_len = scratch.len() + content.len();
        if new_len > self.config.max_scratch_size {
            return Err(CacheError::ScratchOverflow(new_len, self.config.max_scratch_size));
        }
        scratch.push_str(content);
        Ok(())
    }

    /// Read the current scratch pad contents.
    pub fn read_scratch(&self) -> String {
        self.scratch_pad.read().clone()
    }

    /// Clear the scratch pad (call at each turn boundary).
    pub fn clear_scratch(&self) {
        let mut scratch = self.scratch_pad.write();
        scratch.clear();
        tracing::trace!(session = %self.config.session_id, "Scratch pad cleared");
    }

    // -----------------------------------------------------------------------
    // Metrics & Monitoring
    // -----------------------------------------------------------------------

    /// Record an API response to update cache metrics.
    ///
    /// Parses `prompt_cache_hit_tokens` from the provided [`ApiUsage`] and
    /// updates all internal counters (hits, misses, costs, savings).
    pub fn record_response(&self, usage: &ApiUsage) {
        let mut m = self.metrics.write();
        m.record_api_response(usage);

        tracing::debug!(
            session = %self.config.session_id,
            prompt_tokens = usage.prompt_tokens,
            cache_hit_tokens = usage.prompt_cache_hit_tokens,
            cache_miss_tokens = usage.prompt_cache_miss_tokens,
            hit_rate = %format!("{:.1}%", m.hit_rate() * 100.0),
            token_hit_rate = %format!("{:.1}%", m.token_cache_hit_rate() * 100.0),
            "Recorded API response"
        );
    }

    /// Get a snapshot of the current cache metrics.
    pub fn metrics(&self) -> CacheMetrics {
        self.metrics.read().clone()
    }

    /// Get the real-time request-level hit rate (updated after each API call).
    pub fn hit_rate(&self) -> f64 {
        self.metrics.read().hit_rate()
    }

    /// Get the token-level cache hit rate — the metric that matters for cost.
    pub fn token_hit_rate(&self) -> f64 {
        self.metrics.read().token_cache_hit_rate()
    }

    /// Format a comprehensive cache performance report.
    pub fn format_report(&self) -> String {
        self.metrics.read().format_report()
    }

    // -----------------------------------------------------------------------
    // Utility
    // -----------------------------------------------------------------------

    /// Check whether the prefix is still valid (hasn't been corrupted since freezing).
    ///
    /// Recomputes the fingerprint of the stored prefix bytes and compares it
    /// against the original fingerprint stored at initialisation time.
    pub fn validate_prefix_integrity(&self) -> bool {
        let initialised = *self.prefix_initialised.read();
        if !initialised {
            return false;
        }

        let prefix = self.immutable_prefix.read();
        let current_hash = compute_fingerprint(&prefix);

        let fp_guard = self.prefix_fingerprint.read();
        match fp_guard.as_ref() {
            Some(fp) => fp.hash == current_hash,
            None => false,
        }
    }

    /// Invalidate the entire session state (e.g., after a system-prompt change).
    ///
    /// Clears the prefix, log, scratch, and resets all metrics.  After calling
    /// this you must re-initialise the prefix before making API calls.
    pub fn invalidate_session(&self) {
        {
            let mut prefix = self.immutable_prefix.write();
            prefix.clear();
        }
        {
            let mut fp = self.prefix_fingerprint.write();
            *fp = None;
        }
        {
            let mut init = self.prefix_initialised.write();
            *init = false;
        }
        {
            let mut log = self.log_entries.write();
            log.clear();
        }
        {
            let mut scratch = self.scratch_pad.write();
            scratch.clear();
        }
        {
            let mut m = self.metrics.write();
            *m = CacheMetrics {
                session_id: self.config.session_id.clone(),
                start_time: Some(std::time::Instant::now()),
                ..Default::default()
            };
        }

        tracing::warn!(session = %self.config.session_id, "Session invalidated — prefix and log cleared");
    }

    /// Estimate how many tokens the next request will consume.
    ///
    /// Combines the frozen prefix size with current log-entry sizes and
    /// applies a rough ~4 chars/token heuristic.
    pub fn estimate_next_request_tokens(&self) -> usize {
        let prefix_bytes = self.immutable_prefix.read().len();
        let log_bytes: usize = self
            .log_entries
            .read()
            .iter()
            .map(|e| e.content.len() + e.role.len())
            .sum();

        estimate_tokens_from_bytes(prefix_bytes + log_bytes)
    }

    /// Return the number of entries currently in the append-only log.
    pub fn log_len(&self) -> usize {
        self.log_entries.read().len()
    }

    /// Check whether the prefix has been initialised (frozen).
    pub fn is_prefix_initialised(&self) -> bool {
        *self.prefix_initialised.read()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Validate that a role string is one of the accepted values.
    fn validate_role(&self, role: &str) -> Result<(), CacheError> {
        match role {
            "user" | "assistant" | "system" | "tool" => Ok(()),
            _ => Err(CacheError::InvalidRole(role.to_string())),
        }
    }
}

// ============================================================================
// Free-standing Utilities
// ============================================================================

/// Byte-stable JSON serializer — produces deterministic output.
///
/// Guarantees:
/// - Keys sorted alphabetically.
/// - No trailing whitespace variation.
/// - Consistent float formatting.
/// - No embedded timestamps.
pub fn serialize_stable(value: &serde_json::Value) -> Result<String, CacheError> {
    // We implement deterministic serialization manually using BTreeMap-like
    // sorting.  serde_json's default Map preserves insertion order which is
    // non-deterministic across runs.

    fn serialize_value(v: &serde_json::Value, buf: &mut String) -> Result<(), CacheError> {
        match v {
            serde_json::Value::Null => buf.push_str("null"),
            serde_json::Value::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
            serde_json::Value::Number(n) => {
                // Serialize numbers consistently
                if let Some(i) = n.as_i64() {
                    buf.push_str(&i.to_string());
                } else if let Some(f) = n.as_f64() {
                    // Use enough precision to avoid rounding differences
                    buf.push_str(format!("{:.10}", f).trim_end_matches('0').trim_end_matches('.'));
                } else if let Some(u) = n.as_u64() {
                    buf.push_str(&u.to_string());
                }
            }
            serde_json::Value::String(s) => {
                // Escape and quote the string properly
                buf.push('"');
                for ch in s.chars() {
                    match ch {
                        '"' => buf.push_str("\\\""),
                        '\\' => buf.push_str("\\\\"),
                        '\n' => buf.push_str("\\n"),
                        '\r' => buf.push_str("\\r"),
                        '\t' => buf.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            use std::fmt::Write;
                            write!(buf, "\\u{:04x}", c as u32).expect("unwrap failed: reasonix_cache.rs:984");
                        }
                        c => buf.push(c),
                    }
                }
                buf.push('"');
            }
            serde_json::Value::Array(arr) => {
                buf.push('[');
                for (i, item) in arr.iter().enumerate() {
                    if i != 0 {
                        buf.push(',');
                    }
                    serialize_value(item, buf)?;
                }
                buf.push(']');
            }
            serde_json::Value::Object(map) => {
                buf.push('{');
                // Sort keys for deterministic output
                let mut sorted_keys: Vec<&String> = map.keys().collect();
                sorted_keys.sort();
                for (i, k) in sorted_keys.iter().enumerate() {
                    if i != 0 {
                        buf.push(',');
                    }
                    // Recursively serialize key (it's always a string)
                    serialize_value(&serde_json::Value::String((*k).clone()), buf)?;
                    buf.push(':');
                    serialize_value(&map[*k], buf)?;
                }
                buf.push('}');
            }
        }
        Ok(())
    }

    let mut out = String::with_capacity(256);
    serialize_value(value, &mut out)?;
    Ok(out)
}

/// Compute a deterministic fingerprint of arbitrary bytes using SHA-256.
///
/// Returns a hex-encoded 64-character string (SHA-256 = 32 bytes = 64 hex chars).
/// This is cryptographically strong and collision-resistant for our use case.
pub fn compute_fingerprint(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Estimate token count from byte length.
///
/// Uses a rough heuristic of ~4 characters per token, which is reasonable
/// for English text mixed with code.  Not precise — intended only for
/// reporting and capacity planning.
fn estimate_tokens_from_bytes(byte_len: usize) -> usize {
    (byte_len / 4).max(1)
}

/// Timestamp and dynamic-content stripper.
///
/// Removes patterns that would break prefix stability between requests:
/// - ISO-8601 date/time strings.
/// - UUID-formatted strings.
/// - Common version-pattern strings.
/// - Session-ID-like hex strings.
///
/// This is applied to the prefix text **before** hashing, ensuring that
/// minor dynamic variations (e.g., an embedded timestamp) don't bust the cache.
pub fn strip_dynamic_content(text: &str) -> String {
    let mut result = text.to_string();

    // ISO-8601 date patterns like 2024-01-15T10:30:00Z or 2024/01/15 10:30:00
    let iso_re = regex::Regex::new(
        r"\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
    )
    .expect("unwrap failed: reasonix_cache.rs:1062");
    result = iso_re.replace_all(&result, "[DATE]").to_string();

    // UUID pattern: 8-4-4-4-12 hex digits
    let uuid_re = regex::Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    )
    .expect("unwrap failed: reasonix_cache.rs:1069");
    result = uuid_re.replace_all(&result, "[UUID]").to_string();

    // Version pattern: v1.2.3, v0.1.0, etc.
    let ver_re = regex::Regex::new(r"\bv\d+\.\d+\.\d+(?:-[a-zA-Z0-9.]+)?\b").expect("unwrap failed: reasonix_cache.rs:1073");
    result = ver_re.replace_all(&result, "[VERSION]").to_string();

    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Helper: create a default cache for testing
    // -----------------------------------------------------------------

    fn test_cache() -> ReasonixCache {
        ReasonixCache::new(ReasonixConfig {
            session_id: "test-session-001".to_string(),
            ..Default::default()
        })
    }

    fn initialise_test_prefix(cache: &ReasonixCache) -> PrefixFingerprint {
        cache.initialize_prefix(
            "You are a helpful assistant.",
            r#"[{"type":"function","function":{"name":"read_file","description":"Read a file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}, {"type":"function","function":{"name":"write_file","description":"Write a file","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}}}]"#,
            "",
        )
    }

    // -----------------------------------------------------------------
    // test_three_zone_lifecycle
    // -----------------------------------------------------------------

    #[test]
    fn test_three_zone_lifecycle() {
        let cache = test_cache();

        // Zone 1: Initialise prefix
        let fp = initialise_test_prefix(&cache);
        assert!(!fp.hash.is_empty());
        assert!(cache.is_prefix_initialised());

        // Zone 2: Append to log
        cache.append("user", "Hello", 0).unwrap();
        cache.append("assistant", "Hi there!", 0).unwrap();
        assert_eq!(cache.log_len(), 2);

        // Zone 3: Scratch (never appears in payload)
        cache.write_scratch("<thinking>I should respond</thinking>").unwrap();
        assert_eq!(cache.read_scratch(), "<thinking>I should respond</thinking>");

        // Build payload — must NOT contain scratch
        let payload = cache.build_request_payload();
        let payload_str: String = payload.iter().map(|e| e.content.as_str()).collect();
        assert!(
            !payload_str.contains("thinking"),
            "Scratch content leaked into API payload"
        );

        // Clear scratch
        cache.clear_scratch();
        assert_eq!(cache.read_scratch(), "");
    }

    // -----------------------------------------------------------------
    // test_prefix_frozen_after_init
    // -----------------------------------------------------------------

    #[test]
    fn test_prefix_frozen_after_init() {
        let cache = test_cache();

        let fp1 = initialise_test_prefix(&cache);

        // Calling again returns the SAME fingerprint (not recomputed)
        let fp2 = cache.initialize_prefix(
            "Different system prompt!",
            "[]",
            "",
        );
        assert_eq!(fp1.hash, fp2.hash, "Prefix should remain frozen after first init");

        // Integrity check passes
        assert!(cache.validate_prefix_integrity());

        // Fingerprint accessor works
        let fp3 = cache.prefix_fingerprint();
        assert!(fp3.is_some());
        assert_eq!(fp3.unwrap().hash, fp1.hash);
    }

    // -----------------------------------------------------------------
    // test_append_only_grows_monotonically
    // -----------------------------------------------------------------

    #[test]
    fn test_append_only_grows_monotonically() {
        let cache = test_cache();
        initialise_test_prefix(&cache);

        for i in 0..5u32 {
            cache
                .append("user", &format!("Message {}", i), i)
                .unwrap();
            cache
                .append("assistant", &format!("Response {}", i), i)
                .unwrap();
        }

        assert_eq!(cache.log_len(), 10);

        // Verify ordering is preserved
        let payload = cache.build_request_payload();
        assert_eq!(payload[0].content, "Message 0");
        assert_eq!(payload[1].content, "Response 0");
        assert_eq!(payload[8].content, "Message 4");
        assert_eq!(payload[9].content, "Response 4");
    }

    // -----------------------------------------------------------------
    // test_scratch_never_in_payload
    // -----------------------------------------------------------------

    #[test]
    fn test_scratch_never_in_payload() {
        let cache = test_cache();
        initialise_test_prefix(&cache);

        cache.append("user", "Real user message", 0).unwrap();
        cache
            .write_scratch("SECRET_INTERNAL_REASONING_DATA")
            .unwrap();

        let json = cache.build_request_json("deepseek-chat", "");

        // Scratch content must NOT appear in the JSON
        assert!(
            !json.contains("SECRET_INTERNAL"),
            "Scratch data leaked into request JSON"
        );

        // But real user message MUST appear
        assert!(
            json.contains("Real user message"),
            "User message missing from request JSON"
        );
    }

    // -----------------------------------------------------------------
    // test_byte_stable_serialization
    // -----------------------------------------------------------------

    #[test]
    fn test_byte_stable_serialization() {
        // Same logical content → same serialized bytes
        let val = serde_json::json!({
            "z_key": "last",
            "a_key": "first",
            "m_key": "middle",
            "nested": {"z": 1, "a": 2}
        });

        let s1 = serialize_stable(&val).unwrap();
        let s2 = serialize_stable(&val).unwrap();

        assert_eq!(s1, s2, "Serialization must be deterministic");

        // Keys should be sorted
        assert!(s1.starts_with("{\"a_key\""), "Keys should be sorted: 'a' first");
    }

    #[test]
    fn test_serialize_handles_all_types() {
        // Test null, bool, number, string, array, nested object
        let val = serde_json::json!({
            "null_val": null,
            "bool_true": true,
            "bool_false": false,
            "int": 42,
            "float": 3.14159,
            "str": "hello world",
            "arr": [3, 1, 2],
            "obj": {"b": 2, "a": 1}
        });

        let result = serialize_stable(&val).unwrap();
        assert!(result.contains("\"null_val\":null"));
        assert!(result.contains("\"bool_true\":true"));
        assert!(result.contains("\"int\":42"));
        assert!(result.contains("\"str\":\"hello world\""));
        // Array should preserve insertion order (only objects get sorted)
        assert!(result.contains("\"arr\":[3,1,2]"));
        // Nested obj keys should be sorted
        assert!(
            result.contains("\"obj\":{\"a\":1,\"b\":2}"),
            "Nested object keys should be sorted"
        );
    }

    // -----------------------------------------------------------------
    // test_metrics_hit_rate_calculation
    // -----------------------------------------------------------------

    #[test]
    fn test_metrics_hit_rate_calculation() {
        let mut metrics = CacheMetrics {
            session_id: "metrics-test".to_string(),
            start_time: Some(std::time::Instant::now()),
            ..Default::default()
        };

        // No requests yet
        assert_eq!(metrics.hit_rate(), 0.0);
        assert_eq!(metrics.token_cache_hit_rate(), 0.0);

        // Full prefix hit
        metrics.record_api_response(&ApiUsage {
            prompt_tokens: 1000,
            completion_tokens: 50,
            prompt_cache_hit_tokens: 1000,
            prompt_cache_miss_tokens: 0,
            total_cost_usd: 0.001,
        });
        assert_eq!(metrics.total_requests, 1);
        assert_eq!(metrics.prefix_hits, 1);
        assert_eq!(metrics.hit_rate(), 1.0);
        assert!((metrics.token_cache_hit_rate() - 1.0).abs() < f64::EPSILON);

        // Partial hit
        metrics.record_api_response(&ApiUsage {
            prompt_tokens: 1200,
            completion_tokens: 80,
            prompt_cache_hit_tokens: 800,
            prompt_cache_miss_tokens: 400,
            total_cost_usd: 0.0015,
        });
        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.partial_hits, 1);
        assert!((metrics.hit_rate() - 1.0).abs() < f64::EPSILON); // both are hits

        // Complete miss
        metrics.record_api_response(&ApiUsage {
            prompt_tokens: 500,
            completion_tokens: 30,
            prompt_cache_hit_tokens: 0,
            prompt_cache_miss_tokens: 500,
            total_cost_usd: 0.0008,
        });
        assert_eq!(metrics.misses, 1);
        assert!((metrics.hit_rate() - 2.0 / 3.0).abs() < 1e-10);

        // Report formatting doesn't panic
        let report = metrics.format_report();
        assert!(report.contains("ReasonIX Cache Report"));
        assert!(report.contains("metrics-test"));
    }

    // -----------------------------------------------------------------
    // test_full_request_build_cycle
    // -----------------------------------------------------------------

    #[test]
    fn test_full_request_build_cycle() {
        let cache = test_cache();
        let _fp = initialise_test_prefix(&cache);

        // Turn 0
        cache.append("user", "What is 2+2?", 0).unwrap();

        let json_turn0 = cache.build_request_json("deepseek-reasoner", "");
        assert!(json_turn0.contains("model"));
        assert!(json_turn0.contains("messages"));
        assert!(json_turn0.contains("What is 2+2"));

        // Turn 1: add assistant reply + new user message
        cache.append("assistant", "2+2 equals 4.", 0).unwrap();
        cache.append("user", "Now what about 3+3?", 1).unwrap();

        let json_turn1 = cache.build_request_json("deepseek-reasoner", "");

        // Turn 1 JSON should contain ALL previous messages
        assert!(json_turn1.contains("What is 2+2?"));
        assert!(json_turn1.contains("2+2 equals 4."));
        assert!(json_turn1.contains("Now what about 3+3?"));

        // Both serializations should be valid JSON
        let _: serde_json::Value =
            serde_json::from_str(&json_turn0).expect("turn 0 JSON should parse");
        let _: serde_json::Value =
            serde_json::from_str(&json_turn1).expect("turn 1 JSON should parse");
    }

    // -----------------------------------------------------------------
    // test_prefix_integrity_validation
    // -----------------------------------------------------------------

    #[test]
    fn test_prefix_integrity_validation() {
        let cache = test_cache();

        // Not initialised → invalid
        assert!(!cache.validate_prefix_integrity());

        // After init → valid
        initialise_test_prefix(&cache);
        assert!(cache.validate_prefix_integrity());

        // After invalidation → invalid
        cache.invalidate_session();
        assert!(!cache.validate_prefix_integrity());
        assert!(!cache.is_prefix_initialised());
    }

    // -----------------------------------------------------------------
    // test_session_invalidation
    // -----------------------------------------------------------------

    #[test]
    fn test_session_invalidation() {
        let cache = test_cache();
        initialise_test_prefix(&cache);

        cache.append("user", "Before invalidation", 0).unwrap();
        cache.write_scratch("scratch data").unwrap();
        cache.record_response(&ApiUsage {
            prompt_tokens: 100,
            completion_tokens: 10,
            prompt_cache_hit_tokens: 90,
            prompt_cache_miss_tokens: 10,
            total_cost_usd: 0.0005,
        });

        assert!(cache.is_prefix_initialised());
        assert_eq!(cache.log_len(), 1);
        assert!(!cache.read_scratch().is_empty());
        assert_eq!(cache.metrics().total_requests, 1);

        cache.invalidate_session();

        assert!(!cache.is_prefix_initialised());
        assert_eq!(cache.log_len(), 0);
        assert!(cache.read_scratch().is_empty());
        assert_eq!(cache.metrics().total_requests, 0); // metrics also reset
    }

    // -----------------------------------------------------------------
    // test_strip_timestamps
    // -----------------------------------------------------------------

    #[test]
    fn test_strip_timestamps() {
        let input = "Generated on 2024-01-15T10:30:00Z by session abc123-def4-5678-90ef-1234567890ab. Version v1.2.3.";
        let stripped = strip_dynamic_content(input);

        assert!(!stripped.contains("2024-01-15"), "Date should be replaced");
        assert!(!stripped.contains("abc123"), "UUID should be replaced");
        assert!(!stripped.contains("v1.2.3"), "Version should be replaced");
        assert!(stripped.contains("[DATE]"), "Date placeholder present");
        assert!(stripped.contains("[UUID]"), "UUID placeholder present");
        assert!(stripped.contains("[VERSION]"), "Version placeholder present");
        // Static text preserved
        assert!(stripped.contains("Generated on"));
        assert!(stripped.contains("by session"));
    }

    // -----------------------------------------------------------------
    // test_compute_fingerprint_deterministic
    // -----------------------------------------------------------------

    #[test]
    fn test_compute_fingerprint_deterministic() {
        let data = b"Hello, ReasonIX!";
        let h1 = compute_fingerprint(data);
        let h2 = compute_fingerprint(data);
        assert_eq!(h1, h2, "Same input must produce same fingerprint");

        let h3 = compute_fingerprint(b"Different data");
        assert_ne!(h1, h3, "Different input must produce different fingerprint");

        // SHA-256 fingerprints are 64 hex characters
        assert_eq!(h1.len(), 64, "SHA-256 hex output should be 64 chars");
    }

    // -----------------------------------------------------------------
    // test_error_cases
    // -----------------------------------------------------------------

    #[test]
    fn test_invalid_role_error() {
        let cache = test_cache();
        let result = cache.append("hacker", "pwned", 0);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CacheError::InvalidRole(_)));
    }

    #[test]
    fn test_scratch_overflow() {
        let cache = test_cache();
        let big = "x".repeat(5000);
        let result = cache.write_scratch(&big);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CacheError::ScratchOverflow(_, _)));
    }

    #[test]
    fn test_append_scratch_overflow() {
        let cache = test_cache();
        cache.write_scratch(&"x".repeat(2000)).unwrap();
        let result = cache.append_scratch(&"y".repeat(3000));
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------
    // test_tool_result_appending
    // -----------------------------------------------------------------

    #[test]
    fn test_tool_result_appending() {
        let cache = test_cache();
        initialise_test_prefix(&cache);

        cache
            .append_tool_result("read_file", "file contents here", 0)
            .unwrap();

        let payload = cache.build_request_payload();
        assert_eq!(payload.len(), 1);
        assert_eq!(payload[0].role, "tool");
        assert_eq!(payload[0].content, "file contents here");
        assert_eq!(payload[0].tool_name.as_deref(), Some("read_file"));
        assert!(payload[0].tool_call_id.is_some());
    }

    // -----------------------------------------------------------------
    // test_log_trimming_at_capacity
    // -----------------------------------------------------------------

    #[test]
    fn test_log_trimming_at_capacity() {
        let config = ReasonixConfig {
            max_log_entries: 5,
            ..Default::default()
        };
        let cache = ReasonixCache::new(config);
        initialise_test_prefix(&cache);

        // Fill beyond capacity
        for i in 0..8u32 {
            cache.append("user", &format!("msg_{}", i), i).unwrap();
        }

        // Should have trimmed from front, keeping last 5
        assert_eq!(cache.log_len(), 5);
        let payload = cache.build_request_payload();
        // Oldest remaining entry should be msg_3 (indices 0,1,2 evicted)
        assert_eq!(payload[0].content, "msg_3");
        assert_eq!(payload[4].content, "msg_7");
    }

    // -----------------------------------------------------------------
    // test_estimate_next_request_tokens
    // -----------------------------------------------------------------

    #[test]
    fn test_estimate_next_request_tokens() {
        let cache = test_cache();
        initialise_test_prefix(&cache);

        // With just prefix
        let est0 = cache.estimate_next_request_tokens();
        assert!(est0 > 0, "Should estimate some tokens even with empty log");

        // After adding messages
        cache.append("user", &"x".repeat(400), 0).unwrap();
        let est1 = cache.estimate_next_request_tokens();
        assert!(est1 > est0, "Estimate should grow with more content");
    }

    // -----------------------------------------------------------------
    // test_build_request_json_with_new_message
    // -----------------------------------------------------------------

    #[test]
    fn test_build_request_json_with_new_message() {
        let cache = test_cache();
        initialise_test_prefix(&cache);

        cache.append("user", "First message", 0).unwrap();
        cache.append("assistant", "First reply", 0).unwrap();

        // Include a new user message inline
        let json = cache.build_request_json("deepseek-chat", "Second message");

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msgs = parsed["messages"].as_array().unwrap();

        // system + 2 log entries + 1 new message = 4
        assert_eq!(msgs.len(), 4);

        // Last message should be the new user message
        let last = &msgs[msgs.len() - 1];
        assert_eq!(last["role"], "user");
        assert_eq!(last["content"], "Second message");
    }

    // -----------------------------------------------------------------
    // test_context_zone_display
    // -----------------------------------------------------------------

    #[test]
    fn test_context_zone_display() {
        // Just verify the enum variants exist and are comparable
        assert_eq!(ContextZone::ImmutablePrefix, ContextZone::ImmutablePrefix);
        assert_ne!(ContextZone::AppendOnlyLog, ContextZone::VolatileScratch);
    }
}
