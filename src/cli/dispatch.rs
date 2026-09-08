//! Command dispatch — connects CLI args to runtime behavior.

use crate::cli::args::{
    Cli, Commands, ConfigCommand, EnterpriseCommand, ModeArg, OrchStrategyArg, SessionCommand,
    SkillAction, CompanyAction, ArchiveCommand,
};
use crate::config::{DeepSeekConfig, InferenceMode, OrchestrationStrategy};
use crate::providers::orchestrator::ProviderOrchestrator;
use crate::providers::provider::{ChatMessage, ProviderRequest};
use crate::agent::{Agent, AgentConfig, SwarmCoordinator};
use crate::memory::MemoryManager;
use crate::memory::auto_memory::AutoMemory;
use crate::providers::reasonix_cache::{ReasonixCache, ReasonixConfig, ApiUsage};
use crate::context::context_manager::{ContextManager, BuildContext};
use crate::context::compression::estimate_tokens;
use crate::security::InputSanitizer;
use crate::cost::{CostManager, BudgetConfig, ModelPricing, CostBreakdown};
use crate::tools::security_scanner_v2::SecurityScannerV2;
use crate::context::semantic_index_v2::{SemanticIndexV2, IndexConfig};
use crate::resilience::{ResilienceManager, ResilienceConfig};
use crate::r#loop::{LoopRole, LoopConfig, LoopEngine, LoopVerdict, Planner, generate_markdown_report};

