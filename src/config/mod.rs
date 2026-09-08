//! DeepSeek Carp configuration system.
//!
//! Configuration is loaded in this priority order:
//! 1. Hardcoded defaults
//! 2. `~/.deepseek-carp/config.toml` (user-level)
//! 3. `<project>/.deepseek-carp/config.toml` (project-level override)
//! 4. Environment variables (`DEEPCARP_*` prefix)
//!
//! API keys are stored separately in `~/.deepseek-carp/credentials.toml`
//! with file permissions set to 600.

pub mod paths;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::providers::TaskCategory;

// ============================================================================
// Inference Mode — Cloud API vs Enterprise (二选一)
// ============================================================================

/// The inference backend mode. Cloud and Enterprise are mutually exclusive.
///
/// ```text
/// Mode A (Cloud):    本地 Qwen3.6-27b → SmartUpgrade → DeepSeek/GLM/Kimi/Minimax
/// Mode B (Enterprise): 本地 Qwen3.6-27b → CarpAI Enterprise 算力集群（不调外部API）
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum InferenceMode {
    /// Cloud API mode — local first, smart upgrade to external APIs when needed.
    #[default]
    Cloud,
    /// Enterprise mode — local Qwen for quick tasks, enterprise cluster for heavy
    /// inference. No external API calls.
    Enterprise,
}


// ============================================================================
// Provider Configuration
// ============================================================================

/// A configured AI provider (API or local).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    /// Provider identifier: "deepseek", "glm", "kimi", "minimax", "qwen-local", etc.
    pub name: String,
    /// API base URL (for API providers) or local endpoint (for local providers).
    pub endpoint: String,
    /// Model name to use with this provider.
    pub model: String,
    /// Whether this is a local provider (Ollama/llama.cpp).
    #[serde(default)]
    pub is_local: bool,
    /// API key reference name in credentials.toml.
    /// If None, uses name as key reference.
    pub api_key_ref: Option<String>,
    /// Request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Maximum concurrent requests to this provider.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    /// Whether this provider is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Extra headers to include in API requests.
    #[serde(default)]
    pub extra_headers: std::collections::HashMap<String, String>,
    /// Task specialization hint for smart routing.
    /// e.g., "code", "chat", "reasoning", "general"
    #[serde(default)]
    pub specialty: Option<String>,
}

fn default_timeout() -> u64 { 60 }
fn default_max_concurrent() -> usize { 4 }
fn default_enabled() -> bool { true }

// ============================================================================
// Orchestration Strategy
// ============================================================================

/// How the orchestrator selects between providers.
///
/// The recommended strategy for most users is `SmartUpgrade`:
/// local Qwen first → if task is complex, smart-route to the best cloud API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum OrchestrationStrategy {
    /// **Smart Upgrade** (推荐默认): 本地 Qwen 先处理 → 如果复杂度/置信度不够，
    /// 按任务类型智能路由到最佳云API → 失败则级联下一个。省钱又可靠。
    #[default]
    SmartUpgrade,
    /// Sequential cascade: try providers in order, fall through on failure.
    /// DeepSeek → GLM → Kimi → Minimax.
    Cascade,
    /// Parallel race: send to all providers simultaneously, first wins.
    /// Fast but expensive (wastes API credits).
    ParallelRace,
    /// **Hybrid Parallel** (v0.2.0): Local + Cloud simultaneously.
    /// Local provides low-latency baseline (<500ms), cloud provides
    /// high-quality answer. Cancels cloud if local returns with high confidence.
    /// Best balance of speed and quality.
    HybridParallel,
    /// Route based on task type (code → DeepSeek, chat → GLM, reasoning → Kimi).
    TaskBasedRouting,
    /// Weighted round-robin based on cost and latency history.
    AdaptiveWeighted,
    /// Use the cheapest provider that meets quality threshold.
    CostOptimized,
}


// ============================================================================
// Smart Upgrade Config
// ============================================================================

