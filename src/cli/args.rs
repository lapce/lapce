//! CLI argument definitions using clap.
//!
//! All commands and flags for `deepseek-carp`.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// DeepSeek Carp — Multi-provider AI coding assistant
///
/// Local-first AI coding assistant with multi-provider orchestration.
/// 本地优先：Qwen → DeepSeekCoder → Kimi → GLM 本地模型，
/// 云端兜底：DeepSeek API → GLM API → Kimi API → Minimax API，
/// 海外入口 (手动选择): Claude · OpenAI · Copilot
#[derive(Parser, Debug)]
#[command(
    name = "deepseek-carp",
    version,
    about = "DeepSeek Carp — Multi-provider AI coding assistant",
    long_about = "Local-first AI coding assistant. Local models first (Qwen, DeepSeekCoder), \
                  cloud auto-fallback (DeepSeek → GLM → Kimi → Minimax). \
                  Overseas Claude/OpenAI/Copilot available as manual opt-in.",
    after_help = "For more information, visit: https://github.com/juming75/deepseek-carp"
)]
pub struct Cli {
    /// Subcommand
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Working directory (defaults to current directory)
    #[arg(short = 'd', long, global = true)]
    pub dir: Option<PathBuf>,

    /// Configuration file path
    #[arg(long, global = true, env = "DEEPCARP_CONFIG")]
    pub config: Option<PathBuf>,

    /// Log level: trace, debug, info, warn, error
    #[arg(long, global = true, env = "DEEPCARP_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Provider to use (overrides config)
    #[arg(short = 'p', long, global = true)]
    pub provider: Option<String>,

    /// Model to use (overrides config)
    #[arg(short = 'm', long, global = true)]
    pub model: Option<String>,

    /// Orchestration strategy (default: smart-upgrade)
    #[arg(long, global = true)]
    pub strategy: Option<OrchStrategyArg>,

    /// Inference mode: cloud or enterprise (default: cloud from config)
    #[arg(long, global = true)]
    pub mode: Option<ModeArg>,

    /// Non-interactive mode (no TUI)
    #[arg(long, global = true)]
    pub no_tui: bool,

    /// Output in machine-readable JSON format (CLI-Anything compatible)
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(ValueEnum, Debug, Clone)]
pub enum ModeArg {
    /// Cloud API mode: local Qwen + external APIs (DeepSeek/GLM/Kimi/Minimax)
    Cloud,
    /// Enterprise mode: local Qwen + CarpAI Enterprise cluster (no external APIs)
    Enterprise,
}

#[derive(ValueEnum, Debug, Clone)]
pub enum OrchStrategyArg {
    /// Smart upgrade: local Qwen first, upgrade to cloud only for complex tasks (default, recommended)
    #[clap(name = "smart-upgrade")]
    SmartUpgrade,
    /// Cascade: try providers in order, fall through
    Cascade,
    /// Parallel race: send to all at once, first wins (expensive)
    #[clap(name = "parallel-race")]
    ParallelRace,
    /// Task-based routing: route by prompt type
    #[clap(name = "task-based")]
    TaskBased,
    /// Adaptive: weighted by performance history
    Adaptive,
    /// Cost-optimized: cheapest acceptable response
    #[clap(name = "cost-optimized")]
    CostOptimized,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start interactive chat (TUI mode)
    Chat {
        /// Initial prompt to send
        prompt: Option<String>,
        /// Working directory
        #[arg(short = 'd', long)]
        dir: Option<PathBuf>,
        /// Resume a specific session
        #[arg(long)]
        session: Option<String>,
    },

    /// One-shot question (non-interactive)
    Ask {
        /// The question to ask
        question: Vec<String>,
        /// Working directory
        #[arg(short = 'd', long)]
        dir: Option<PathBuf>,
    },

    /// Get code completion at cursor position
    Complete {
        /// File to complete in
        file: PathBuf,
        /// Line number (1-based)
        #[arg(short = 'l', long)]
        line: usize,
        /// Column number (1-based)
        #[arg(short = 'c', long)]
        column: usize,
    },