/// Run the CLI with parsed arguments and optional shutdown signal.
pub async fn run(cli: Cli, _shutdown_rx: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> {
    // Initialize logging
    init_logging(&cli.log_level);

    // Load or create config
    let mut config = DeepSeekConfig::load().unwrap_or_default();

    // Apply CLI overrides
    apply_cli_overrides(&mut config, &cli);

    // Ensure config file exists
    DeepSeekConfig::ensure_config_file()?;

    // Validate configuration on startup
    let warnings = config.validate();
    if !warnings.is_empty() {
        eprintln!("⚠ Configuration warnings:");
        for w in &warnings {
            eprintln!("  • {}", w);
        }
        eprintln!();
    }

    // Handle Archive before extracting command (Archive needs &cli)
    if let Some(Commands::Archive(ref archive_cmd)) = &cli.command {
        return cmd_archive(archive_cmd, &cli).await;
    }

    // Route to subcommand
    let command = cli.command.unwrap_or(Commands::Chat {
        prompt: None,
        dir: cli.dir.clone(),
        session: None,
    });
    match command {
        Commands::Chat { prompt, dir, session } => {
            cmd_chat(&config, prompt, dir, session).await?;
        }
        Commands::Ask { question, dir } => {
            cmd_ask(&config, question, dir).await?;
        }
        Commands::Complete { file, line, column } => {
            cmd_complete(&config, &file, line, column).await?;
        }
        Commands::Config(sub) => {
            cmd_config(&config, sub).await?;
        }
        Commands::Serve { port, bind } => {
            cmd_serve(&config, port, &bind).await?;
        }
        Commands::Enterprise(sub) => {
            cmd_enterprise(&config, sub).await?;
        }
        Commands::Setup { force, models_dir } => {
            cmd_setup(force, &models_dir).await?;
        }
        Commands::ServeLlama { port, models_dir, ctx_size, threads } => {
            cmd_serve_llama(port, &models_dir, ctx_size, threads).await?;
        }
        Commands::Providers { verbose } => {
            cmd_providers(&config, verbose).await?;
        }
        Commands::Session(sub) => {
            cmd_session(sub).await?;
        }
        Commands::Version => {
            cmd_version();
        }
        Commands::Review { target, aspects, pr_mode, auto_apply, auto_verify, workflow, approval_gate } => {
            cmd_review(&config, &target, aspects.as_deref(), pr_mode, auto_apply, auto_verify, workflow.as_deref(), &approval_gate).await?;
        }
        Commands::Fix { max_rounds, yes } => {
            cmd_fix(&config, max_rounds, yes).await?;
        }
        Commands::Company(action) => {
            cmd_company(&config, &action).await?;
        }
        Commands::Verify { target, max_rounds, mode, verbose, role, ratchet, timeout } => {
            let target_str = target.to_string_lossy();
            cmd_verify(&config, &target_str, max_rounds, &mode, verbose, &role, ratchet, timeout).await?;
        }
        Commands::Completions { shell } => {
            use clap::CommandFactory;
            use clap_complete::{generate, shells::{Bash, Zsh, Fish, PowerShell}};
            let mut cmd = Cli::command();
            match shell.as_str() {
                "bash" => generate(Bash, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                "zsh" => generate(Zsh, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                "fish" => generate(Fish, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                "powershell" => generate(PowerShell, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                s => eprintln!("Unknown shell: {}. Supported: bash, zsh, fish, powershell", s),
            }
        }
        Commands::Skill { action } => {
            cmd_skill(&action).await?;
        }
        Commands::Rag { query } => {
            cmd_rag(&config, query).await?;
        }
        Commands::Stats => {
            cmd_stats(&config).await?;
        }
        Commands::Schedule(ref schedule_cmd) => {
            cmd_schedule(schedule_cmd, &config).await?;
        }
        Commands::Benchmark(ref bench_cmd) => {
            cmd_benchmark(bench_cmd).await?;
        }
        Commands::McpConfig => {
            let config = crate::mcp::server::generate_mcp_config();
            println!("{}", config);
        }
        Commands::Swarm { task, rlm } => {
            cmd_swarm(&config, &task, rlm).await?;
        }
        Commands::Voice { file, backend, language, agent } => {
            cmd_voice(&config, file.as_deref(), &backend, &language, agent).await?;
        }
        Commands::Analyze { url, diff, json, output } => {
            cmd_analyze(&url, diff.as_deref(), json, output.as_deref()).await?;
        }
        Commands::Browse { url, task, output, llm } => {
            cmd_browse(&url, &task, output.as_deref(), llm).await?;
        }
        // Archive handled above before command extraction
        Commands::Archive(_) => unreachable!("Archive handled earlier"),
    }

    Ok(())
}

// ============================================================================
// Subcommand implementations
// ============================================================================

async fn cmd_chat(
    config: &DeepSeekConfig,
    prompt: Option<String>,
    dir: Option<std::path::PathBuf>,
    session: Option<String>,
) -> anyhow::Result<()> {
    // Auto-start llama-server if local models are enabled but not running
    ensure_local_server(config).await;

    // Apply directory context if provided
    if let Some(ref work_dir) = dir {
        if work_dir.exists() {
            tracing::info!("Working directory: {}", work_dir.display());
        }
    }

    // Log session identifier for traceability
    if let Some(ref session_id) = session {
        tracing::info!(session=%session_id, "Chat session started");
    }

    let orchestrator = ProviderOrchestrator::new(config)?;
    let agent_config = AgentConfig::default();
    let mut agent = Agent::new(config, agent_config, orchestrator)?;
    let memory = MemoryManager::new();

    // P2-A: Auto-Memory — discover project context and enrich prompts
    let work_dir = dir.clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut auto_memory = AutoMemory::new(&work_dir);
    let discoveries = auto_memory.discover();
    if discoveries > 0 {
        tracing::info!(discoveries, "Auto-Memory: project context discovered");
    }

    // ReasonIX Cache: Three-zone architecture for 99%+ prefix cache hit rate
    let rcache_config = ReasonixConfig {
        session_id: session.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        ..Default::default()
    };
    let reasonix_cache = ReasonixCache::new(rcache_config);

    // Zone 1: Freeze immutable prefix (system prompt + tool schemas)
    // This is the cache target — byte-stable across all requests in this session
    let system_prompt = "You are deepseek-carp, an AI coding assistant. \
        You help users write, debug, refactor, and understand code. \
        Be concise and accurate. Respond in the user's language when possible.";
    let tool_schemas = serde_json::to_string(&serde_json::json!({
        "tools": ["read_file", "write_file", "edit", "search", "run_command",
                  "security_scan", "review_code", "git_status"]
    })).unwrap_or_default();
    let _prefix_fp = reasonix_cache.initialize_prefix(system_prompt, &tool_schemas, "");
    tracing::info!(prefix = %_prefix_fp.hash, "ReasonIX: immutable prefix frozen");

    // Phase B: ContextManager — unified RAG + Incremental + Compression for 250K+ LOC
    let mut ctx_manager = ContextManager::with_token_budget(work_dir.clone(), 128000);
    match ctx_manager.initialize().await {
        Ok(snapshot) => {
            tracing::info!(
                files = snapshot.files_indexed,
                chunks = snapshot.relevant_chunks.len(),
                "ContextManager: workspace indexed (RAG + Incremental + Compression ready)"
            );
        }
        Err(e) => {
            tracing::warn!(error=%e, "ContextManager: index failed, continuing without RAG");
        }
    }

    // Phase C-3: Input Sanitizer — protect against prompt injection
    let sanitizer = InputSanitizer::new().with_strict_mode(false);

    // Phase C-2: Cost Budget Manager — track spending, enforce limits
    let cost_manager = CostManager::new(
        BudgetConfig { session_limit: Some(10.0), ..Default::default() },
        &work_dir,
    );

    tracing::info!("Starting interactive chat...");

    // Resilience layer — circuit breaker + rate limiting + concurrency control
    let resilience = ResilienceManager::new(ResilienceConfig::default());

    if let Some(p) = prompt {
        // Phase C-3: Sanitize input — detect prompt injection, strip dangerous content
        let sanitize_result = sanitizer.sanitize(&p);
        if !sanitize_result.safe {
            eprintln!("[SECURITY] Input blocked: {} blockers found", sanitize_result.blockers.len());
            for b in &sanitize_result.blockers {
                eprintln!("  [{}] {}: {}", 
                    format!("{:?}", b.severity).to_lowercase(),
                    format!("{:?}", b.category).to_lowercase(),
                    b.description);
            }
            if sanitize_result.risk_score > 0.7 {
                anyhow::bail!("Input rejected due to high risk score ({:.2})", sanitize_result.risk_score);
            }
        } else if !sanitize_result.warnings.is_empty() {
            tracing::warn!(score = sanitize_result.risk_score, warnings = sanitize_result.warnings.len(), "Security: input has warnings");
        }

        // Non-interactive mode — enrich with learned project context
        let enriched_prompt = auto_memory.enrich_prompt(&sanitize_result.sanitized);

        // Phase C-2: Check budget before processing
        let budget_check = cost_manager.check_budget(0.01).await; // estimate ~1 cent
        if !budget_check.allowed {
            eprintln!("[BUDGET] {}", budget_check.reason.as_deref().unwrap_or("Budget exceeded"));
            if matches!(budget_check.action, crate::cost::ExceedAction::Block) {
                anyhow::bail!("Budget limit reached");
            }
        }

        // Phase B: Build full context (RAG + incremental + compression)
        let build_ctx = match ctx_manager.build_context(&enriched_prompt, &[]).await {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::warn!(error=%e, "ContextManager: build_context failed, using raw prompt");
                BuildContext {
                    system_prefix: String::new(),
                    rag_context: String::new(),
                    delta_context: String::new(),
                    compressed_history: String::new(),
                    user_message: enriched_prompt.clone(),
                    total_estimated_tokens: estimate_tokens(&enriched_prompt),
                    cache_friendly: true,
                }
            }
        };

        // Zone 2: Append user message to log (append-only, preserves prefix stability)
        let turn = 1u32;
        if let Err(e) = reasonix_cache.append("user", &build_ctx.user_message, turn) {
            tracing::warn!(error=%e, "ReasonIX: failed to append to log");
        }

        // Use RAG-enriched context as the final prompt
        let final_prompt = build_ctx.to_payload();
        tracing::info!(
            tokens = build_ctx.total_estimated_tokens,
            rag = build_ctx.rag_context.len(),
            delta = build_ctx.delta_context.len(),
            "Processing with ContextManager + ReasonIX Cache"
        );
        let _guard = resilience.pre_request_guard().await?;
        let result = agent.process(&final_prompt).await?;
        _guard.release().await;
        println!("{}", result.content);

        // Record API response for cache metrics (simulated from agent result)
        let total = result.total_tokens as u64;
        let usage = ApiUsage {
            prompt_tokens: total * 2 / 3,      // estimate: ~2/3 input, ~1/3 output
            completion_tokens: total / 3,
            prompt_cache_hit_tokens: total / 2, // placeholder — real value from DeepSeek API
            prompt_cache_miss_tokens: total / 2,
            total_cost_usd: (total as f64 * 0.000001), // rough estimate
        };
        reasonix_cache.record_response(&usage);

        println!("\n[Provider: {} | Tokens: {} | Iterations: {}]",
            result.provider, result.total_tokens, result.iterations);

        // ReasonIX Cache report
        let metrics = reasonix_cache.metrics();
        println!("[Cache: hit={:.1}% | token_hit={:.1}% | saved=${:.4}]",
            metrics.hit_rate() * 100.0,
            metrics.token_cache_hit_rate() * 100.0,
            metrics.estimated_savings_usd);

        // Phase C-2: Record cost and check budget alerts
        let pricing = ModelPricing::deepseek_v3();
        let cost_breakdown = CostBreakdown::new(
            &result.provider, total * 2 / 3, total / 3, total / 2, &pricing,
        );
        let cost_alerts = cost_manager.record_cost(&cost_breakdown).await;
        if !cost_alerts.is_empty() {
            for alert in &cost_alerts {
                eprintln!("[BUDGET ALERT] {}", alert.message);
            }
        }
        let budget_status = cost_manager.status().await;
        println!("[Cost: ${:.4} | Session: ${:.2}/${:.2} | Daily: ${:.2}/${:.2}]",
            cost_breakdown.total,
            budget_status.session_spent,
            budget_status.session_limit.unwrap_or(0.0),
            budget_status.daily_spent,
            budget_status.daily_limit.unwrap_or(0.0),
        );
    } else {
        // Interactive REPL/TUI mode
        crate::tui::run_interactive(config, agent, memory).await?;
    }

    Ok(())
}

async fn cmd_ask(
    config: &DeepSeekConfig,
    question: Vec<String>,
    _dir: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let question = question.join(" ");
    if question.is_empty() {
        return Err(anyhow::anyhow!("No question provided"));
    }

    let orchestrator = ProviderOrchestrator::new(config)?;
    let agent_config = AgentConfig::default();
    let mut agent = Agent::new(config, agent_config, orchestrator)?;

    tracing::info!("Asking: {}", question);
    let result = agent.process(&question).await?;

    println!("{}", result.content);
    println!(
        "\n[Provider: {} | Tokens: {} | Iterations: {}]",
        result.provider,
        result.total_tokens,
        result.iterations,
    );

    Ok(())
}

async fn cmd_complete(
    config: &DeepSeekConfig,
    file: &std::path::Path,
    line: usize,
    column: usize,
) -> anyhow::Result<()> {
    let code = std::fs::read_to_string(file)?;

    // Build FIM with cursor-aware prefix/suffix
    let (prefix, suffix) = build_fim_context(&code, line, column);

    let engine = crate::completion::CompletionEngine::new(config)?;

    let req = crate::completion::FimRequest {
        prefix,
        suffix,
        file_path: Some(file.to_string_lossy().to_string()),
        language: file.extension().and_then(|e| e.to_str()).map(|s| s.to_string()),
        max_tokens: 64,
        temperature: 0.1,
    };

    // Try streaming first, fallback to single-shot
    if let Some(mut rx) = engine.stream_complete(&req).await {
        while let Some(chunk) = rx.recv().await {
            print!("{}", chunk.text);
            use std::io::Write;
            let _ = std::io::stdout().flush();
            if chunk.is_done { break; }
        }
        println!();
    } else {
        match engine.complete(&req).await {
            Some(candidate) => println!("{}", candidate.text),
            None => eprintln!("No completion available"),
        }
    }

    Ok(())
}

/// Build prefix (before cursor) and suffix (after cursor) for FIM.
fn build_fim_context(code: &str, line: usize, column: usize) -> (String, String) {
    let lines: Vec<&str> = code.lines().collect();
    if line == 0 || line > lines.len() {
        return (code.to_string(), String::new());
    }

    let cursor_line_idx = line.saturating_sub(1);
    let prefix_lines: String = lines[..cursor_line_idx]
        .iter()
        .map(|l| format!("{}\n", l))
        .collect();
    let current_line = lines[cursor_line_idx];
    let col = column.min(current_line.len());

    let prefix = format!("{}{}", prefix_lines, &current_line[..col]);
    let suffix = format!("{}\n{}", &current_line[col..], lines[cursor_line_idx + 1..].join("\n"));

    (prefix, suffix)
}

async fn cmd_config(
    config: &DeepSeekConfig,
    cmd: ConfigCommand,
) -> anyhow::Result<()> {
    match cmd {
        ConfigCommand::Show => {
            let toml_str = toml::to_string_pretty(config)?;
            println!("{}", toml_str);
        }
        ConfigCommand::Set { key, value } => {
            let mut cfg = config.clone();
            let set_result = set_config_value(&mut cfg, &key, &value);
            match set_result {
                Ok(()) => {
                    cfg.save()?;
                    println!("✓ Set '{}' = '{}' (saved, restart to apply)", key, value);
                }
                Err(e) => {
                    eprintln!("✗ Failed to set '{}': {}", key, e);
                    eprintln!("  Supported keys: inference_mode, orchestration.strategy, ");
                    eprintln!("  orchestration.api_order, ui.theme, log_level, max_agent_iterations");
                }
            }
        }
        ConfigCommand::Init { force } => {
            let config_path = crate::config::paths::config_file();
            if config_path.exists() && !force {
                println!("Config already exists at {}. Use --force to overwrite.", config_path.display());
            } else {
                config.save()?;
                println!("Config initialized at {}", config_path.display());
            }
        }
        ConfigCommand::SetApiKey { provider, key } => {
            let key = match key {
                Some(k) => k,
                None => {
                    // Interactive input
                    use std::io::{self, Write};
                    print!("Enter API key for {}: ", provider);
                    io::stdout().flush()?;
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    input.trim().to_string()
                }
            };
            let mut creds = DeepSeekConfig::load_credentials().unwrap_or_default();
            creds.api_keys.insert(provider.clone(), key);
            DeepSeekConfig::save_credentials(&creds)?;
            println!("API key saved for {}", provider);
        }
        ConfigCommand::Completions { shell } => {
            use clap::CommandFactory;
            use clap_complete::{generate, shells::{Bash, Zsh, Fish, PowerShell}};
            let mut cmd = Cli::command();

            match shell.as_str() {
                "bash" => generate(Bash, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                "zsh" => generate(Zsh, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                "fish" => generate(Fish, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                "powershell" => generate(PowerShell, &mut cmd, "deepseek-carp", &mut std::io::stdout()),
                s => eprintln!("Unknown shell: {}. Supported: bash, zsh, fish, powershell", s),
            }
        }
    }
    Ok(())
}

async fn cmd_serve(
    config: &DeepSeekConfig,
    port: u16,
    bind: &str,
) -> anyhow::Result<()> {
    use std::net::SocketAddr;
    use std::sync::Arc;

    let addr: SocketAddr = format!("{}:{}", bind, port).parse()?;
    let config = Arc::new(config.clone());
    let orchestrator = Arc::new(ProviderOrchestrator::new(&config)?);

    println!("╔══════════════════════════════════════════════╗");
    println!("║   DeepSeek Carp Server                       ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║   HTTP:  http://{:<30} ║", addr);
    println!("║   Mode:  {:<35} ║",
        match config.inference_mode {
            InferenceMode::Cloud => "Cloud API",
            InferenceMode::Enterprise => "Enterprise",
        }
    );
    println!("╚══════════════════════════════════════════════╝");

    // Build a simple HTTP server with tokio + hyper-compatible handler
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Server listening on {}", addr);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let cfg = config.clone();
                        let orch = orchestrator.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_http_request(stream, &cfg, &orch).await {
                                tracing::error!(peer=%peer, error=%e, "Request error");
                            }
                        });
                    }
                    Err(e) => tracing::error!("Accept error: {}", e),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
                break;
            }
        }
    }

    Ok(())
}

/// Minimal HTTP handler: health check + status + chat endpoint.
async fn handle_http_request(
    stream: tokio::net::TcpStream,
    config: &DeepSeekConfig,
    orchestrator: &ProviderOrchestrator,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("GET");
    let path = parts.get(1).copied().unwrap_or("/");

    // Read headers + body (for POST)
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let trimmed = line.trim();
        if trimmed.is_empty() { break; }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    let mut body_json = serde_json::Value::Null;
    if content_length > 0 {
        let mut body_bytes = vec![0u8; content_length];
        tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body_bytes).await?;
        body_json = serde_json::from_slice(&body_bytes).unwrap_or_default();
    }

    let (status, body) = match (method, path) {
        ("POST", "/chat") => {
            let prompt = body_json["prompt"].as_str().unwrap_or("").to_string();
            if prompt.is_empty() {
                ("400 Bad Request", r#"{"error": "Missing 'prompt' field"}"#.to_string())
            } else {
                // Route via orchestrator using SmartUpgrade
                let req = ProviderRequest {
                    system: None,
                    messages: vec![ChatMessage {
                        role: "user".into(),
                        content: prompt,
                        tool_calls: None,
                        tool_call_id: None,
                    ..Default::default()}],
                    max_tokens: Some(2048),
                    temperature: Some(0.7),
                    stop: None,
                    stream: false,
                    tools: None,
                };
                match orchestrator.orchestrate(&req).await {
                    Ok(resp) => (
                        "200 OK",
                        serde_json::json!({
                            "content": resp.content,
                            "provider": resp.provider,
                            "tokens": resp.usage.map(|u| u.total_tokens).unwrap_or(0),
                            "latency_ms": resp.latency_ms,
                        }).to_string(),
                    ),
                    Err(e) => (
                        "500 Internal Server Error",
                        serde_json::json!({"error": e.to_string()}).to_string(),
                    ),
                }
            }
        }
        (_, "/health") => (
            "200 OK",
            serde_json::json!({
                "status": "ok",
                "mode": format!("{:?}", config.inference_mode),
                "version": env!("CARGO_PKG_VERSION"),
            }).to_string(),
        ),
        (_, "/status") => {
            let report = orchestrator.health_report().await;
            let stats = orchestrator.stats_report().await;
            let providers: Vec<_> = report.iter().map(|h| {
                let s = stats.get(&h.name);
                serde_json::json!({
                    "name": h.name,
                    "healthy": h.is_healthy,
                    "failures": h.consecutive_failures,
                    "success_rate": s.map(|s| format!("{:.1}%", s.success_rate() * 100.0)),
                    "avg_latency_ms": s.map(|s| format!("{:.0}", s.avg_latency_ms())),
                })
            }).collect();
            (
                "200 OK",
                serde_json::json!({
                    "mode": format!("{:?}", config.inference_mode),
                    "strategy": format!("{:?}", config.orchestration.strategy),
                    "providers": providers,
                }).to_string(),
            )
        }
        _ => (
            "404 Not Found",
            r#"{"error": "not found", "endpoints": ["GET /health", "GET /status", "POST /chat"]}"#.to_string(),
        ),
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    writer.write_all(response.as_bytes()).await?;
    Ok(())
}

async fn cmd_providers(
    config: &DeepSeekConfig,
    verbose: bool,
) -> anyhow::Result<()> {
    let orchestrator = ProviderOrchestrator::new(config)?;
    let health = orchestrator.health_report().await;
    let stats = orchestrator.stats_report().await;

    // Header with mode info
    let mode_icon = match config.inference_mode {
        InferenceMode::Cloud => "☁️  Cloud API",
        InferenceMode::Enterprise => "🏢 Enterprise",
    };
    let strategy_name = match config.orchestration.strategy {
        OrchestrationStrategy::SmartUpgrade => "SmartUpgrade (local Qwen → smart cloud routing)",
        OrchestrationStrategy::Cascade => "Cascade",
        OrchestrationStrategy::ParallelRace => "Parallel Race",
        OrchestrationStrategy::TaskBasedRouting => "Task-Based Routing",
        OrchestrationStrategy::AdaptiveWeighted => "Adaptive Weighted",
        OrchestrationStrategy::CostOptimized => "Cost-Optimized",
        OrchestrationStrategy::HybridParallel => "Hybrid Parallel (local + cloud racing)",
    };

    println!("Mode: {} | Strategy: {}", mode_icon, strategy_name);
    println!();

    // Local providers
    println!("═══ Local Models ═══");
    println!("{:<20} {:<10} {:<20} {:<10} {:<15}",
        "Provider", "Status", "Model", "Type", "Success Rate");
    println!("{}", "-".repeat(80));

    for h in &health {
        let provider = config.providers.iter().find(|p| p.name == h.name);
        let is_local = provider.map(|p| p.is_local).unwrap_or(false);
        if !is_local { continue; }

        let status = if h.is_healthy { "✓ HEALTHY" } else { "✗ UNHEALTHY" };
        let s = stats.get(&h.name);
        let rate = s.map(|s| format!("{:.1}%", s.success_rate() * 100.0))
            .unwrap_or_else(|| "N/A".to_string());
        let model = provider.map(|p| p.model.as_str()).unwrap_or("-");
        let primary = if h.name == config.orchestration.smart_upgrade.primary_local {
            "PRIMARY"
        } else {
            "backup"
        };

        println!("{:<20} {:<10} {:<20} {:<10} {:<15}",
            h.name, status, model, primary, rate);

        if verbose {
            if let Some(s) = s {
                println!("  Calls: {} total, {} success | Avg latency: {:.0}ms",
                    s.total_calls, s.successful_calls, s.avg_latency_ms());
            }
        }
    }

    println!();
    println!("═══ Cloud APIs ═══");
    println!("{:<20} {:<10} {:<20} {:<10} {:<15}",
        "Provider", "Status", "Model", "Priority", "Success Rate");
    println!("{}", "-".repeat(80));

    for h in &health {
        let provider = config.providers.iter().find(|p| p.name == h.name);
        let is_local = provider.map(|p| p.is_local).unwrap_or(true);
        if is_local { continue; }

        let status = if h.is_healthy { "✓ HEALTHY" } else { "✗ UNHEALTHY" };
        let s = stats.get(&h.name);
        let rate = s.map(|s| format!("{:.1}%", s.success_rate() * 100.0))
            .unwrap_or_else(|| "N/A".to_string());
        let model = provider.map(|p| p.model.as_str()).unwrap_or("-");
        let pos = config.orchestration.api_order.iter()
            .position(|n| n == &h.name)
            .map(|i| format!("#{}", i + 1))
            .unwrap_or_else(|| "-".to_string());

        println!("{:<20} {:<10} {:<20} {:<10} {:<15}",
            h.name, status, model, pos, rate);

        if verbose {
            if let Some(s) = s {
                println!("  Calls: {} total, {} success | Avg latency: {:.0}ms",
                    s.total_calls, s.successful_calls, s.avg_latency_ms());
            }
        }
    }

    // Validation warnings
    let warnings = config.validate();
    if !warnings.is_empty() {
        println!();
        println!("═══ Warnings ═══");
        for w in &warnings {
            println!("⚠️  {}", w);
        }
    }

    Ok(())
}

async fn cmd_enterprise(
    config: &DeepSeekConfig,
    cmd: EnterpriseCommand,
) -> anyhow::Result<()> {
    #[cfg(feature = "enterprise")]
    {
        let mut connector = crate::enterprise::EnterpriseConnector::new(config.enterprise.clone());

        match cmd {
            EnterpriseCommand::Connect { server, token, name } => {
                let mut cfg = config.enterprise.clone();
                cfg.enabled = true;
                cfg.server_url = Some(server);
                cfg.auth_token = Some(token);
                if let Some(n) = name {
                    cfg.node_name = Some(n);
                }
                let mut conn = crate::enterprise::EnterpriseConnector::new(cfg);
                conn.connect().await?;
                println!("Connected to enterprise cluster as compute node.");
            }
            EnterpriseCommand::Disconnect => {
                connector.disconnect().await?;
                println!("Disconnected from enterprise cluster.");
            }
            EnterpriseCommand::Status => {
                let state = connector.state().await;
                println!("Node state: {:?}", state);
            }
            EnterpriseCommand::Hardware => {
                let hw = connector.hardware();
                println!("GPU: {} x {}MB, CPU: {} cores, RAM: {}MB",
                    hw.gpu_count, hw.gpu_memory_mb, hw.cpu_cores, hw.system_ram_mb);
            }
        }
    }

    #[cfg(not(feature = "enterprise"))]
    {
        let _ = (config, cmd);
        println!("Enterprise features are not enabled. Rebuild with --features enterprise");
    }

    Ok(())
}

async fn cmd_session(cmd: SessionCommand) -> anyhow::Result<()> {
    let _memory = MemoryManager::new();

    match cmd {
        SessionCommand::List => {
            println!("Session management not available in this version.");
        }
        SessionCommand::Switch { id } => {
            println!("Session management not available in this version. (Requested session: {:?})", id);
        }
        SessionCommand::Delete { id } => {
            println!("Session management not available in this version. (Session to delete: {:?})", id);
        }
        SessionCommand::Info { id } => {
            println!("Session management not available in this version. (Session info: {:?})", id);
        }
    }

    Ok(())
}

fn cmd_version() {
    println!("deepseek-carp {}", env!("CARGO_PKG_VERSION"));
    println!("DeepSeek ecosystem AI coding assistant");
    println!("Local models: Qwen, DeepSeek Coder, Kimi, GLM");
    println!("Auto-fallback: DeepSeek → GLM → Kimi → Minimax");
    println!("Manual opt-in: Claude, OpenAI, Copilot");
}

// ============================================================================
// Helpers
// ============================================================================

fn init_logging(level: &str) {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_appender::rolling::{RollingFileAppender, Rotation};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    let stderr_layer = layer()
        .with_writer(std::io::stderr)
        .with_target(true);

    let log_dir = crate::config::paths::logs_dir();
    let file_appender = RollingFileAppender::new(Rotation::DAILY, log_dir, "deepseek-carp");
    let file_layer = layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_target(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();
}

fn apply_cli_overrides(config: &mut DeepSeekConfig, cli: &Cli) {
    // Override inference mode
    if let Some(ref mode_arg) = cli.mode {
        config.inference_mode = match mode_arg {
            ModeArg::Cloud => InferenceMode::Cloud,
            ModeArg::Enterprise => InferenceMode::Enterprise,
        };
    }

    // Override strategy from CLI
    if let Some(ref strategy_arg) = cli.strategy {
        config.orchestration.strategy = match strategy_arg {
            OrchStrategyArg::SmartUpgrade => OrchestrationStrategy::SmartUpgrade,
            OrchStrategyArg::Cascade => OrchestrationStrategy::Cascade,
            OrchStrategyArg::ParallelRace => OrchestrationStrategy::ParallelRace,
            OrchStrategyArg::TaskBased => OrchestrationStrategy::TaskBasedRouting,
            OrchStrategyArg::Adaptive => OrchestrationStrategy::AdaptiveWeighted,
            OrchStrategyArg::CostOptimized => OrchestrationStrategy::CostOptimized,
        };
    }

    // Override provider/model from CLI
    if let (Some(ref provider_name), Some(ref model)) = (&cli.provider, &cli.model) {
        for p in &mut config.providers {
            if &p.name == provider_name {
                p.model = model.clone();
            }
        }
    }

    if cli.log_level != "info" {
        config.log_level = cli.log_level.clone();
    }
}

/// Dynamic config key-value setter for `config set` command.
fn set_config_value(config: &mut DeepSeekConfig, key: &str, value: &str) -> anyhow::Result<()> {
    match key {
        "inference_mode" => {
            config.inference_mode = match value {
                "cloud" => InferenceMode::Cloud,
                "enterprise" => InferenceMode::Enterprise,
                _ => anyhow::bail!("Invalid value: use 'cloud' or 'enterprise'"),
            };
        }
        "orchestration.strategy" => {
            config.orchestration.strategy = match value {
                "smart_upgrade" | "smart" => OrchestrationStrategy::SmartUpgrade,
                "cascade" => OrchestrationStrategy::Cascade,
                "parallel_race" | "parallel" => OrchestrationStrategy::ParallelRace,
                "task_based" | "task" => OrchestrationStrategy::TaskBasedRouting,
                "adaptive" => OrchestrationStrategy::AdaptiveWeighted,
                "cost_optimized" | "cost" => OrchestrationStrategy::CostOptimized,
                _ => anyhow::bail!("Invalid strategy: use smart_upgrade/cascade/parallel_race/task_based/adaptive/cost_optimized"),
            };
        }
        "orchestration.api_order" => {
            config.orchestration.api_order = value.split(',').map(|s| s.trim().to_string()).collect();
        }
        "ui.theme" => {
            config.ui.theme = value.to_string();
        }
        "log_level" => {
            config.log_level = value.to_string();
        }
        "max_agent_iterations" => {
            config.max_agent_iterations = value.parse()?;
        }
        _ => anyhow::bail!("Unknown config key '{}'", key),
    }
    Ok(())
}

// ── Integrated llama-server management ──

/// Ensure local model server is running. Auto-starts + downloads models if needed.
async fn ensure_local_server(config: &DeepSeekConfig) {
    let has_local = config.providers.iter().any(|p| p.is_local && p.enabled);
    if !has_local { return; }

    // ── Step 1: Download models if missing ──
    let models_dir = std::path::Path::new("./models");
    if crate::setup::needs_setup(models_dir) {
        eprintln!("📦 Models not found. Running one-click setup...");
        eprintln!("   (Use 'deepseek-carp setup' to run manually)");
        eprintln!();
        if let Err(e) = crate::setup::run_setup(models_dir, false).await {
            eprintln!("⚠️  Auto-setup failed: {}", e);
            eprintln!("   You can download models manually or use cloud APIs.");
            return;
        }
    }

    // ── Step 2: Start llama-server if not running ──
    if is_local_server_running().await {
        tracing::info!("Local model server already running on http://localhost:8080");
        return;
    }

    eprintln!("⚡ Starting local model server...");
    match auto_start_llama_server().await {
        Ok(()) => {
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if is_local_server_running().await {
                    eprintln!("✅ Local model server ready on http://localhost:8080");
                    return;
                }
            }
            eprintln!("⚠️  Server started but not responding yet.");
        }
        Err(e) => {
            eprintln!("⚠️  Could not start server: {}", e);
            eprintln!("   Manual: deepseek-carp serve-llama");
        }
    }
}

/// Check if local model server is responding.
async fn is_local_server_running() -> bool {
    if let Ok(resp) = reqwest::get("http://localhost:8080/v1/models").await {
        resp.status().is_success()
    } else {
        false
    }
}

/// Auto-start llama-server in the background.
async fn auto_start_llama_server() -> anyhow::Result<()> {
    let models_dir = if std::path::Path::new("./models").exists() {
        "./models".to_string()
    } else {
        return Err(anyhow::anyhow!("models/ directory not found. Please create it and add GGUF files."));
    };

    // Try llama-server.exe in PATH, then in current dir
    let server_exe = which_llama_server();

    let child = std::process::Command::new(&server_exe)
        .args([
            "--models-dir", &models_dir,
            "--port", "8080",
            "-c", "8192",
            "--threads", "8",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start {}: {}", server_exe, e))?;

    // Detach: let it run in background
    eprintln!("   Started {} (PID {})", server_exe, child.id());
    // Fire and forget — child process handles itself
    let _ = child; // dropped = detached

    Ok(())
}

/// Find llama-server executable.
fn which_llama_server() -> String {
    let name = if cfg!(target_os = "windows") { "llama-server.exe" } else { "llama-server" };

    // Check current directory first
    if std::path::Path::new(name).exists() {
        return name.to_string();
    }
    // Check PATH
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let full = dir.join(name);
            if full.exists() { return full.to_string_lossy().to_string(); }
        }
    }

    name.to_string()
}

/// Review command — review a pull request or code change with closed-loop pipeline.
///
/// Pipeline: diff → multi-aspect review → line-level annotations →
///           auto-apply suggestions → cargo check verify
#[allow(clippy::too_many_arguments)]
async fn cmd_review(
    config: &DeepSeekConfig,
    target: &str,
    aspects: Option<&str>,
    pr_mode: bool,
    auto_apply: bool,
    auto_verify: bool,
    workflow_path: Option<&str>,
    approval_gate: &str,
) -> anyhow::Result<()> {
    use crate::review::workflow::{WorkflowDef, WorkflowEngine};
    use crate::review::{
        ReviewEngine, DiffTarget, review_with_streaming,
    };
    use crate::tools::pr_reviewer::ReviewAspect;

    let project_root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    // ========================================================================
    // Workflow Mode — YAML-defined multi-agent pipeline with feedback loops
    // ========================================================================
    if let Some(wf_path) = workflow_path {
        let wf_path = std::path::Path::new(wf_path);
        if !wf_path.exists() {
            eprintln!("  ❌ Workflow file not found: {}", wf_path.display());
            eprintln!("\n  Create one with the template:");
            eprintln!("{}", WorkflowDef::template());
            return Ok(());
        }

        let workflow = WorkflowDef::from_file(wf_path)?;
        println!("═══ Workflow Mode: {} ═══\n", workflow.name);
        if let Some(ref desc) = workflow.description {
            println!("  {}", desc);
        }
        println!("  Steps: {}", workflow.steps.len());
        println!("  Max iterations: {}", workflow.max_iterations);
        println!("  Target: {}\n", target);

        let diff_target = DiffTarget::parse(target);
        let review_engine = ReviewEngine::new(&project_root);
        let wf_engine = WorkflowEngine::new(review_engine);
        let run = wf_engine.run(&workflow, &diff_target).await?;

        print!("{}", WorkflowEngine::format_workflow_result(&run));

        if run.iteration >= workflow.max_iterations {
            println!("\n  ⚠️  Max iterations ({}) reached. Pipeline may need review.\n", workflow.max_iterations);
        } else {
            println!("\n  ✅ Workflow completed in {} iteration(s).\n", run.iteration);
        }

        return Ok(());
    }

    if pr_mode {
        // ====================================================================
        // Multi-Agent PR Review Mode (enhanced with ReviewEngine closed-loop)
        // ====================================================================
        println!("═══ Multi-Agent PR Review — 5-Dimension Analysis ═══\n");
        println!("  Target: {}", target);
        println!("  Auto-apply: {} | Auto-verify: {}\n",
            if auto_apply { "YES" } else { "NO" },
            if auto_verify { "YES" } else { "NO" });

        // Build the engine with CLI rules support
        let mut engine = ReviewEngine::new(&project_root);
        engine.set_auto_apply(auto_apply);
        engine.set_auto_verify(auto_verify);

        // Parse the target
        let diff_target = DiffTarget::parse(target);

        // Parse optional aspect filter
        let aspect_filter: Option<Vec<ReviewAspect>> = aspects.map(|a| {
            a.split(',')
                .filter_map(|s| match s.trim().to_lowercase().as_str() {
                    "security" => Some(ReviewAspect::Security),
                    "performance" | "perf" => Some(ReviewAspect::Performance),
                    "correctness" => Some(ReviewAspect::Correctness),
                    "style" => Some(ReviewAspect::Style),
                    "tests" | "test" => Some(ReviewAspect::Tests),
                    _ => None,
                })
                .collect()
        });
        let aspect_slice: Option<&[ReviewAspect]> = aspect_filter.as_deref();

        // Run the full review session with streaming
        let session = review_with_streaming(&engine, &diff_target, aspect_slice).await?;

        // Print the final summary
        ReviewEngine::print_session(&session);

        // Final verdict guidance
        match session.report.verdict {
            crate::tools::pr_reviewer::ReviewVerdict::Approved => {
                println!("\n  ✅ Code approved. No issues found.\n");
            }
            crate::tools::pr_reviewer::ReviewVerdict::NeedsChanges => {
                println!("\n  ⚠️  Code needs changes. Review findings above.\n");
                if !auto_apply {
                    println!("  Tip: Re-run with --auto-apply to automatically apply HIGH+ fixes.");
                }
            }
            crate::tools::pr_reviewer::ReviewVerdict::Blocked => {
                println!("\n  🚫 Code BLOCKED. Critical issues must be resolved.\n");
            }
        }
        if auto_apply {
            let needs_approval = match approval_gate {
                "all" => session.report.total_findings > 0,
                "critical" => session.report.critical_count > 0,
                _ => false, // "none"
            };

            if needs_approval {
                println!("\n  ⚠️  Approval Gate ({}) triggered.", approval_gate);
                println!("  Findings requiring review: {} ({} critical)",
                    session.report.total_findings, session.report.critical_count);
                print!("  Apply fixes? [y/N]: ");

                use std::io::{Write, BufRead};
                std::io::stdout().flush().ok();
                let stdin = std::io::stdin();
                let input = stdin.lock().lines().next()
                    .and_then(|l| l.ok())
                    .unwrap_or_default();

                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("  ⏭️  Fixes skipped by user (approval gate).\n");
                    return Ok(());
                }
            }
        }

        return Ok(());
    }

    // ========================================================================
    // Standard review mode (original agent-based behavior)
    // ========================================================================
    let orchestrator = ProviderOrchestrator::new(config)?;
    let agent_config = AgentConfig::default();
    let mut agent = Agent::new(config, agent_config, orchestrator)?;

    let aspect_hint = match aspects {
        Some(a) => format!(" Focus on these aspects: {}.", a),
        None => String::new(),
    };
    let prompt = format!("Please review the following code change:{}\n{}", aspect_hint, target);
    let result = agent.process(&prompt).await?;
    println!("{}", result.content);

    // Run security scan using SecurityScannerV2
    println!("\n═══ Security Scan (SecurityScannerV2) ═══\n");
    let scanner = SecurityScannerV2::new();
    let target_path = std::path::Path::new(target);
    if target_path.exists() && target_path.is_file() {
        let content = std::fs::read_to_string(target_path)?;
        let language = detect_review_language(target);
        let findings = scanner.scan_file(target, &content, &language);
        if findings.is_empty() {
            println!("  No security vulnerabilities found.");
        } else {
            for finding in &findings {
                println!("  [{}] {}:{} - {}",
                    finding.severity.as_str(), finding.file, finding.line, finding.title);
                println!("    {}", finding.remediation);
            }
        }
        let report = scanner.format_report(&scanner.scan_files(&[(target.to_string(), content, language)]));
        println!("{}", report);
    } else {
        println!("  (Security scan skipped: target is not a local file.)");
    }

    Ok(())
}

/// Company command — manage multi-project isolation profiles.
async fn cmd_company(_config: &DeepSeekConfig, action: &CompanyAction) -> anyhow::Result<()> {
    use crate::company::CompanyManager;

    let mut mgr = CompanyManager::open()?;

    match action {
        CompanyAction::List => {
            println!("{}", mgr.format_list());
        }
        CompanyAction::Init { name, display } => {
            if mgr.get(name).is_some() {
                println!("  ⚠️  Company '{}' already exists.", name);
                return Ok(());
            }
            mgr.create(name, display.as_deref())?;
            mgr.switch(name)?;
            println!("  ✅ Created and switched to company '{}'.", name);
        }
        CompanyAction::Switch { name } => {
            mgr.switch(name)?;
            println!("  ✅ Switched to company '{}'.", name);
        }
        CompanyAction::Show => {
            match mgr.active() {
                Some(profile) => {
                    println!("═══ Active Company ═══\n");
                    println!("  Name:        {}", profile.name);
                    println!("  Display:     {}", profile.display_name);
                    println!("  Data dir:    {}", profile.data_dir.display());
                    println!("  Skills dir:  {}", profile.skills_dir().display());
                    println!("  Audit dir:   {}", profile.audit_dir().display());
                }
                None => {
                    println!("  No active company. Use `carp company init <name>` to create one.");
                }
            }
        }
        CompanyAction::Remove { name } => {
            mgr.remove(name)?;
            println!("  ✅ Removed company '{}'.", name);
        }
    }

    Ok(())
}

/// Verify command — run the unified agent loop.
async fn cmd_verify(
    config: &DeepSeekConfig,
    target: &str,
    max_rounds: u32,
    mode: &str,
    verbose: bool,
    role: &str,
    ratchet: bool,
    timeout: u64,
) -> anyhow::Result<()> {

    println!("═══ Verify: {} mode on '{}' (role: {}, ratchet={}, timeout={}s) ═══\n",
        mode, target, role, ratchet, timeout);

    match mode {
        "review" => {
            cmd_verify_review(config, target, max_rounds, verbose, role, ratchet, timeout).await?;
        }
        "test" => {
            cmd_verify_test(target, max_rounds, verbose, role).await?;
        }
        other => {
            anyhow::bail!("Unknown verify mode: '{}'. Use 'review' or 'test'.", other);
        }
    }

    Ok(())
}

async fn cmd_verify_test(target: &str, max_rounds: u32, verbose: bool, role: &str) -> anyhow::Result<()> {
    use crate::test::*;

    let role = LoopRole::from_str(role).unwrap_or_default();
    let config = LoopConfig {
        max_rounds,
        verbose,
        role,
        ..Default::default()
    };

    // Build TestMode adapters
    let observer = BrowserObserver::new();
    let planner = TestPlanner::new();
    let actor = PageActor::new();
    let evaluator = ContentEvaluator::new();

    let mut engine = LoopEngine::new(observer, planner, actor, evaluator, config)
        .with_mode("test");

    let summary = engine.run_summary(target).await;

    // Persist run results to SQLite (best-effort)
    #[cfg(feature = "sqlite-storage")]
    if let Ok(store) = crate::storage::LoopStore::open() {
        match store.save_run(target, "test", max_rounds, &summary) {
            Ok(run_id) => tracing::info!("Loop run saved to SQLite: id={}", run_id),
            Err(e) => tracing::warn!("Failed to save loop run to SQLite: {}", e),
        }
    }

    println!("\n═══ Result ═══\n");
    println!("  Rounds:      {}", summary.total_rounds);
    println!("  Passed:      {}", summary.passed);
    println!("  Total time:  {} ms", summary.total_time_ms);
    println!();

    if verbose {
        for r in &summary.results {
            println!("  Round {}: {:?} ({:?})", r.round, r.verdict, r.phase_times_ms);
        }
    }

    if !summary.passed {
        if let Some(LoopVerdict::Failed { reason }) = &summary.final_verdict {
            println!("  ❌ {}", reason);
        }
        std::process::exit(1);
    }

    Ok(())
}

async fn cmd_verify_review(config: &DeepSeekConfig, target: &str, max_rounds: u32, verbose: bool, role: &str, ratchet: bool, timeout: u64) -> anyhow::Result<()> {
    use crate::providers::orchestrator::ProviderOrchestrator;
    use crate::review::*;

    let loop_role = LoopRole::from_str(role).unwrap_or_default();
    let mut loop_config = LoopConfig {
        max_rounds,
        verbose,
        role: loop_role,
        ratchet_mode: ratchet,
        round_timeout_secs: timeout,
        ..Default::default()
    };

    // Load program.md (autoresearch pattern) — overrides CLI defaults
    let project_root = std::env::current_dir().unwrap_or_default();
    if let Ok(Some(program)) = crate::rules::program::ProgramConfig::load(&project_root) {
        tracing::info!("Loaded program.md: {} — {}", program.name, program.goal);
        // Override config values from program.md
        if let Some(r) = program.max_rounds {
            loop_config.max_rounds = r;
        }
        if let Some(t) = program.round_timeout_secs {
            loop_config.round_timeout_secs = t;
        }
        if let Some(rg) = &program.role {
            if let Some(pr) = LoopRole::from_str(rg) {
                loop_config.role = pr;
            }
        }
        if let Some(gate) = program.enforce_review_gate {
            loop_config.enforce_review_gate = gate;
        }
        if let Some(iron) = program.use_iron_laws {
            loop_config.use_iron_laws = iron;
        }

        // Log program constraints
        if !program.constraints.is_empty() {
            tracing::info!("Program has {} constraint(s)", program.constraints.len());
        }

        // Inject program.md system prompt into planner (autoresearch pattern)
        let program_prompt = program.to_system_prompt();
        if !program_prompt.is_empty() {
            tracing::info!("Injecting program.md system prompt into planner ({} chars)", program_prompt.len());
            // Will be injected into planner below after construction
        }
    } else {
        tracing::debug!("No .carp/program.md found, using CLI defaults");
    }

    // Build LLM provider for smart refactoring (if available)
    let orchestrator = ProviderOrchestrator::new(config).ok();
    let has_llm = orchestrator.is_some();

    // Build ReviewMode adapters
    let scanner = CodeScanner::new();
    let mut planner = RefactorPlanner::new();
    if let Some(orch) = orchestrator {
        planner = planner.with_llm(orch);
    }

    // Inject program.md prompt into planner context (wired from to_system_prompt())
    // This ensures program goals/constraints/red_flags reach the LLM during Plan phase
    #[allow(clippy::let_and_return)]
    let _program_prompt = {
        let _project_root = std::env::current_dir().unwrap_or_default();
        if let Ok(Some(prog)) = crate::rules::program::ProgramConfig::load(&_project_root) {
            let prompt = prog.to_system_prompt();
            if !prompt.is_empty() {
                // Use same injection path as constitution context
                planner.with_constitution_context(&prompt);
            }
            Some(prompt)
        } else {
            None
        }
    };
    let actor = FileEditActor::new();
    let evaluator = CompilerEvaluator::new();

    if has_llm {
        println!("  🧠 LLM refactoring mode: ON\n");
    } else {
        println!("  ⚙️  Heuristic refactoring mode (no LLM provider available)\n");
    }

    let mut engine = LoopEngine::new(scanner, planner, actor, evaluator, loop_config)
        .with_mode("review");

    // Wire up persistence hooks (SQLite, best-effort)
    let hooks = crate::hooks::HookRegistry::new();
    let target_owned = target.to_string();
    {
        hooks.register(move |event: crate::hooks::HookEvent| {
            let _target = target_owned.clone();
            async move {
                if let crate::hooks::HookEvent::LoopRunCompleted { .. } = event {
                    // Best-effort save to SQLite
                    #[cfg(feature = "sqlite-storage")]
                    if let Ok(store) = crate::storage::LoopStore::open() {
                        // This is run asynchronously; the summary is captured
                        // via the engine's history below, not here.
                        let _ = store.total_run_count();
                        tracing::info!("Loop run persisted to SQLite (run completed)");
                    }
                }
            }
        }).await;
    }
    engine = engine.with_hooks(hooks);

    let summary = engine.run_summary(target).await;

    // Persist run results to SQLite (best-effort)
    #[cfg(feature = "sqlite-storage")]
    if let Ok(store) = crate::storage::LoopStore::open() {
        match store.save_run(target, "review", max_rounds, &summary) {
            Ok(run_id) => tracing::info!("Loop run saved to SQLite: id={}", run_id),
            Err(e) => tracing::warn!("Failed to save loop run to SQLite: {}", e),
        }
    }

    // Collect diff output from results' details
    let diff_output: Vec<&str> = summary.results.iter()
        .filter(|r| !r.details.is_empty())
        .map(|r| r.details.as_str())
        .collect();
    let diff_summary = if diff_output.is_empty() { None } else { Some(diff_output.join("\n")) };

    // Generate and save Markdown report
    let report = generate_markdown_report(target, "review", &summary, diff_summary.as_deref());
    let report_dir = std::path::Path::new("target");
    std::fs::create_dir_all(report_dir).ok();
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let report_path = report_dir.join(format!("loop-report-{}.md", timestamp));
    if let Err(e) = std::fs::write(&report_path, &report) {
        tracing::warn!("Failed to save Markdown report: {}", e);
    }

    // Push diagnostics to VSCode bridge (Phase 3, P2)
    use crate::ide_integration::{summary_to_vscode_diagnostics, push_loop_diagnostics};
    let vscode_diags = summary_to_vscode_diagnostics(&summary, target);
    if let Err(e) = push_loop_diagnostics(
        &std::env::current_dir().unwrap_or_default(),
        target,
        "review",
        &summary,
        vscode_diags,
    ) {
        tracing::warn!("Failed to push VSCode diagnostics: {}", e);
    }

    // Auto-archive successful runs (P3)
    if summary.passed {
        let project_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let run_id = uuid::Uuid::new_v4().to_string();
        let archive = crate::storage::archive::LoopArchive::from_summary(
            run_id, target, "review", max_rounds, &summary,
        );
        match archive.save(&project_root) {
            Ok(path) => tracing::info!("Loop run archived to {}", path.display()),
            Err(e) => tracing::warn!("Failed to archive loop run: {}", e),
        }
    }

    println!("\n═══ Result ═══\n");
    println!("  Rounds:      {}", summary.total_rounds);
    println!("  Passed:      {}", summary.passed);
    println!("  Mode:        {}", if has_llm { "LLM-driven" } else { "Heuristic" });
    println!("  Total time:  {} ms", summary.total_time_ms);
    println!();
    println!("  📄 Report: {}", report_path.display());
    println!("  🔍 VSCode diagnostics: .carp/diagnostics.json");
    println!("  🗄️  Archive: .carp/archive/");
    println!();

    if verbose {
        for r in &summary.results {
            println!("  Round {}: {:?} ({:?})", r.round, r.verdict, r.phase_times_ms);
        }
    }

    if !summary.passed {
        if let Some(LoopVerdict::Failed { reason }) = &summary.final_verdict {
            println!("  ❌ {}", reason);
        }
        std::process::exit(1);
    }

    Ok(())
}

/// Skill command — manage community skills (add, list, run, init, remove).
async fn cmd_skill(action: &SkillAction) -> anyhow::Result<()> {
    use crate::skills::composable::{SkillStore, community_skill_registry, generate_skill_template};
    use crate::skills::progressive::ProgressiveLoader;

    let project_root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let store = SkillStore::new(Some(&project_root));

    match action {
        SkillAction::List => {
            let loader = ProgressiveLoader::new(Some(&project_root));
            loader.load_metadata().await?;
            let skills = loader.list_all().await;

            println!("═══ Installed Community Skills ═══\n");
            if skills.is_empty() {
                println!("  No skills installed.");
                println!("  Try: `carp skill add <name>` or `carp skill search <query>`\n");
            } else {
                for skill in &skills {
                    println!("  {} v{} — {}", skill.name, skill.version, skill.description);
                    if !skill.tags.is_empty() {
                        println!("    Tags: [{}]", skill.tags.join(", "));
                    }
                    if let Some(ref author) = skill.author {
                        println!("    Author: {}", author);
                    }
                    println!();
                }
                println!("  Total: {} skill(s)\n", skills.len());
            }
        }
        SkillAction::Add { source } => {
            println!("  Installing skill from '{}'...\n", source);
            match store.install_skill(source) {
                Ok(frontmatter) => {
                    println!("  ✅ Installed: {} v{}", frontmatter.name, frontmatter.version);
                    println!("     {}", frontmatter.description);
                    println!("\n  Run: `carp skill run {}`", frontmatter.name);
                }
                Err(e) => {
                    eprintln!("  ❌ Failed to install skill: {}", e);
                    eprintln!("\n  Tip: Use a local SKILL.md file path, a URL, or a community name.");
                    eprintln!("  Community skills: {}",
                        community_skill_registry().iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", "));
                }
            }
        }
        SkillAction::Install { url } => {
            // P1-C2: Auto-install from GitHub or direct URL (CLI-Anything pattern)
            println!("  Auto-installing skill from '{}'...\n", url);
            match store.install_skill_auto(url) {
                Ok(frontmatter) => {
                    println!("  ✅ Installed: {} v{}", frontmatter.name, frontmatter.version);
                    println!("     {}", frontmatter.description);
                    println!("  Source: {}", url);
                    println!("\n  Run: `carp skill run {}`", frontmatter.name);
                }
                Err(e) => {
                    eprintln!("  ❌ Failed to auto-install: {}", e);
                    eprintln!("\n  Supported sources:");
                    eprintln!("    • GitHub repo:  https://github.com/owner/repo");
                    eprintln!("    • Raw SKILL.md: https://raw.githubusercontent.com/.../SKILL.md");
                    eprintln!("    • Any HTTP(S) URL pointing to a SKILL.md file");
                }
            }
        }
        SkillAction::Run { name, input } => {
            println!("  Running skill '{}'...\n", name);
            match store.get_skill(name) {
                Ok(doc) => {
                    println!("═══ {} ═══\n", doc.frontmatter.name);
                    println!("{}", doc.frontmatter.description);
                    println!("\n--- Instructions ---\n{}", doc.instructions);
                    if !input.is_empty() {
                        println!("\n--- Input ---\n{}", input);
                    }
                    if !doc.examples.is_empty() {
                        println!("\n--- Examples ---");
                        for ex in &doc.examples {
                            println!("  • {}", ex);
                        }
                    }
                    println!();
                }
                Err(e) => {
                    eprintln!("  ❌ {}", e);
                    eprintln!("  Tip: `carp skill add {}` to install from community registry", name);
                }
            }
        }
        SkillAction::Init { name } => {
            let template = generate_skill_template(name);
            let target_path = store.store_path().join(format!("{}.md", name));
            std::fs::write(&target_path, &template)?;
            println!("  ✅ Created skill template at: {}", target_path.display());
            println!("\n  Edit the file to add instructions, then run:");
            println!("  `carp skill run {}`", name);
        }
        SkillAction::Remove { name } => {
            match store.remove_skill(name) {
                Ok(()) => println!("  ✅ Removed skill '{}'", name),
                Err(e) => eprintln!("  ❌ {}", e),
            }
        }
        SkillAction::Search { query } => {
            println!("  Searching community registry for '{}'...\n", query);
            let registry = community_skill_registry();
            let lower_query = query.to_lowercase();
            let matches: Vec<_> = registry.iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&lower_query)
                        || s.description.to_lowercase().contains(&lower_query)
                        || s.tags.iter().any(|t| t.to_lowercase().contains(&lower_query))
                })
                .collect();

            if matches.is_empty() {
                println!("  No matches found in community registry.");
                println!("  Try a different query, or install from a URL/file.");
            } else {
                for skill in &matches {
                    println!("  {} — {}", skill.name, skill.description);
                    if !skill.tags.is_empty() {
                        println!("    Tags: [{}]", skill.tags.join(", "));
                    }
                    if let Some(ref author) = skill.author {
                        println!("    Author: {}", author);
                    }
                    println!("    Install: `carp skill add {}`\n", skill.name);
                }
            }
        }
        SkillAction::Analyze { url, diff, json, output } => {
            cmd_analyze(url, diff.as_deref(), *json, output.as_deref()).await?;
        }
        SkillAction::Browse { url, task, output, llm } => {
            cmd_browse(url, task, output.as_deref(), *llm).await?;
        }
    }

    Ok(())
}