/// Configuration for the SmartUpgrade strategy.
///
/// Controls when to upgrade from local to cloud and how to route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartUpgradeConfig {
    /// Local model to use as primary (e.g., "qwen-local").
    #[serde(default = "default_primary_local")]
    pub primary_local: String,

    /// Minimum confidence threshold for local responses (0.0 - 1.0).
    /// If local model's confidence is below this, upgrade to cloud.
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f64,

    /// Maximum local response time in seconds before upgrading.
    #[serde(default = "default_local_timeout")]
    pub local_timeout_secs: u64,

    /// Task complexity keywords that trigger upgrade (Chinese + English).
    #[serde(default = "default_complexity_keywords")]
    pub complexity_keywords: Vec<String>,

    /// When upgrading, use task-based routing or cascade?
    #[serde(default)]
    pub upgrade_strategy: UpgradeStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum UpgradeStrategy {
    /// Smart route to best API for this task.
    #[default]
    TaskBased,
    /// Try APIs in configured order.
    Cascade,
}


fn default_primary_local() -> String { "qwen-local".to_string() }
fn default_confidence_threshold() -> f64 { 0.7 }
fn default_local_timeout() -> u64 { 30 }

fn default_complexity_keywords() -> Vec<String> {
    vec![
        // Chinese keywords
        "重构".into(), "架构".into(), "设计模式".into(), "分布式".into(),
        "并发".into(), "性能优化".into(), "安全漏洞".into(), "算法".into(),
        "大数据".into(), "机器学习".into(), "深度学习".into(), "微服务".into(),
        "数据库优化".into(), "缓存策略".into(), "消息队列".into(),
        "系统设计".into(), "代码审查".into(),
        // English keywords
        "refactor".into(), "architecture".into(), "design pattern".into(),
        "distributed".into(), "concurrency".into(), "performance optimization".into(),
        "security".into(), "algorithm".into(), "big data".into(),
        "machine learning".into(), "deep learning".into(), "microservices".into(),
        "database optimization".into(), "caching strategy".into(),
        "system design".into(), "code review".into(),
    ]
}

impl Default for SmartUpgradeConfig {
    fn default() -> Self {
        Self {
            primary_local: default_primary_local(),
            confidence_threshold: default_confidence_threshold(),
            local_timeout_secs: default_local_timeout(),
            complexity_keywords: default_complexity_keywords(),
            upgrade_strategy: UpgradeStrategy::default(),
        }
    }
}

// ============================================================================
// Local Model Config
// ============================================================================

/// Local model tier configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelTier {
    /// Name of the tier (e.g., "budget", "standard", "premium").
    pub name: String,
    /// Model file name.
    pub model: String,
    /// Approximate context window size (tokens).
    pub context_window: u32,
    /// Recommended for code tasks.
    pub code_optimized: bool,
    /// Recommended for reasoning tasks.
    pub reasoning_optimized: bool,
    /// Relative speed (higher is faster).
    pub speed_factor: f64,
    /// Relative quality (higher is better).
    pub quality_factor: f64,
}

impl Default for LocalModelTier {
    fn default() -> Self {
        Self {
            name: "budget".into(),
            model: "qwen2.5-7b-instruct-1m-q4_k_m.gguf".into(),
            context_window: 131072, // 128K tokens
            code_optimized: false,
            reasoning_optimized: false,
            speed_factor: 1.0,
            quality_factor: 0.7,
        }
    }
}

/// Local model configuration manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelConfig {
    /// Available local model tiers.
    pub tiers: Vec<LocalModelTier>,
    /// Active tier name.
    pub active_tier: String,
    /// Enable automatic tier switching based on task complexity.
    pub auto_tier: bool,
    /// GGUF-specific settings.
    pub gguf_settings: GgufSettings,
}

/// GGUF (llama.cpp) specific settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufSettings {
    /// Number of layers to offload to GPU.
    pub n_gpu_layers: i32,
    /// Context size.
    pub n_ctx: u32,
    /// Threads to use.
    pub n_threads: u32,
    /// Threads to use for batch processing.
    pub n_threads_batch: u32,
    /// Enable flash attention.
    pub flash_attn: bool,
}

