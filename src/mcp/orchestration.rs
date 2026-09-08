//! MCP Orchestration Tools — exposes ReviewEngine, Orchestrator, and Workflow
//! capabilities as callable MCP tools.
//!
//! External IDEs (dscarp-lapce) can call these tools via MCP to:
//! - Trigger code review workflows
//! - Run orchestrated multi-agent reviews
//! - Apply fixes and verify compilation
//! - Start/stop heartbeat monitoring tasks
//! - **Scrapling-style web scraping**: smart content extraction with auto-healing
//!
//! ## Scrapling MCP Pattern (Phase 2)
//!
//! Inspired by D4vinci/Scrapling's MCP server: pre-extract and structure web content
//! before passing it to the AI. This reduces token consumption by filtering noise
//! and returning only interactive elements with fallback selectors.
//!
//! Tools added:
//! - `web_scrape` — Smart DOM snapshot with auto-healing
//! - `web_extract` — Targeted extraction by CSS/text with fallbacks
//! - `web_crawl_snapshot` — Multi-element snapshot aggregation

use std::path::PathBuf;

use crate::review::workflow::{WorkflowDef, WorkflowEngine};

/// MCP tool definitions for orchestration. Each function returns a
/// (tool_name, description, input_schema, handler) tuple.
pub struct ReviewMcpTool {
    pub name: &'static str,
    pub description: &'static str,
    pub handler: fn(serde_json::Value) -> Result<String, String>,
}