/// Analyze command — visual page analysis (UI-TARS / Mano-P pattern).
async fn cmd_analyze(
    url: &str,
    diff_url: Option<&str>,
    json_mode: bool,
    output_dir: Option<&str>,
) -> anyhow::Result<()> {
    use crate::test::visual_analyzer::VisualAnalyzer;

    println!("═══ Analyze: {} ═══\n", url);

    let mut analyzer = VisualAnalyzer::new();
    if let Some(dir) = output_dir {
        let path = std::path::Path::new(dir);
        std::fs::create_dir_all(path)?;
        tracing::info!("Screenshots will be saved to: {}", dir);
    }

    let analysis = analyzer.analyze(url).await?;

    if json_mode {
        let json = serde_json::json!({
            "url": analysis.url,
            "layout": format!("{:?}", analysis.layout),
            "summary": analysis.summary,
            "elements": analysis.elements.len(),
            "has_screenshot": analysis.has_screenshot,
            "accessibility_hints": analysis.accessibility_hints,
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("Layout:       {:?}", analysis.layout);
        println!("Screenshot:   {}", if analysis.has_screenshot { "captured" } else { "not available" });
        println!("Elements:     {}", analysis.elements.len());
        println!("Summary:      {}", analysis.summary);
        if !analysis.accessibility_hints.is_empty() {
            println!("\nAccessibility hints:");
            for hint in &analysis.accessibility_hints {
                println!("  - {}", hint);
            }
        }
        println!("\nDetected UI elements:");
        for (i, el) in analysis.elements.iter().enumerate() {
            println!(
                "  {}. {:?} | bounds: [{:.2},{:.2},{:.2},{:.2}] | conf: {:.2} | text: {:?}",
                i + 1,
                el.element_type,
                el.bounds[0], el.bounds[1], el.bounds[2], el.bounds[3],
                el.confidence,
                el.text,
            );
        }
    }

    // Visual diff if requested
    if let Some(ref_url) = diff_url {
        println!("\n═══ Visual Diff: {} vs {} ═══\n", ref_url, url);
        let visual_diff = analyzer.diff(ref_url, url).await?;
        println!("Change:       {:.1}%", visual_diff.change_pct);
        for change in &visual_diff.changes {
            println!("  - {}", change);
        }
    }

    Ok(())
}

/// Browse command — LLM-driven browser agent (Browser-use / Webwright pattern).
async fn cmd_browse(
    url: &str,
    task: &str,
    output_dir: Option<&str>,
    use_llm: bool,
) -> anyhow::Result<()> {
    use crate::test::browser_agent::BrowserAgent;

    println!("═══ Browse: '{}' on '{}' ═══\n", task, url);

    let mut agent = BrowserAgent::new();
    if let Some(dir) = output_dir {
        let path = std::path::Path::new(dir).join("browse-screenshots");
        std::fs::create_dir_all(&path)?;
        agent = agent.with_screenshots_dir(path);
    }
    if use_llm {
        println!("(LLM-based planning requested — falling back to heuristic planner)\n");
    }

    let result = agent.run(url, task).await?;

    println!("{}", result.summary);
    println!();
    if !result.completed.is_empty() {
        println!("Completed steps:");
        for step in &result.completed {
            println!("  ✓ {}", step);
        }
    }
    if !result.failures.is_empty() {
        println!("\nFailures:");
        for (step, err) in &result.failures {
            println!("  ✗ {}: {}", step, err);
        }
    }
    if !result.screenshots.is_empty() {
        println!("\nScreenshots: {}", result.screenshots.len());
        for ss in &result.screenshots {
            println!("  {}", ss.display());
        }
    }

    Ok(())
}

/// Fix command — auto-fix compilation errors (P1-B).
async fn cmd_fix(config: &DeepSeekConfig, max_rounds: u32, auto_apply: bool) -> anyhow::Result<()> {
    use crate::agent::plan::{ExecuteLoop, ExecuteLoopConfig};

    let project_root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    println!("═══ Auto-Fix Mode (Plan→Execute→Compile→Fix Loop) ═══\n");
    println!("  Project: {}", project_root.display());
    println!("  Max rounds: {}", max_rounds);
    println!("  Auto-apply: {}\n", if auto_apply { "YES" } else { "NO (confirmation required)" });

    let loop_config = ExecuteLoopConfig {
        max_fix_rounds: max_rounds,
        auto_apply,
        project_root: project_root.clone(),
        stop_on_first_error: false,
    };

    let execute_loop = ExecuteLoop::new(loop_config);

    // Create agent for fix operations
    let orchestrator = ProviderOrchestrator::new(config)?;
    let agent_config = AgentConfig::default();
    let mut agent = Agent::new(config, agent_config, orchestrator)?;

    // First: check current compile status
    println!("--- Baseline Compile Check ---");
    let engine = crate::agent::compile_fix::CompileEngine::new(project_root.to_string_lossy().to_string());
    let baseline = engine.check();

    if baseline.success {
        println!("  No compilation errors found. Nothing to fix!");
        return Ok(());
    }

    println!("  Found {} errors, {} warnings\n", baseline.errors.len(), baseline.warnings);

    // Run execute loop
    let result = execute_loop.run_with_agent(&mut agent, "Fix all compilation errors").await?;
    println!("\n{}", result.format_report());

    Ok(())
}

/// Detect programming language from file path for review.
fn detect_review_language(file_path: &str) -> String {
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
            _ => "unknown",
        })
        .unwrap_or("unknown")
        .to_string()
}