impl Default for GgufSettings {
    fn default() -> Self {
        Self {
            n_gpu_layers: -1, // Offload all layers
            n_ctx: 65536, // 64K tokens
            n_threads: 8,
            n_threads_batch: 8,
            flash_attn: true,
        }
    }
}

impl Default for LocalModelConfig {
    fn default() -> Self {
        Self {
            tiers: vec![
                LocalModelTier {
                    name: "budget".into(),
                    model: "qwen2.5-7b-instruct-1m-q4_k_m.gguf".into(),
                    context_window: 131072, // 128K tokens
                    code_optimized: false,
                    reasoning_optimized: false,
                    speed_factor: 1.0,
                    quality_factor: 0.7,
                },
                LocalModelTier {
                    name: "code".into(),
                    model: "qwen2.5-coder-7b-instruct-q4_0.gguf".into(),
                    context_window: 131072, // 128K tokens
                    code_optimized: true,
                    reasoning_optimized: false,
                    speed_factor: 0.9,
                    quality_factor: 0.8,
                },
                LocalModelTier {
                    name: "premium".into(),
                    model: "DeepSeek-R1-14B-Q4_K_M.gguf".into(),
                    context_window: 131072, // 128K tokens
                    code_optimized: true,
                    reasoning_optimized: true,
                    speed_factor: 0.5,
                    quality_factor: 0.95,
                },
                LocalModelTier {
                    name: "ultra".into(),
                    model: "Qwen3.6-27B-Q4_K_M.gguf".into(),
                    context_window: 131072, // 128K tokens
                    code_optimized: true,
                    reasoning_optimized: true,
                    speed_factor: 0.3,
                    quality_factor: 0.98,
                },
            ],
            active_tier: "budget".into(),
            auto_tier: true,
            gguf_settings: GgufSettings::default(),
        }
    }
}

impl LocalModelConfig {
    /// Get the active model tier.
    pub fn active_tier(&self) -> Option<&LocalModelTier> {
        self.tiers.iter().find(|t| t.name == self.active_tier)
    }

    /// Select the best tier for a task category.
    pub fn select_tier_for_task(&self, category: &TaskCategory) -> &LocalModelTier {
        if !self.auto_tier {
            return self.active_tier().unwrap_or(&self.tiers[0]);
        }

        match category {
            TaskCategory::CodeGeneration | TaskCategory::CodeReview => {
                self.tiers.iter()
                    .find(|t| t.code_optimized)
                    .unwrap_or(&self.tiers[0])
            }
            TaskCategory::ComplexReasoning => {
                self.tiers.iter()
                    .find(|t| t.reasoning_optimized)
                    .unwrap_or_else(|| self.tiers.last().unwrap_or(&self.tiers[0]))
            }
            _ => {
                self.tiers.iter()
                    .find(|t| t.name == "budget")
                    .unwrap_or(&self.tiers[0])
            }
        }
    }

    /// List all available tiers.
    pub fn list_tiers(&self) -> Vec<String> {
        self.tiers.iter().map(|t| t.name.clone()).collect()
    }
}

// ============================================================================
// Orchestration Config
// ============================================================================

/// Configuration for multi-provider orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    /// Active orchestration strategy.
    #[serde(default)]
    pub strategy: OrchestrationStrategy,

    /// SmartUpgrade settings (used when strategy is SmartUpgrade).
    #[serde(default)]
    pub smart_upgrade: SmartUpgradeConfig,

    /// Ordered list of cloud API providers to try (by name).
    /// Default: ["deepseek", "glm", "kimi", "minimax"]
    #[serde(default = "default_api_order")]
    pub api_order: Vec<String>,

    /// Ordered list of local providers to try.
    /// Default: ["qwen-local", "deepseek-local"]
    #[serde(default = "default_local_order")]
    pub local_order: Vec<String>,

    /// Maximum failures before marking a provider as unhealthy.
    #[serde(default = "default_max_failures")]
    pub max_failures_before_disable: u32,

    /// Cooldown period in seconds before retrying a disabled provider.
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: u64,

    /// Health check interval in seconds.
    #[serde(default = "default_health_check_secs")]
    pub health_check_interval_secs: u64,

    /// Maximum total timeout for orchestrated requests.
    #[serde(default = "default_orch_timeout")]
    pub total_timeout_secs: u64,
}