    /// Configuration management
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Start as a server (for remote/enterprise mode)
    Serve {
        /// Port to listen on
        #[arg(short = 'p', long, default_value = "8734")]
        port: u16,
        /// Bind address
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },

    /// Download models and set up local environment (one-click)
    Setup {
        /// Force re-download all models
        #[arg(long)]
        force: bool,
        /// Custom models directory
        #[arg(long, default_value = "./models")]
        models_dir: String,
    },

    /// Start local model server (auto-detects llama-server)
    ServeLlama {
        /// Port for llama-server (default: 8080)
        #[arg(short = 'p', long, default_value = "8080")]
        port: u16,
        /// Models directory
        #[arg(long, default_value = "./models")]
        models_dir: String,
        /// Context size
        #[arg(long, default_value = "8192")]
        ctx_size: u32,
        /// CPU threads
        #[arg(long, default_value = "8")]
        threads: u32,
    },

    /// Enterprise compute node commands (requires enterprise feature)
    #[command(subcommand)]
    Enterprise(EnterpriseCommand),

    /// Show providers and health status
    Providers {
        /// Show detailed stats
        #[arg(short = 'v', long)]
        verbose: bool,
    },

    /// Manage sessions
    #[command(subcommand)]
    Session(SessionCommand),

    /// Print version information
    Version,

    /// Review code: file, directory, or full PR (multi-agent 5-dimension analysis)
    Review {
        /// Target: file path, directory, "pr", branch name, or diff file
        target: String,
        /// Review aspects (comma-separated): security,performance,correctness,style,tests
        #[arg(long)]
        aspects: Option<String>,
        /// Enable multi-agent parallel review mode
        #[arg(long)]
        pr_mode: bool,
        /// Auto-apply HIGH+ severity suggestions
        #[arg(long)]
        auto_apply: bool,
        /// Auto-verify fixes with cargo check after applying
        #[arg(long)]
        auto_verify: bool,
        /// Path to workflow YAML file (for custom multi-agent pipelines)
        #[arg(long)]
        workflow: Option<String>,
        /// Approval gate level before applying fixes: none, critical, all
        #[arg(long, default_value = "none")]
        approval_gate: String,
    },

    /// Manage community skills (add, list, run, init, remove)
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    Fix {
        /// Max auto-fix rounds (default: 3)
        #[arg(long, default_value = "3")]
        max_rounds: u32,
        /// Auto-apply fixes without confirmation
        #[arg(long)]
        yes: bool,
    },

    /// Manage company profiles (multi-project isolation)
    #[command(subcommand)]
    Company(CompanyAction),

    /// Run the unified agent loop (Observe→Plan→Act→Evaluate)
    Verify {
        /// Target: file path or URL to verify
        #[arg(value_parser = clap::value_parser!(std::path::PathBuf))]
        target: std::path::PathBuf,
        /// Maximum loop rounds (default: 5)
        #[arg(long, default_value = "5")]
        max_rounds: u32,
        /// Mode: review (code quality) or test (browser E2E)
        #[arg(long, default_value = "review")]
        mode: String,
        /// Verbose per-phase timing
        #[arg(long)]
        verbose: bool,
        /// Cognitive role for the LLM (developer|architect|reviewer|qa|founder|release|security|manager|designer)
        #[arg(long, default_value = "developer")]
        role: String,
        /// Ratchet mode: auto-revert changes on failed rounds (autoresearch keep-or-discard)
        #[arg(long)]
        ratchet: bool,
        /// Per-round timeout in seconds (default: 120, Fixed Budget Evaluation)
        #[arg(long, default_value = "120")]
        timeout: u64,
    },

    /// Index codebase and search for relevant context
    Rag {
        /// Search query (omitting starts interactive mode)
        query: Option<String>,
    },

    /// Show metrics and usage statistics
    Stats,