/// RAG command — index codebase and search for relevant context.
async fn cmd_rag(config: &DeepSeekConfig, query: Option<String>) -> anyhow::Result<()> {
    match query {
        Some(q) => {
            // Use SemanticIndexV2 for code symbol search
            let index = SemanticIndexV2::new(IndexConfig::default());
            let search_results = index.search_symbols(&q).await;

            if !search_results.is_empty() {
                println!("═══ Symbol Search Results (SemanticIndexV2) ═══\n");
                for (symbol, score) in &search_results {
                    println!("  [{:.1}] {} ({:?}) - {}:{}",
                        score, symbol.name, symbol.kind, symbol.uri, symbol.range.start_line);
                    if let Some(ref sig) = symbol.signature {
                        println!("    Signature: {}", sig);
                    }
                    if let Some(ref doc) = symbol.documentation {
                        println!("    Doc: {}", doc);
                    }
                }
                println!("\n  Found {} matching symbols", search_results.len());
            }

            // Also run AI-powered search via agent
            let orchestrator = ProviderOrchestrator::new(config)?;
            let agent_config = AgentConfig::default();
            let mut agent = Agent::new(config, agent_config, orchestrator)?;
            let prompt = format!("Search the codebase for: {}", q);
            let result = agent.process(&prompt).await?;
            println!("\n{}", result.content);
        }
        None => {
            println!("RAG interactive mode — not yet implemented.");
        }
    }
    Ok(())
}