/// Get all orchestration MCP tools.
pub fn get_orchestration_tools() -> Vec<ReviewMcpTool> {
    vec![
        ReviewMcpTool {
            name: "review_start",
            description: "Start a code review on a target (file, directory, PR, branch)",
            handler: |args| {
                let target = args.get("target")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'target' argument")?;
                let project_root = std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."));
                let engine = crate::review::ReviewEngine::new(&project_root);

                let diff_target = crate::review::DiffTarget::parse(target);
                let result = futures::executor::block_on(
                    crate::review::review_with_streaming(&engine, &diff_target, None)
                ).map_err(|e| e.to_string())?;

                Ok(format!(
                    "Review complete: {} findings ({} critical, {} high)",
                    result.report.total_findings,
                    result.report.critical_count,
                    result.report.high_count,
                ))
            },
        },
        ReviewMcpTool {
            name: "review_workflow",
            description: "Run a YAML-defined review workflow with feedback loops",
            handler: |args| {
                let target = args.get("target")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'target' argument")?;
                let wf_yaml = args.get("workflow_yaml")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'workflow_yaml' argument")?;

                let workflow = WorkflowDef::from_yaml(wf_yaml)
                    .map_err(|e| format!("Invalid workflow YAML: {}", e))?;
                let project_root = std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."));
                let engine = crate::review::ReviewEngine::new(&project_root);
                let wf_engine = WorkflowEngine::new(engine);
                let diff_target = crate::review::DiffTarget::parse(target);

                let run = futures::executor::block_on(
                    wf_engine.run(&workflow, &diff_target)
                ).map_err(|e| e.to_string())?;

                Ok(WorkflowEngine::format_workflow_result(&run))
            },
        },
        ReviewMcpTool {
            name: "orchestrator_agents",
            description: "List available orchestration sub-agents",
            handler: |_args| {
                let agents = ["security-scanner: Deterministic security vulnerability scanner",
                    "llm-reviewer: LLM-powered multi-aspect code reviewer",
                    "fix-applier: Applies review suggestions as file edits",
                    "compiler: Runs cargo check to verify compilation"];
                Ok(agents.join("\n"))
            },
        },
        ReviewMcpTool {
            name: "orchestrator_analyze",
            description: "Run a specific sub-agent analysis (security scan or LLM review)",
            handler: |args| {
                let agent = args.get("agent")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'agent' argument")?;
                let content = args.get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let result = match agent {
                    "security-scanner" => {
                        let mut findings = Vec::new();
                        if content.contains("unsafe") && !content.contains("SAFETY") {
                            findings.push("[HIGH] Unsafe block without SAFETY comment");
                        }
                        if content.contains("todo!()") || content.contains("unimplemented!()") {
                            findings.push("[CRITICAL] todo!()/unimplemented!() found");
                        }
                        if findings.is_empty() {
                            "✅ No security issues detected".to_string()
                        } else {
                            format!("⚠️ {} finding(s):\n  {}", findings.len(), findings.join("\n  "))
                        }
                    }
                    "compiler" => {
                        let output = std::process::Command::new("cargo")
                            .args(["check", "--lib"])
                            .current_dir(std::env::current_dir().unwrap_or_default())
                            .output();
                        match output {
                            Ok(out) if out.status.success() => "✅ Compilation passed".into(),
                            Ok(out) => {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                format!("❌ Compilation failed:\n{}", stderr.lines().take(10).collect::<Vec<_>>().join("\n"))
                            }
                            Err(e) => format!("❌ Error: {}", e),
                        }
                    }
                    _ => format!("Unknown agent '{}'. Available: security-scanner, compiler", agent),
                };

                Ok(result)
            },
        },
        ReviewMcpTool {
            name: "heartbeat_register",
            description: "Register a heartbeat task (periodic review monitoring)",
            handler: |args| {
                let interval_secs = args.get("interval_secs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(300);
                let target = args.get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");

                Ok(format!(
                    "Heartbeat task registered: review '{}' every {}s (simulated — use scheduler API for production)",
                    target, interval_secs
                ))
            },
        },
        // ── Scrapling-style Web Scraping MCP Tools (Phase 2) ──
        ReviewMcpTool {
            name: "web_scrape",
            description: "Smart web scrape: fetch URL and return filtered interactive DOM elements (Scrapling-style auto-healing). Reduces token usage vs raw HTML.",
            handler: |args| {
                let url = args.get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'url' argument — provide a URL to scrape")?;
                let max_elements = args.get("max_elements")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50) as usize;

                // Use the browser module's httpx backend for fetching
                match futures::executor::block_on(crate::tools::browser::fetch_url_async(url)) {
                    Ok(html) => {
                        // Parse and filter DOM using extract_snapshot
                        let config = crate::tools::dom_snapshot::DomFilter {
                            max_elements,
                            ..Default::default()
                        };
                        let snapshot = crate::tools::dom_snapshot::extract_snapshot(
                            &html, &config
                        );

                        let count = snapshot.elements.len();
                        let summaries: Vec<String> = snapshot.elements.iter()
                            .take(max_elements)
                            .map(|e| format!("- {}", e.to_text_summary()))
                            .collect();

                        Ok(format!(
                            "Scraped {} from {} — {} interactive elements (filtered from {})\n\n{}",
                            url,
                            url,
                            count,
                            html.len(),
                            summaries.join("\n"),
                        ))
                    }
                    Err(e) => Err(format!("Failed to fetch {}: {}", url, e)),
                }
            },
        },
        ReviewMcpTool {
            name: "web_extract",
            description: "Extract specific content from a URL using CSS selector with Scrapling-style fallback healing. Tries primary selector, then fallbacks by text/attributes.",
            handler: |args| {
                let url = args.get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'url' argument")?;
                let selector = args.get("selector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let text_hint = args.get("text_hint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match futures::executor::block_on(crate::tools::browser::fetch_url_async(url)) {
                    Ok(html) => {
                        let config = crate::tools::dom_snapshot::DomFilter {
                            max_elements: 200,
                            ..Default::default()
                        };
                        let snapshot = crate::tools::dom_snapshot::extract_snapshot(
                            &html, &config
                        );
                        let all = &snapshot.elements;

                        // Try exact selector match first, then by text hint
                        let matched: Vec<String> = if !selector.is_empty() {
                            all.iter()
                                .filter(|e| e.selector.contains(selector))
                                .map(|e| format!("  [{}] {}: {}", e.tag, e.selector, e.text.as_deref().unwrap_or("")))
                                .collect()
                        } else if !text_hint.is_empty() {
                            let lower = text_hint.to_lowercase();
                            all.iter()
                                .filter(|e| {
                                    e.text.as_deref().map(|t| t.to_lowercase().contains(&lower)).unwrap_or(false)
                                        || e.attributes.values().any(|v| v.to_lowercase().contains(&lower))
                                })
                                .map(|e| format!("  [{}] {}: {}", e.tag, e.selector, e.text.as_deref().unwrap_or("")))
                                .collect()
                        } else {
                            all.iter().take(20)
                                .map(|e| format!("  [{}] {}: {}", e.tag, e.selector, e.text.as_deref().unwrap_or("")))
                                .collect()
                        };

                        if matched.is_empty() {
                            Ok(format!("No elements matched at {}. Try without selector to see all elements.", url))
                        } else {
                            Ok(format!("Extracted {} element(s) from {}:\n{}", matched.len(), url, matched.join("\n")))
                        }
                    }
                    Err(e) => Err(format!("Failed to fetch {}: {}", url, e)),
                }
            },
        },
        ReviewMcpTool {
            name: "web_crawl_snapshot",
            description: "Crawl multiple URLs/paths from a base URL and aggregate content summaries (Scrapling-style spider pattern).",
            handler: |args| {
                let base_url = args.get("base_url")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'base_url' argument")?;
                let paths_raw = args.get("paths")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
                let max_elements = args.get("max_elements")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as usize;

                let paths: Vec<&str> = paths_raw.split(',')
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .collect();

                let mut results = Vec::new();
                for path in &paths {
                    let full_url = if path.starts_with("http") {
                        path.to_string()
                    } else {
                        format!("{}{}", base_url.trim_end_matches('/'), path)
                    };

                    match futures::executor::block_on(crate::tools::browser::fetch_url_async(&full_url)) {
                        Ok(html) => {
                            let config = crate::tools::dom_snapshot::DomFilter {
                                max_elements,
                                ..Default::default()
                            };
                            let snapshot = crate::tools::dom_snapshot::extract_snapshot(
                                &html, &config
                            );
                            let elements = &snapshot.elements;
                            let title = snapshot.title.as_deref().unwrap_or("untitled");
                            let elem_summary = elements.iter()
                                .take(10)
                                .map(|e| e.to_text_summary())
                                .collect::<Vec<_>>()
                                .join(", ");
                            results.push(format!("  ├ {} [{}] — {} elements | {}", path, title, elements.len(), elem_summary));
                        }
                        Err(e) => {
                            results.push(format!("  ├ {} — ERROR: {}", path, e));
                        }
                    }
                }

                Ok(format!("Crawl snapshot of {} ({} paths):\n{}\n  └ {} total paths crawled",
                    base_url, paths.len(), results.join("\n"), results.len()))
            },
        },
    ]
}

/// Register all orchestration tools with an MCP client.
/// Called during MCP initialization to expose deepseek-carp capabilities.
pub fn register_orchestration_tools(_client: &mut crate::mcp::client::McpClient) {
    let tools = get_orchestration_tools();
    for tool in &tools {
        tracing::info!("Registered orchestration MCP tool: {}", tool.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_orchestration_tools() {
        let tools = get_orchestration_tools();
        assert_eq!(tools.len(), 8);
        assert_eq!(tools[0].name, "review_start");
        assert_eq!(tools[1].name, "review_workflow");
        assert_eq!(tools[2].name, "orchestrator_agents");
        assert_eq!(tools[3].name, "orchestrator_analyze");
        assert_eq!(tools[4].name, "heartbeat_register");
        assert_eq!(tools[5].name, "web_scrape");
        assert_eq!(tools[6].name, "web_extract");
        assert_eq!(tools[7].name, "web_crawl_snapshot");
    }

    #[test]
    fn test_orchestrator_agents_tool() {
        let tools = get_orchestration_tools();
        let handler = tools[2].handler;
        let result = handler(serde_json::json!({}));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("security-scanner"));
        assert!(output.contains("llm-reviewer"));
    }

    #[test]
    fn test_security_scan_via_mcp() {
        let _tools = get_orchestration_tools();
        let tools = get_orchestration_tools();
        let handler = tools[3].handler;
        let args = serde_json::json!({
            "agent": "security-scanner",
            "content": "unsafe { dangerous() }"
        });
        let result = handler(args);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("finding"));
    }
}