fn default_api_order() -> Vec<String> {
    vec![
        // Chinese domestic auto-upgrade chain (国产自动兜底链路)
        "deepseek".to_string(),
        "glm".to_string(),
        "kimi".to_string(),
        "minimax".to_string(),
        // Overseas providers NOT in auto chain (海外需手动选择):
        // "openai", "claude", "copilot"
    ]
}

fn default_local_order() -> Vec<String> {
    vec![
        "qwen-local".to_string(),
        "deepseek-local".to_string(),
    ]
}

fn default_max_failures() -> u32 { 3 }
fn default_cooldown_secs() -> u64 { 120 }
fn default_health_check_secs() -> u64 { 30 }
fn default_orch_timeout() -> u64 { 120 }

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            strategy: OrchestrationStrategy::default(),
            smart_upgrade: SmartUpgradeConfig::default(),
            api_order: default_api_order(),
            local_order: default_local_order(),
            max_failures_before_disable: 3,
            cooldown_secs: 120,
            health_check_interval_secs: 30,
            total_timeout_secs: 120,
        }
    }
}

// ============================================================================
// Enterprise Compute Node Config
// ============================================================================

/// Configuration for connecting to a CarpAI Enterprise cluster as a compute node.
/// Only used when `inference_mode = "enterprise"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterpriseNodeConfig {
    /// Enterprise server URL (e.g., "https://enterprise.carpai.example.com").
    pub server_url: Option<String>,
    /// gRPC endpoint for compute node registration.
    pub grpc_endpoint: Option<String>,
    /// Authentication token for the enterprise cluster.
    pub auth_token: Option<String>,
    /// Node name (auto-generated if not set).
    pub node_name: Option<String>,
    /// Maximum GPU memory to allocate for enterprise tasks (in MB).
    #[serde(default = "default_gpu_memory_mb")]
    pub max_gpu_memory_mb: u64,
    /// Maximum CPU cores to use for enterprise tasks (0 = auto-detect).
    #[serde(default)]
    pub max_cpu_cores: u32,
    /// Heartbeat interval in seconds.
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_interval_secs: u64,
    /// Enterprise inference strategy: use enterprise exclusively or allow local fallback.
    #[serde(default)]
    pub allow_local_fallback: bool,
}

fn default_gpu_memory_mb() -> u64 { 8192 }
fn default_heartbeat_secs() -> u64 { 30 }

impl Default for EnterpriseNodeConfig {
    fn default() -> Self {
        Self {
            server_url: None,
            grpc_endpoint: None,
            auth_token: None,
            node_name: None,
            max_gpu_memory_mb: 8192,
            max_cpu_cores: 0,
            heartbeat_interval_secs: 30,
            allow_local_fallback: true,
        }
    }
}

// ============================================================================
// TUI / UI Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Color theme: "dark", "light", "deepseek" (brand colors).
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Show startup banner.
    #[serde(default = "default_true")]
    pub show_banner: bool,
    /// Enable syntax highlighting in chat.
    #[serde(default = "default_true")]
    pub syntax_highlighting: bool,
    /// Maximum history lines in scrollback.
    #[serde(default = "default_history_lines")]
    pub max_history_lines: usize,
    /// Auto-copy last response to clipboard.
    #[serde(default)]
    pub auto_copy: bool,
    /// Show provider info in status bar.
    #[serde(default = "default_true")]
    pub show_provider_info: bool,
}

fn default_theme() -> String { "deepseek".to_string() }
fn default_true() -> bool { true }
fn default_history_lines() -> usize { 10000 }

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "deepseek".to_string(),
            show_banner: true,
            syntax_highlighting: true,
            max_history_lines: 10000,
            auto_copy: false,
            show_provider_info: true,
        }
    }
}

// ============================================================================
// Main Config
// ============================================================================