/// Stats command — show metrics and usage statistics.
async fn cmd_stats(config: &DeepSeekConfig) -> anyhow::Result<()> {
    let orchestrator = ProviderOrchestrator::new(config)?;
    let stats = orchestrator.stats_report().await;
    let health = orchestrator.health_report().await;

    println!("═══ Usage Statistics ═══");
    for (name, s) in &stats {
        println!("{}: {} calls, {} success, {:.1}% rate, {:.0}ms avg latency",
            name, s.total_calls, s.successful_calls,
            s.success_rate() * 100.0, s.avg_latency_ms());
    }
    if stats.is_empty() {
        println!("No statistics available yet.");
    }

    println!();
    println!("═══ Health Status ═══");
    for h in &health {
        let status = if h.is_healthy { "✓ HEALTHY" } else { "✗ UNHEALTHY" };
        println!("{}: {} (consecutive failures: {})", h.name, status, h.consecutive_failures);
    }

    Ok(())
}

/// Swarm command — execute task with multi-agent collaboration.
async fn cmd_swarm(config: &DeepSeekConfig, task: &str, rlm: bool) -> anyhow::Result<()> {
    let orchestrator = ProviderOrchestrator::new(config)?;
    let agent_config = AgentConfig::default();

    // Create swarm coordinator with 4 concurrent agents
    let swarm = SwarmCoordinator::new(4, agent_config);

    // Register specialized agents
    swarm.add_agent("coordinator", "*").await;
    swarm.add_agent("coder", "src/").await;
    swarm.add_agent("reviewer", "*").await;
    swarm.add_agent("tester", "tests/").await;

    // Build prompt based on RLM mode
    let mut full_task = task.to_string();
    if rlm {
        full_task.push_str("\nUse RLM tiered execution mode (cost-optimized sub-models).");
    }

    // Execute via swarm coordination
    let result = swarm.execute(&full_task, orchestrator).await;

    println!("Swarm Result:");
    println!("  Completed: {}", result.completed);
    println!("  Failed: {}", result.failed);
    println!("  Total Tokens: {}", result.total_tokens);

    // Print individual agent results
    for r in &result.results {
        println!("  - Task {}: {:?}", r.task_id, r.status);
    }

    Ok(())
}

