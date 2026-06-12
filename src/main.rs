//! DeepSeek Carp — Multi-provider AI coding assistant.
//!
//! Binary entry point. Handles CLI dispatch, MCP server mode, graceful
//! shutdown with session checkpointing, and structured error reporting.
//!
//! Startup modes (selected by env var, override with --mode):
//!
//! | Mode       | When                      | What it does                      |
//! |------------|---------------------------|-----------------------------------|
//! | cli        | Default                   | Chat / ask / complete subcommands |
//! | mcp-stdio  | DEEPCARP_MCP_SERVER=on    | MCP tool server on stdin/stdout   |
//! | mcp-sse    | DEEPCARP_MCP_SERVER=on    | MCP tool server on TCP localhost   |

use clap::Parser;
use deepseek_carp::cli::{Cli, run};
use deepseek_carp::config::paths;
use deepseek_carp::mcp::server::DeepseekMcpServer;

fn main() {
    let mode = std::env::var("DEEPCARP_STARTUP_MODE")
        .or_else(|_| std::env::var("DEEPCARP_MCP_SERVER").map(|v| if v == "on" { "mcp".into() } else { "cli".into() }))
        .unwrap_or_else(|_| "cli".into());

    if mode == "mcp" || mode == "mcp-stdio" || mode == "mcp-sse" {
        run_mcp_server(mode.as_str());
        return;
    }

    let parsed = <Cli as clap::Parser>::try_parse();
    if let Ok(cli) = parsed {
        use deepseek_carp::cli::args::Commands;
        if matches!(cli.command, Some(Commands::McpConfig)) {
            println!("{}", deepseek_carp::mcp::server::generate_mcp_config());
            return;
        }
    }

    tokio_start();
}

fn run_mcp_server(mode: &str) {
    let transport = if mode == "mcp-sse" { "sse" } else { "stdio" };
    let port: u16 = std::env::var("DEEPCARP_MCP_PORT").ok()
        .and_then(|s| s.parse().ok()).unwrap_or(7789);
    let server = DeepseekMcpServer::new();
    eprintln!("🤖 deepseek-carp MCP server (transport={}, port={})", transport, port);
    match transport {
        "sse" => { let _ = server.run_sse(port); }
        _ => { let _ = server.run_stdio(); }
    }
}

fn tokio_start() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    runtime.block_on(async_main());
}

async fn async_main() {
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::warn!("Received Ctrl+C — saving checkpoint & initiating graceful shutdown...");
        write_shutdown_checkpoint();
        shutdown_tx_clone.send(true).ok();
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        tracing::warn!("Graceful shutdown timeout — forcing exit");
        std::process::exit(1);
    });

    let cli = Cli::parse();

    if let Err(e) = run(cli, shutdown_rx).await {
        eprintln!("Error: {}", e);
        let mut source = e.source();
        while let Some(s) = source {
            eprintln!("  caused by: {}", s);
            source = s.source();
        }
        std::process::exit(1);
    }

    tracing::info!("DeepSeek Carp shut down gracefully.");
}

fn write_shutdown_checkpoint() {
    let path = paths::sessions_dir().join("_shutdown_checkpoint.json");
    let checkpoint = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "reason": "user_interrupt",
        "note": "Session checkpoint saved on Ctrl+C. Use 'deepseek-carp chat --session last' to resume."
    });

    if let Ok(json) = serde_json::to_string_pretty(&checkpoint) {
        if std::fs::write(&path, json).is_ok() {
            eprintln!("💾 Session checkpoint saved to {}", path.display());
        }
    }
}