/// Root configuration for DeepSeek Carp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekConfig {
    /// Inference mode: Cloud (external APIs) or Enterprise (corporate cluster).
    /// These two modes are mutually exclusive.
    #[serde(default)]
    pub inference_mode: InferenceMode,

    /// All configured providers (both API and local).
    #[serde(default = "default_providers")]
    pub providers: Vec<ProviderEntry>,

    /// Multi-provider orchestration settings.
    #[serde(default)]
    pub orchestration: OrchestrationConfig,

    /// Enterprise compute node settings (only used in Enterprise mode).
    #[serde(default)]
    pub enterprise: EnterpriseNodeConfig,

    /// UI settings.
    #[serde(default)]
    pub ui: UiConfig,

    /// Local model configuration.
    #[serde(default)]
    pub local_models: LocalModelConfig,

    /// Default working directory for sessions.
    #[serde(default)]
    pub default_workdir: Option<PathBuf>,

    /// Log level: "trace", "debug", "info", "warn", "error".
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Maximum agent iterations per turn.
    #[serde(default = "default_max_iterations")]
    pub max_agent_iterations: u32,

    /// Max context messages to keep in conversation history.
    #[serde(default = "default_context_size")]
    pub max_context_messages: usize,
}

fn default_log_level() -> String { "info".to_string() }
fn default_max_iterations() -> u32 { 100 }
fn default_context_size() -> usize { 100 }

/// Default providers that ship with DeepSeek Carp.
///
/// Local model strategy (llama-server with --models-dir):
/// - qwen-local (必选): Qwen2.5-7B (日常) / Qwen2.5-Coder-7B (代码) / Qwen3.6-27B (复杂推理)
///   三模型通过 llama-server --models-dir 自动路由
/// - deepseek-local (可选): DeepSeek-R1-14B, disabled by default
///
/// All local models run via llama-server:
///   llama-server --models-dir ./models --port 8080 -c 8192
fn default_providers() -> Vec<ProviderEntry> {
    vec![
        // === Local providers ===
        // qwen-local (必选): 三模型集成 —
        //   qwen2.5-7b-instruct-1m-q4_k_m.gguf (日常，默认)
        //   qwen2.5-coder-7b-instruct-q4_0.gguf   (代码)
        //   Qwen3.6-27B-Q4_K_M.gguf              (复杂推理)
        // llama-server --models-dir ./models 自动按请求路由模型
        ProviderEntry {
            name: "qwen-local".into(),
            endpoint: "http://localhost:8080".into(),
            model: "qwen2.5-7b-instruct-1m-q4_k_m.gguf".into(),
            is_local: true,
            api_key_ref: None,
            timeout_secs: 60,
            max_concurrent: 3,
            enabled: true,
            extra_headers: Default::default(),
            specialty: Some("general".into()),
        },
        // deepseek-local (可选): On-demand for code reasoning
        ProviderEntry {
            name: "deepseek-local".into(),
            endpoint: "http://localhost:8080".into(),
            model: "DeepSeek-R1-14B-Q4_K_M.gguf".into(),
            is_local: true,
            api_key_ref: None,
            timeout_secs: 120,
            max_concurrent: 1,
            enabled: false,
            extra_headers: Default::default(),
            specialty: Some("code".into()),
        },
        // === Overseas providers (海外入口，需手动选择) ===
        // NOT in auto-upgrade chain. Users must manually enable via config
        // or --provider flag. Keep configuration entries so users can opt in.
        ProviderEntry {
            name: "claude".into(),
            endpoint: "https://api.anthropic.com/v1".into(),
            model: "claude-sonnet-4-20250514".into(),
            is_local: false,
            api_key_ref: Some("claude".into()),
            timeout_secs: 120,
            max_concurrent: 4,
            enabled: false,       // 手动选择，不在自动兜底链路中
            extra_headers: Default::default(),
            specialty: Some("reasoning".into()),
        },
        ProviderEntry {
            name: "openai".into(),
            endpoint: "https://api.openai.com/v1".into(),
            model: "gpt-4o".into(),
            is_local: false,
            api_key_ref: Some("openai".into()),
            timeout_secs: 120,
            max_concurrent: 4,
            enabled: false,       // 手动选择，不在自动兜底链路中
            extra_headers: Default::default(),
            specialty: Some("general".into()),
        },
        ProviderEntry {
            name: "copilot".into(),
            endpoint: "https://api.githubcopilot.com".into(),
            model: "copilot-gpt-4o".into(),
            is_local: false,
            api_key_ref: Some("copilot".into()),
            timeout_secs: 120,
            max_concurrent: 4,
            enabled: false,       // 手动选择，不在自动兜底链路中
            extra_headers: Default::default(),
            specialty: Some("code".into()),
        },
        // === Chinese domestic providers (国内服务商，自动兜底链路) ===
        ProviderEntry {
            name: "deepseek".into(),
            endpoint: "https://api.deepseek.com/v1".into(),
            model: "deepseek-v4-pro".into(),
            is_local: false,
            api_key_ref: Some("deepseek".into()),
            timeout_secs: 120,
            max_concurrent: 4,
            enabled: true,
            extra_headers: Default::default(),
            specialty: Some("code".into()),
        },
        ProviderEntry {
            name: "glm".into(),
            endpoint: "https://open.bigmodel.cn/api/paas/v4".into(),
            model: "GLM-5.1".into(),
            is_local: false,
            api_key_ref: Some("glm".into()),
            timeout_secs: 120,
            max_concurrent: 4,
            enabled: true,
            extra_headers: Default::default(),
            specialty: Some("chat".into()),
        },
        ProviderEntry {
            name: "kimi".into(),
            endpoint: "https://api.moonshot.cn/v1".into(),
            model: "kimi-2.6".into(),
            is_local: false,
            api_key_ref: Some("kimi".into()),
            timeout_secs: 120,
            max_concurrent: 4,
            enabled: true,
            extra_headers: Default::default(),
            specialty: Some("reasoning".into()),
        },
        ProviderEntry {
            name: "minimax".into(),
            endpoint: "https://api.minimax.chat/v1".into(),
            model: "M2.7".into(),
            is_local: false,
            api_key_ref: Some("minimax".into()),
            timeout_secs: 120,
            max_concurrent: 4,
            enabled: true,
            extra_headers: Default::default(),
            specialty: Some("general".into()),
        },
    ]
}