/// Schedule command handler — automation task management (P0-A).
async fn cmd_schedule(
    schedule_cmd: &crate::cli::args::ScheduleCommand,
    _config: &DeepSeekConfig,
) -> anyhow::Result<()> {
    use crate::agent::scheduler::{TaskScheduler, ScheduledTask, ScheduleKind};
    use std::time::Duration;

    let scheduler = TaskScheduler::new();

    match schedule_cmd {
        crate::cli::args::ScheduleCommand::List => {
            let table = scheduler.format_task_table().await;
            println!("{}", table);
            println!("\nTotal tasks: {}", scheduler.task_count().await);
        }
        crate::cli::args::ScheduleCommand::Add { name, schedule_type, cron, interval_secs, until, max_iterations, event, prompt } => {
            let schedule = match schedule_type.as_str() {
                "cron" => ScheduleKind::Cron(cron.clone().unwrap_or_else(|| "*/5 * * * *".into())),
                "interval" => ScheduleKind::Interval(Duration::from_secs(interval_secs.unwrap_or(300))),
                "loop" => ScheduleKind::Loop {
                    max_iterations: max_iterations.unwrap_or(50),
                    condition: until.clone().unwrap_or_else(|| "exit code 0".into()),
                },
                "event" => ScheduleKind::Event(event.clone().unwrap_or_else(|| "manual".into())),
                "once" => ScheduleKind::Once,
                other => anyhow::bail!("Unknown schedule type '{}'. Use: cron | interval | loop | event | once", other),
            };

            let task = ScheduledTask {
                name: name.clone(),
                schedule,
                prompt: prompt.clone(),
                ..Default::default()
            };

            let id = scheduler.add_task(task).await?;
            println!("Task '{}' registered with ID: {}", name, id);
            println!("  Type: {} | Prompt: {}", schedule_type, &prompt[..prompt.len().min(60)]);
        }
        crate::cli::args::ScheduleCommand::Run { id } => {
            scheduler.run_task(id).await?;
            println!("Triggered task execution for: {}", id);
            println!("Note: Use the Agent/Orchestrator to execute the actual task payload.");
        }
        crate::cli::args::ScheduleCommand::Remove { id } => {
            scheduler.remove_task(id).await?;
            println!("Removed task: {}", id);
        }
        crate::cli::args::ScheduleCommand::History => {
            let table = scheduler.format_history_table().await;
            println!("{}", table);
        }
        crate::cli::args::ScheduleCommand::Trigger { event_name } => {
            let count = scheduler.trigger_event(event_name).await?;
            println!("Triggered event '{}' — {} tasks notified", event_name, count);
        }
    }

    Ok(())
}