    /// Manage scheduled automation tasks (cron, interval, loop, event)
    #[command(subcommand)]
    Schedule(ScheduleCommand),

    /// Run SWE-bench benchmark evaluation
    #[command(subcommand)]
    Benchmark(BenchmarkCommand),

    /// Output Claude Desktop / VS Code MCP server configuration JSON
    McpConfig,

    /// Generate shell completions (bash, zsh, fish, powershell)
    Completions {
        /// Shell: bash, zsh, fish, powershell
        shell: String,
    },

    /// Analyze a URL visually (UI-TARS / Mano-P inspired visual page analysis)
    Analyze {
        /// URL to analyze
        url: String,
        /// Run visual diff against another URL
        #[arg(long)]
        diff: Option<String>,
        /// Output analysis as JSON
        #[arg(long)]
        json: bool,
        /// Screenshots output directory
        #[arg(long)]
        output: Option<String>,
    },

    /// Browse a URL with natural language task (Browser-use / Webwright pattern)
    Browse {
        /// URL to browse
        url: String,
        /// Natural language task description
        task: String,
        /// Screenshots output directory
        #[arg(long)]
        output: Option<String>,
        /// Enable LLM-based action planning
        #[arg(long)]
        llm: bool,
    },

    /// Execute task with swarm multi-agent collaboration
    Swarm {
        /// Task description for the swarm
        task: String,
        /// Enable RLM tiered execution mode (cost-optimized sub-models)
        #[arg(long)]
        rlm: bool,
    },

    /// Voice input — transcribe audio to text (P3)
    Voice {
        /// Audio file path (WAV format) to transcribe
        file: Option<PathBuf>,
        /// STT backend to use
        #[arg(long, default_value = "cloud")]
        backend: String,
        /// Language code (e.g., "en", "zh", "auto")
        #[arg(short = 'l', long, default_value = "auto")]
        language: String,
        /// Send transcribed text directly to agent for processing
        #[arg(long)]
        agent: bool,
    },

    /// Manage archived loop runs (P3)
    #[command(subcommand)]
    Archive(ArchiveCommand),
}

