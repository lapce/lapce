use deepseek_carp::tools::lsp_client_v2::{LapceBridgeConn, LapceLspBridge};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let workspace = std::env::current_dir()?;
    println!("Workspace: {}", workspace.display());

    let server_name = LapceLspBridge::detect_for(&workspace);
    if server_name.is_empty() {
        eprintln!("No LSP server detected for this workspace");
        return Ok(());
    }
    println!("Detected LSP server: {}", server_name);

    let bridge = LapceLspBridge::new(&server_name, workspace.clone());
    let mut conn = LapceBridgeConn::spawn(bridge).await?;
    println!("Spawned LSP server successfully");

    let init_params = json!({
        "processId": std::process::id(),
        "rootUri": format!("file://{}", workspace.display()),
        "capabilities": json!({
            "textDocument": {
                "completion": { "completionItem": { "snippetSupport": true } },
                "hover": { "contentFormat": ["plaintext", "markdown"] },
                "diagnostics": {}
            }
        })
    });

    let _id = conn.send_request("initialize", init_params).await?;
    println!("Sent initialize request");

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let responses = conn.drain_to_end().await?;
    for msg in &responses {
        if msg.id.is_some() {
            println!("Initialize response: {}", serde_json::to_string_pretty(msg).unwrap());
        }
    }

    conn.send_notification("initialized", json!({})).await?;
    println!("Sent initialized notification");

    let src_file = workspace.join("src/main.rs");
    if !src_file.exists() {
        eprintln!("src/main.rs not found, skipping didOpen");
        conn.shutdown().await?;
        return Ok(());
    }

    let content = std::fs::read_to_string(&src_file)?;
    let did_open_params = json!({
        "textDocument": {
            "uri": format!("file://{}", src_file.display()),
            "languageId": "rust",
            "version": 1,
            "text": content
        }
    });

    conn.send_notification("textDocument/didOpen", did_open_params).await?;
    println!("Sent textDocument/didOpen for {}", src_file.display());

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    let messages = conn.drain_to_end().await?;
    
    let mut has_diagnostics = false;
    for msg in &messages {
        if let Some(method) = &msg.method {
            if method == "textDocument/publishDiagnostics" {
                has_diagnostics = true;
                println!("\n=== Diagnostics ===");
                println!("{}", serde_json::to_string_pretty(msg).unwrap());
            } else {
                println!("Notification: {}", method);
            }
        }
    }

    if !has_diagnostics {
        println!("No diagnostics received");
    }

    conn.shutdown().await?;
    println!("Shutdown complete");

    Ok(())
}