impl Default for DeepSeekConfig {
    fn default() -> Self {
        Self {
            inference_mode: InferenceMode::default(),
            providers: default_providers(),
            orchestration: OrchestrationConfig::default(),
            enterprise: EnterpriseNodeConfig::default(),
            ui: UiConfig::default(),
            local_models: LocalModelConfig::default(),
            default_workdir: None,
            log_level: "info".to_string(),
            max_agent_iterations: 100,
            max_context_messages: 100,
        }
    }
}

// ============================================================================
// Credentials (separate file, permission 600)
// ============================================================================

/// API credentials stored in `~/.deepseek-carp/credentials.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Credentials {
    /// Map of provider name → API key.
    #[serde(default)]
    pub api_keys: std::collections::HashMap<String, String>,
    /// Enterprise auth token.
    pub enterprise_token: Option<String>,
}

// ============================================================================
// Config Loading
// ============================================================================

impl DeepSeekConfig {
    /// Load configuration from all sources, with proper priority merging.
    pub fn load() -> anyhow::Result<Self> {
        let config_path = paths::config_file();

        // Start with defaults
        let mut config = Self::default();

        // Layer 1: User config file
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let user_config: DeepSeekConfig = toml::from_str(&content)?;
            config = merge_configs(config, user_config);
        }

        // Layer 2: Environment variables (DEEPCARP_*)
        config = apply_env_overrides(config);