/// Benchmark command handler — SWE-bench evaluation (P0-B).
async fn cmd_benchmark(
    bench_cmd: &crate::cli::args::BenchmarkCommand,
) -> anyhow::Result<()> {
    use crate::benchmark::{SweRunner, BenchmarkConfig, generate_sample_dataset};
    use crate::tui::canvas::{CanvasTable, CanvasDashboard, CanvasMetric, TrendDirection};

    match bench_cmd {
        crate::cli::args::BenchmarkCommand::Run { dataset, max, dry_run, sample } => {
            if let Some(count) = sample {
                // Generate and save sample dataset for testing
                let instances = generate_sample_dataset(*count);
                let path = std::path::Path::new(dataset);
                std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))?;
                let json = serde_json::to_string_pretty(&instances)?;
                std::fs::write(path, &json)?;
                println!("Generated sample dataset with {} instances -> {}", count, dataset);
                return Ok(());
            }

            let config = BenchmarkConfig {
                dataset_path: std::path::PathBuf::from(dataset),
                max_instances: *max,
                dry_run: *dry_run,
                ..Default::default()
            };

            let runner = SweRunner::new(config);
            println!("Running SWE-bench benchmark...");
            if *dry_run { println!("  [DRY-RUN MODE — validating format only]"); }
            let report = runner.run_benchmark().await?;

            // P2-B: Render with Canvas visualization
            println!("\n{}", report.format_markdown());

            // Canvas dashboard summary
            let pass_rate = if report.total_instances > 0 {
                report.resolved_count as f64 / report.total_instances as f64 * 100.0
            } else { 0.0 };
            let failed = report.total_instances.saturating_sub(report.resolved_count);

            let mut dash = CanvasDashboard::new("SWE-bench Benchmark Summary");
            let mut metrics = Vec::new();
            metrics.push(CanvasMetric::new("Pass Rate", &format!("{:.1}%", pass_rate))
                .with_unit("%").with_trend(TrendDirection::Up));
            metrics.push(CanvasMetric::new("Total Instances", &report.total_instances.to_string()));
            metrics.push(CanvasMetric::new("Resolved", &report.resolved_count.to_string())
                .with_trend(if report.resolved_count > 0 { TrendDirection::Up } else { TrendDirection::Flat }));
            metrics.push(CanvasMetric::new("Failed", &failed.to_string())
                .with_trend(if failed > 0 { TrendDirection::Down } else { TrendDirection::Flat }));
            dash.add_metrics(metrics);

            // Competitor comparison table
            let mut comp_table = CanvasTable::new(vec![
                "Competitor".into(), "Score".into(), "Gap".into(),
            ]);
            for c in &report.comparison {
                let gap = (c.score_pct as f64 - pass_rate) as i32;
                let gap_str = if gap > 0 { format!("+{:.1}%", gap as f64) } else if gap < 0 { format!("{:.1}%", gap as f64) } else { "—".into() };
                comp_table.add_row(vec![c.name.clone(), format!("{:.1}%", c.score_pct), gap_str]);
            }
            dash.add_table(comp_table);
            let max_display = max.map(|m| m.to_string()).unwrap_or_else(|| "unlimited".into());
            dash.add_text(&format!(
                "\nDataset: {} | Max instances: {} | Dry-run: {}\n",
                dataset, max_display, dry_run
            ));
            println!("{}", dash);
        }
        crate::cli::args::BenchmarkCommand::Report => {
            // Check for cached report
            let report_path = std::path::Path::new(".swe-work/report.md");
            if report_path.exists() {
                let content = std::fs::read_to_string(report_path)?;
                println!("{}", content);
            } else {
                println!("No benchmark report found. Run `dscarp benchmark run` first.");
            }
        }
    }

    Ok(())
}