/// Subcommand: skill management
#[derive(Subcommand, Debug)]
pub enum SkillAction {
    /// List all installed skills
    List,
    /// Install a skill (file path, URL, or community name)
    Add {
        /// Source: file path, URL, or community skill name
        source: String,
    },
    /// Auto-install from GitHub repo or direct SKILL.md URL (CLI-Anything pattern)
    /// Auto-discovers SKILL.md from GitHub repos, falls back to direct URL fetch.
    Install {
        /// GitHub repo URL (https://github.com/owner/repo) or direct SKILL.md URL
        url: String,
    },
    /// Run a skill with optional input
    Run {
        /// Skill name
        name: String,
        /// Input/prompt for the skill
        #[arg(default_value = "")]
        input: String,
    },
    /// Create a new SKILL.md template
    Init {
        /// Skill name
        name: String,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name
        name: String,
    },
    /// Search community registry
    Search {
        /// Search query
        query: String,
    },
    /// Analyze a URL visually (UI-TARS / Mano-P inspired visual page analysis)
    Analyze {
        /// URL to analyze
        url: String,
        /// Run visual diff against another URL
        #[arg(long)]
        diff: Option<String>,
        /// Output analysis as JSON
        #[arg(long)]
        json: bool,
        /// Screenshots output directory
        #[arg(long)]
        output: Option<String>,
    },
    /// Browse a URL with natural language task (Browser-use / Webwright pattern)
    Browse {
        /// URL to browse
        url: String,
        /// Natural language task description
        task: String,
        /// Screenshots output directory
        #[arg(long)]
        output: Option<String>,
        /// Enable LLM-based action planning
        #[arg(long)]
        llm: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Show current configuration
    Show,
    /// Set a configuration value
    Set {
        /// Configuration key (e.g., "orchestration.strategy")
        key: String,
        /// New value
        value: String,
    },
    /// Initialize default configuration
    Init {
        /// Force overwrite existing config
        #[arg(long)]
        force: bool,
    },
    /// Set API key for a provider
    SetApiKey {
        /// Provider name (e.g., "deepseek", "glm")
        provider: String,
        /// API key (omit to enter interactively)
        key: Option<String>,
    },
    /// Generate shell completions
    Completions {
        /// Shell: bash, zsh, fish, powershell
        shell: String,
    },
}

/// Company profile management (multi-project isolation).
#[derive(Subcommand, Debug)]
pub enum CompanyAction {
    /// List all company profiles
    List,
    /// Create a new company profile
    Init {
        /// Company name
        name: String,
        /// Optional display name
        #[arg(long)]
        display: Option<String>,
    },
    /// Switch to a different company
    Switch {
        /// Company name to switch to
        name: String,
    },
    /// Show active company info
    Show,
    /// Remove a company profile
    Remove {
        /// Company name to remove
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum EnterpriseCommand {
    /// Register as a compute node
    Connect {
        /// Enterprise server URL
        #[arg(long)]
        server: String,
        /// Auth token
        #[arg(long)]
        token: String,
        /// Node name (auto-generated if not set)
        #[arg(long)]
        name: Option<String>,
    },
    /// Disconnect from enterprise cluster
    Disconnect,
    /// Show node status
    Status,
    /// Show hardware info
    Hardware,
}

#[derive(Subcommand, Debug)]
pub enum SessionCommand {
    /// List all sessions
    List,
    /// Switch to a session
    Switch {
        /// Session ID
        id: String,
    },
    /// Delete a session
    Delete {
        /// Session ID
        id: String,
    },
    /// Show session info
    Info {
        /// Session ID
        id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleCommand {
    /// List all scheduled tasks
    List,
    /// Add a new scheduled task
    Add {
        /// Task name
        #[arg(long)]
        name: String,
        /// Schedule type: cron, interval, loop, event, once
        #[arg(long)]
        schedule_type: String,
        /// Cron expression (for --schedule-type cron), e.g., "*/5 * * * *"
        #[arg(long)]
        cron: Option<String>,
        /// Interval in seconds (for --schedule-type interval)
        #[arg(long)]
        interval_secs: Option<u64>,
        /// Loop condition / stop condition (for --schedule-type loop)
        #[arg(long)]
        until: Option<String>,
        /// Max iterations for loop tasks
        #[arg(long)]
        max_iterations: Option<u32>,
        /// Event name to listen for (for --schedule-type event)
        #[arg(long)]
        event: Option<String>,
        /// The prompt/task description for the agent to execute
        prompt: String,
    },
    /// Manually run a task by ID
    Run {
        /// Task ID
        id: String,
    },
    /// Remove a task by ID
    Remove {
        /// Task ID
        id: String,
    },
    /// Show execution history
    History,
    /// Trigger an event (runs all event-listening tasks)
    Trigger {
        /// Event name
        event_name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum BenchmarkCommand {
    /// Run SWE-bench evaluation
    Run {
        /// Path to dataset JSON file
        #[arg(long, default_value = "./swe-data")]
        dataset: String,
        /// Max instances to run
        #[arg(long)]
        max: Option<usize>,
        /// Dry-run mode (validate format only)
        #[arg(long)]
        dry_run: bool,
        /// Generate sample dataset for testing
        #[arg(long)]
        sample: Option<usize>,
    },
    /// Show last benchmark report
    Report,
}

/// Manage archived loop runs.
#[derive(Subcommand, Debug)]
pub enum ArchiveCommand {
    /// List all archived runs
    List,
    /// Show details of a specific archived run
    Show {
        /// Run ID to show
        id: String,
    },
    /// Delete an archived run
    Delete {
        /// Run ID to delete
        id: String,
    },
    /// Purge archives older than N days
    Purge {
        /// Delete archives older than this many days
        #[arg(long, default_value = "90")]
        days: u64,
    },
    /// Generate a data-driven engineering retrospective (gstack /retro)
    Retro,
}