        Ok(config)
    }

    /// Save current config to user config file.
    pub fn save(&self) -> anyhow::Result<()> {
        let config_path = paths::config_file();
        paths::ensure_dirs();
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        tracing::info!("Config saved to {}", config_path.display());
        Ok(())
    }

    /// Load credentials from the credentials file.
    pub fn load_credentials() -> anyhow::Result<Credentials> {
        let cred_path = paths::credentials_file();
        if cred_path.exists() {
            let content = std::fs::read_to_string(&cred_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(Credentials::default())
        }
    }

    /// Save credentials with secure permissions (600 on Unix).
    pub fn save_credentials(creds: &Credentials) -> anyhow::Result<()> {
        let cred_path = paths::credentials_file();
        paths::ensure_dirs();
        let content = toml::to_string_pretty(creds)?;
        std::fs::write(&cred_path, &content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cred_path, std::fs::Permissions::from_mode(0o600))?;
        }

        tracing::info!("Credentials saved to {}", cred_path.display());
        Ok(())
    }

    /// Find the API key for a provider.
    pub fn get_api_key(&self, provider_name: &str) -> Option<String> {
        let env_var = format!(
            "DEEPCARP_API_KEY_{}",
            provider_name.to_uppercase().replace('-', "_")
        );
        if let Ok(key) = std::env::var(&env_var) {
            if !key.is_empty() {
                return Some(key);
            }
        }
        Self::load_credentials()
            .ok()
            .and_then(|c| c.api_keys.get(provider_name).cloned())
    }

    /// Generate default config file if it doesn't exist.
    pub fn ensure_config_file() -> anyhow::Result<()> {
        let config_path = paths::config_file();
        if !config_path.exists() {
            let config = Self::default();
            config.save()?;
            tracing::info!("Created default config at {}", config_path.display());
        }
        Ok(())
    }

    /// Validate configuration consistency.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Check mode consistency
        if self.inference_mode == InferenceMode::Enterprise {
            if self.enterprise.server_url.is_none() {
                warnings.push(
                    "Enterprise mode enabled but no server_url configured. \
                     Run: deepseek-carp enterprise connect --server <url>".into()
                );
            }
            // Warn if cloud API keys are also configured
            let creds = Self::load_credentials().unwrap_or_default();
            let has_api_keys = creds.api_keys.values().any(|k| !k.is_empty());
            if has_api_keys {
                warnings.push(
                    "Enterprise mode is active — cloud API keys will NOT be used. \
                     Remove API keys if you want pure enterprise mode.".into()
                );
            }
        }

        if self.inference_mode == InferenceMode::Cloud {
            let creds = Self::load_credentials().unwrap_or_default();
            if creds.api_keys.is_empty() {
                warnings.push(
                    "Cloud mode active but no API keys configured. \
                     Run: deepseek-carp config set-api-key <provider>".into()
                );
            }
        }

        warnings
    }
}

// ============================================================================
// Config Merging & Env Overrides
// ============================================================================

fn merge_configs(mut base: DeepSeekConfig, override_config: DeepSeekConfig) -> DeepSeekConfig {
    // Inference mode (explicit override)
    base.inference_mode = override_config.inference_mode;

    // Providers: override list completely if provided
    if !override_config.providers.is_empty() {
        base.providers = override_config.providers;
    }

    // Orchestration: merge fields
    base.orchestration.strategy = override_config.orchestration.strategy;
    if !override_config.orchestration.api_order.is_empty() {
        base.orchestration.api_order = override_config.orchestration.api_order;
    }
    if !override_config.orchestration.local_order.is_empty() {
        base.orchestration.local_order = override_config.orchestration.local_order;
    }

    // Enterprise
    base.enterprise = override_config.enterprise;

    // UI
    base.ui = override_config.ui;

    // Local models
    base.local_models = override_config.local_models;

    if let Some(wd) = override_config.default_workdir {
        base.default_workdir = Some(wd);
    }
    if override_config.log_level != "info" {
        base.log_level = override_config.log_level;
    }
    base.max_agent_iterations = override_config.max_agent_iterations;
    base.max_context_messages = override_config.max_context_messages;

    base
}