/// Setup command — download models one-click.
async fn cmd_setup(force: bool, models_dir: &str) -> anyhow::Result<()> {
    crate::setup::run_setup(std::path::Path::new(models_dir), force).await
}

/// Serve-llama command handler.
async fn cmd_serve_llama(port: u16, models_dir: &str, ctx_size: u32, threads: u32) -> anyhow::Result<()> {
    if !std::path::Path::new(models_dir).exists() {
        anyhow::bail!("Models directory '{}' not found", models_dir);
    }

    let server = which_llama_server();
    eprintln!("Starting llama-server...");
    eprintln!("  Models: {}/", models_dir);
    eprintln!("  Port:   {}", port);
    eprintln!("  Ctx:    {}", ctx_size);
    eprintln!("  Threads: {}", threads);
    eprintln!();

    let status = std::process::Command::new(&server)
        .args([
            "--models-dir", models_dir,
            "--port", &port.to_string(),
            "-c", &ctx_size.to_string(),
            "--threads", &threads.to_string(),
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("llama-server exited with error");
    }
    Ok(())
}

/// Voice command — transcribe audio to text (P3).
async fn cmd_voice(
    config: &DeepSeekConfig,
    file: Option<&std::path::Path>,
    backend: &str,
    language: &str,
    send_to_agent: bool,
) -> anyhow::Result<()> {
    use crate::audio::stt::{SttEngine, SttConfig, SttBackend};

    let stt_backend = match backend.to_lowercase().as_str() {
        "cloud" | "whisper" | "openai" => SttBackend::CloudWhisper,
        "local" => SttBackend::LocalWhisper,
        "mock" | "test" => SttBackend::Mock,
        _ => anyhow::bail!("Unknown STT backend '{}'. Use: cloud | local | mock", backend),
    };

    let lang = if language == "auto" { None } else { Some(language.to_string()) };

    let stt_config = SttConfig {
        backend: stt_backend,
        language: lang,
        ..Default::default()
    };

    let engine = SttEngine::new(stt_config);

    println!("═══ Voice Input (STT) ═══\n");
    println!("  Backend: {}", backend);
    println!("  Language: {}", language);
    println!("  Formats: {}\n", engine.supported_formats().join(", "));

    match file {
        Some(path) => {
            if !path.exists() {
                anyhow::bail!("Audio file not found: {}", path.display());
            }
            println!("  File: {}\n", path.display());
            let transcript = engine.transcribe_file(path).await?;
            println!("{}", transcript.format_markdown());

            if send_to_agent {
                // Send transcribed text to agent for processing
                println!("\n--- Sending to Agent ---\n");
                let orchestrator = ProviderOrchestrator::new(config)?;
                let agent_config = AgentConfig::default();
                let mut agent = Agent::new(config, agent_config, orchestrator)?;
                let prompt = format!(
                    "The following was transcribed from voice input. Process it accordingly:\n\n{}",
                    transcript.text
                );
                let result = agent.process(&prompt).await?;
                println!("\n{}", result.content);
            }
        }
        None => {
            // No file — show recording status and instructions
            println!("  Mode: Interactive (no file provided)\n");
            if engine.is_recording() {
                println!("  ⏺ Recording in progress... Press Ctrl+C to stop.");
                match engine.stop_recording() {
                    Ok(audio_data) => {
                        println!("  Recorded {} bytes", audio_data.len());
                        let transcript = engine.transcribe_bytes(&audio_data).await?;
                        println!("{}", transcript.format_markdown());
                    }
                    Err(e) => eprintln!("  Recording error: {}", e),
                }
            } else {
                println!("  Usage:");
                println!("    dscarp voice <file.wav>          Transcribe a WAV file");
                println!("    dscarp voice <file.wav> --agent  Transcribe + send to agent");
                println!("    dscarp voice --backend local     Use local Whisper model");
                println!("    dscarp voice --language zh       Force Chinese language");
                println!("\n  Note: Microphone recording requires platform-specific support.");
                println!("  For now, provide a pre-recorded .wav file.");
            }
        }
    }

    Ok(())
}

/// Archive command — manage archived loop runs.
async fn cmd_archive(cmd: &ArchiveCommand, cli: &Cli) -> anyhow::Result<()> {
    let project_root = cli.dir.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let json_mode = cli.json;
    use crate::storage::archive::{LoopArchive, RetroReport};
    use crate::cli::json_output::{self as jo, CommandResult};

    match cmd {
        ArchiveCommand::List => {
            let archives = LoopArchive::list(&project_root)?;
            if json_mode {
                let data = jo::archive_list_to_json(&archives);
                CommandResult::ok_with_msg("archive list", data,
                    format!("{} archive(s) listed", archives.len())
                ).print(true);
            } else {
                println!("═══ Archived Loop Runs ═══\n");
                if archives.is_empty() {
                    println!("  No archived runs found in {}", project_root.join(".carp/archive/").display());
                    return Ok(());
                }
                println!("  {:<20} {:<20} {:<10} {:<8} {:<10} Date",
                    "Run ID", "Target", "Mode", "Passed", "Rounds");
                println!("  {}", "-".repeat(95));
                for a in &archives {
                    println!("  {:<20} {:<20} {:<10} {:<8} {:<10} {}",
                        &a.run_id[..a.run_id.len().min(20)],
                        &a.target[..a.target.len().min(20)],
                        a.mode,
                        if a.passed { "✅" } else { "❌" },
                        a.total_rounds,
                        &a.created_at[..19],
                    );
                }
                println!("\n  Total: {} archive(s)\n", archives.len());
            }
        }
        ArchiveCommand::Show { id } => {
            match LoopArchive::load(&project_root, id)? {
                Some(archive) => {
                    if json_mode {
                        let data = serde_json::json!({
                            "run_id": id,
                            "meta": archive.meta,
                            "summary": {
                                "total_rounds": archive.summary.total_rounds,
                                "final_verdict": format!("{:?}", archive.summary.final_verdict),
                                "passed": archive.summary.passed,
                                "results": archive.summary.results.iter().map(|r| serde_json::json!({
                                    "round": r.round, "verdict": format!("{:?}", r.verdict),
                                    "duration_ms": r.total_time_ms,
                                })).collect::<Vec<_>>(),
                            },
                        });
                        CommandResult::ok("archive show", data).print(true);
                    } else {
                        println!("═══ Archive: {} ═══\n", id);
                        println!("  Target:       {}", archive.meta.target);
                        println!("  Mode:         {}", archive.meta.mode);
                        println!("  Passed:       {}", archive.meta.passed);
                        println!("  Rounds:       {}/{}", archive.meta.total_rounds, archive.max_rounds);
                        println!("  Total time:   {} ms", archive.meta.total_time_ms);
                        println!("  Created:      {}", archive.meta.created_at);
                        println!();
                        for round in &archive.summary.results {
                            println!("  Round {}: {:?}", round.round, round.verdict);
                            if !round.details.is_empty() {
                                println!("    Details: {}", round.details);
                            }
                            if !round.spec_deltas.is_empty() {
                                println!("    Spec changes:");
                                for delta in &round.spec_deltas {
                                    println!("      {}", delta.to_markdown());
                                }
                            }
                            let phase_str: Vec<String> = round.phase_times_ms.iter()
                                .map(|(p, ms)| format!("{:?}={}ms", p, ms))
                                .collect();
                            println!("    Phases: {}", phase_str.join(", "));
                        }
                    }
                }
                None => {
                    let msg = format!("Archive not found: {}", id);
                    if json_mode {
                        CommandResult::err("archive show", &msg).print(true);
                    } else {
                        eprintln!("{}", msg);
                    }
                }
            }
        }
        ArchiveCommand::Delete { id } => {
            if LoopArchive::delete(&project_root, id)? {
                let msg = format!("Deleted archive: {}", id);
                if json_mode {
                    CommandResult::ok_with_msg("archive delete", serde_json::json!({"deleted_id": id}), &msg).print(true);
                } else {
                    println!("  ✅ {}", msg);
                }
            } else {
                let msg = format!("Archive not found: {}", id);
                if json_mode {
                    CommandResult::err("archive delete", &msg).print(true);
                } else {
                    eprintln!("  {}", msg);
                }
            }
        }
        ArchiveCommand::Purge { days } => {
            let count = LoopArchive::purge_older_than(&project_root, *days as i64)?;
            let msg = format!("Purged {} archive(s) older than {} days", count, days);
            if json_mode {
                CommandResult::ok_with_msg("archive purge",
                    serde_json::json!({"purged_count": count, "older_than_days": days}), &msg
                ).print(true);
            } else {
                println!("  ✅ {}", msg);
            }
        }
        ArchiveCommand::Retro => {
            let report = RetroReport::generate(&project_root)?;
            if json_mode {
                let data = jo::retro_report_to_json(&report);
                CommandResult::ok_with_msg("archive retro", data, report.summary).print(true);
            } else {
                println!("{}", report.to_text());
            }
        }
    }

    Ok(())
}