fn apply_env_overrides(mut config: DeepSeekConfig) -> DeepSeekConfig {
    // DEEPCARP_MODE
    if let Ok(mode) = std::env::var("DEEPCARP_MODE") {
        config.inference_mode = match mode.to_lowercase().as_str() {
            "enterprise" | "corp" => InferenceMode::Enterprise,
            _ => InferenceMode::Cloud,
        };
    }
    // DEEPCARP_LOG_LEVEL
    if let Ok(level) = std::env::var("DEEPCARP_LOG_LEVEL") {
        config.log_level = level;
    }
    // DEEPCARP_ORCHESTRATION_STRATEGY
    if let Ok(strategy) = std::env::var("DEEPCARP_ORCHESTRATION_STRATEGY") {
        config.orchestration.strategy = match strategy.as_str() {
            "smart_upgrade" | "smart" => OrchestrationStrategy::SmartUpgrade,
            "cascade" => OrchestrationStrategy::Cascade,
            "parallel_race" | "parallel" => OrchestrationStrategy::ParallelRace,
            "task_based" | "task" => OrchestrationStrategy::TaskBasedRouting,
            "adaptive" => OrchestrationStrategy::AdaptiveWeighted,
            "cost" => OrchestrationStrategy::CostOptimized,
            _ => OrchestrationStrategy::SmartUpgrade,
        };
    }
    // DEEPCARP_ENTERPRISE_SERVER
    if let Ok(url) = std::env::var("DEEPCARP_ENTERPRISE_SERVER") {
        config.enterprise.server_url = Some(url);
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_inference_mode_is_cloud() {
        let config = DeepSeekConfig::default();
        assert_eq!(config.inference_mode, InferenceMode::Cloud);
    }

    #[test]
    fn test_default_strategy_is_smart_upgrade() {
        let config = DeepSeekConfig::default();
        assert_eq!(config.orchestration.strategy, OrchestrationStrategy::SmartUpgrade);
    }

    #[test]
    fn test_primary_local_is_qwen_local() {
        let config = DeepSeekConfig::default();
        let qwen = config.providers.iter()
            .find(|p| p.name == "qwen-local")
            .expect("qwen-local should exist (必选)");
        assert_eq!(qwen.model, "qwen2.5-7b-instruct-1m-q4_k_m.gguf");
        assert!(qwen.is_local);
        assert!(qwen.enabled, "qwen-local must be enabled by default");
    }

    #[test]
    fn test_local_providers() {
        let config = DeepSeekConfig::default();
        // qwen-local (必选): enabled
        let qwen = config.providers.iter().find(|p| p.name == "qwen-local").unwrap();
        assert!(qwen.enabled, "qwen-local is mandatory");
        // deepseek-local (可选): disabled
        let ds = config.providers.iter().find(|p| p.name == "deepseek-local").unwrap();
        assert!(!ds.enabled, "deepseek-local is optional");
        // Verify only 2 local providers
        let locals: Vec<_> = config.providers.iter().filter(|p| p.is_local).collect();
        assert_eq!(locals.len(), 2, "Only 2 local providers: qwen-local + deepseek-local");
    }

    #[test]
    fn test_complexity_keywords_not_empty() {
        let config = SmartUpgradeConfig::default();
        assert!(!config.complexity_keywords.is_empty());
        assert!(config.complexity_keywords.contains(&"重构".to_string()));
        assert!(config.complexity_keywords.contains(&"refactor".to_string()));
    }

    #[test]
    fn test_api_order() {
        let config = DeepSeekConfig::default();
        // Enabled cloud providers (auto-upgrade chain only)
        let apis: Vec<&str> = config.providers.iter()
            .filter(|p| !p.is_local && p.enabled)
            .map(|p| p.name.as_str())
            .collect();
        // 4 domestic: deepseek, glm, kimi, minimax
        // 3 overseas (disabled): claude, openai, copilot
        assert_eq!(apis, vec!["deepseek", "glm", "kimi", "minimax"]);
        // Verify overseas exist but disabled
        let claude = config.providers.iter().find(|p| p.name == "claude").unwrap();
        assert!(!claude.enabled, "Overseas providers should be disabled by default");
    }

    #[test]
    fn test_enterprise_validation_warns_missing_url() {
        let mut config = DeepSeekConfig::default();
        config.inference_mode = InferenceMode::Enterprise;
        let warnings = config.validate();
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_serialize_roundtrip() {
        let config = DeepSeekConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: DeepSeekConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.inference_mode, InferenceMode::Cloud);
        assert_eq!(parsed.orchestration.strategy, OrchestrationStrategy::SmartUpgrade);
    }
}
