//! Tool system — trait-based tool execution.
//!
//! `ToolExecutor` trait allows tool execution logic to be injected,
//! enabling testing, sandboxing, and custom tool implementations.

pub mod diff;
pub mod dom_snapshot;
pub mod streaming;
pub mod precise_edit;
pub mod edit_reliability;
pub mod tool_perf;
pub mod checkpoint;
pub mod git_snapshot;
pub mod error_recovery;
pub mod apply_editor;
pub mod browser;
pub mod lsp_diagnostics;
pub mod streaming_chunks;
pub mod advanced_tools;
pub mod auto_fix;
pub mod code_smell;
pub mod semantic_refactor;
pub mod refactor_preview;
pub mod refactor_history;
pub mod path_analysis;
pub mod refactor_apply;
pub mod realtime_review;
pub mod test_generator;
pub mod integration_test;
pub mod debug_engine;
pub mod security_scanner;
pub mod security_scanner_v2;
pub mod lsp_client_v2;
pub mod audit_logger;
pub mod pr_reviewer;
pub mod batch_editor;
pub mod apply_engine;
pub mod shipping;
pub mod stealthy_fetcher;
pub mod remote_ops;
pub mod hybrid_browser;

pub use diff::{DiffEngine, FileEdit, DiffHunk, EditResult, DiffSession};
pub use dom_snapshot::{
    DomSnapshot, DomFilter, InteractiveElement, BoundingBox,
    extract_snapshot, extract_snapshot_from_url,
};
pub use streaming::{
    StreamingToolExecutor, StreamingTool, ToolProgress,
    StreamingToolError, ToolInterrupt, ShellTool, FileReadTool,
};
pub use precise_edit::{PreciseEditEngine, MatchStrategy, EditResult as PreciseEditResult};
pub use edit_reliability::{
    ReliableEditEngine, ReliableEditResult, EditConfidence, PostEditValidation,
    AstAwareMatcher, AstMatcherConfig, CodeBoundary, EditConfidenceScorer,
};
pub use tool_perf::{
    ToolResultCache, ToolCacheConfig, ToolCacheStats,
    BlockingExecutor, ShellPool, ShellPoolStats, PooledShell,
    BulkFileOps, BulkReadResult, BulkWriteResult,
};
pub use checkpoint::CheckpointManager;
pub use git_snapshot::{
    GitSnapshotManager, GitOutput, BranchManager, TaskBranch, BranchStatus, MergeResult,
    ConflictInfo, ConflictResolver, ParsedConflict, ResolveStrategy, ResolveResult,
    PrWorkflow, PrCheckReport,
};
pub use apply_editor::{IdeConnector, IdeEdit, IdeConnection, IdeLockfile};
pub use error_recovery::{
    ErrorClassifier, ErrorSeverity, RetryStrategy, CircuitBreaker, CircuitState,
    retry_async,
};
pub use auto_fix::{
    ErrorPatternLibrary, ErrorPattern, FixGenerator, AutoFixSuggestion,
    ErrorCategory, FixRiskLevel, ApplyPosition, ErrorWithFix,
};
pub use code_smell::{
    SmellDetector, SmellType, SmellOccurrence, SmellMetrics,
};
pub use semantic_refactor::{
    SemanticRefactorEngine, RefactorType, RefactorOperation, SemanticInfo,
    SemanticKind, UsageLocation, Location, RiskLevel,
    analyze_impact, ImpactAnalysis, BreakingChange,
};
pub use refactor_preview::{
    RefactorPreview, PreviewHunk, ChangeLine, ChangeLineType,
    generate_preview, format_ansi_preview, format_markdown_preview,
};
pub use refactor_history::{
    RefactorHistory, RefactorDecision, RefactorContext, LearnedPatterns, RefactorStats,
};
pub use path_analysis::{
    PathAnalyzer, ControlFlowGraph, CFGNode, CFGNodeType,
    ExecutionPath, BugPrediction, BugLocation, BugType,
};
pub use security_scanner_v2::{
    SecurityScannerV2, SecurityFindingV2, SecurityReportV2, VulnerabilitySeverity,
    VulnerabilityPattern, Confidence, ReportSummary, ComplianceStatus,
    LanguageSecurityContext,
};
pub use lsp_client_v2::{
    LspClientV2, LspServerConfig, LspCapabilities, LspRequest, LspResponse,
    Diagnostic, Range, Position, SymbolKind, WorkspaceSymbol,
};
pub use pr_reviewer::{
    PrReviewer, PrReviewResult, PrReviewReport, ReviewAspect, ReviewVerdict,
    FindingSeverity,
};
pub use batch_editor::{
    BatchEditor, BatchTransaction, FileEdit as BatchFileEdit, EditType,
    TxnStatus, TxnResult, TxnMetadata, RiskLevel as BatchRiskLevel, EditorStats,
};
pub use apply_engine::{
    ApplyEngine, EditFormat, EditResult as ApplyEditResult, EditFormatType,
    ScoredPatch, ConflictStrategy,
};

use crate::providers::provider::{FunctionDef, ToolDef};
use async_trait::async_trait;

pub use hybrid_browser::{
    HybridBrowser, HybridAction, HybridActionResult, HybridSnapshot,
    ElementLocation, LocateMethod,
};

/// Result of executing a tool.
#[derive(Debug, Clone)]
pub enum ToolResult {
    Success(String),
    Error(String),
}

impl ToolResult {
    pub fn to_json(&self) -> String {
        match self {
            ToolResult::Success(msg) => msg.clone(),
            ToolResult::Error(e) => format!(r#"{{"error": "{}"}}"#, e),
        }
    }
}

/// A tool definition.
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Trait for executing tools — enables dependency injection.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, args_json: &str) -> ToolResult;
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: Vec<Tool>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: Vec::new() } }

    pub fn with_defaults() -> Self {
        Self {
            tools: vec![
                Tool {
                    name: "read_file".into(),
                    description: "Read the contents of a file at the given path.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string", "description": "Absolute path to the file"}},
                        "required": ["path"]
                    }),
                },
                Tool {
                    name: "write_file".into(),
                    description: "Write content to a file, creating it if needed.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "content": {"type": "string"}
                        },
                        "required": ["path", "content"]
                    }),
                },
                Tool {
                    name: "search_code".into(),
                    description: "Search for code patterns in the project using regex.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "pattern": {"type": "string"},
                            "directory": {"type": "string"},
                            "file_types": {"type": "string"}
                        },
                        "required": ["pattern"]
                    }),
                },
                Tool {
                    name: "execute_command".into(),
                    description: "Execute a shell command and return the output.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"},
                            "working_dir": {"type": "string"}
                        },
                        "required": ["command"]
                    }),
                },
                Tool {
                    name: "list_directory".into(),
                    description: "List contents of a directory.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    }),
                },
                Tool {
                    name: "fetch_url".into(),
                    description: "Fetch a URL and extract readable text content (HTML-to-text).".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {"url": {"type": "string", "description": "URL to fetch"}},
                        "required": ["url"]
                    }),
                },
                Tool {
                    name: "lsp_diagnostics".into(),
                    description: "Run language-specific diagnostics on a file (Rust/TS/Python/Go). Returns formatted errors for LLM auto-fix.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string", "description": "File to check"},
                            "workspace_root": {"type": "string", "description": "Project root directory"}
                        },
                        "required": ["file_path"]
                    }),
                },
                // ── Bulk file operations (v0.2.0) ──
                Tool {
                    name: "read_files".into(),
                    description: "Read multiple files in parallel. Much faster than calling read_file N times. Provide an array of paths.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "paths": {"type": "array", "items": {"type": "string"}, "description": "Array of absolute file paths"}
                        },
                        "required": ["paths"]
                    }),
                },
                Tool {
                name: "write_files".into(),
                description: "Write multiple files in parallel. Provide an array of {path, content} objects.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "files": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "path": {"type": "string"},
                                    "content": {"type": "string"}
                                },
                                "required": ["path", "content"]
                            }
                        }
                    },
                    "required": ["files"]
                }),
            },
            // ── Advanced Tools (v0.3.0) ──
            Tool {
                name: "generate_tests".into(),
                description: "Generate test files for a source code file. Supports Rust, Python, TypeScript, JavaScript, Go, Java, C#.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "source_file": {"type": "string", "description": "Path to source file to generate tests for"},
                        "framework": {"type": "string", "description": "Test framework (rust, python, typescript, etc.)"},
                        "include_edge_cases": {"type": "boolean", "description": "Include edge case tests"}
                    },
                    "required": ["source_file"]
                }),
            },
            Tool {
                name: "analyze_error".into(),
                description: "Analyze an error message and provide debugging suggestions.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "error_message": {"type": "string", "description": "The error message to analyze"},
                        "source_file": {"type": "string", "description": "Source file related to the error"},
                        "stack_trace": {"type": "string", "description": "Stack trace if available"},
                        "context": {"type": "string", "description": "Additional context about the error"}
                    },
                    "required": ["error_message"]
                }),
            },
            Tool {
                name: "refactor_suggest".into(),
                description: "Analyze a code file and provide refactoring suggestions.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "source_file": {"type": "string", "description": "File to analyze for refactoring"},
                        "refactor_type": {"type": "string", "description": "Type of refactoring (cleanup, performance, readability)"},
                        "line_range": {"type": "string", "description": "Specific line range to focus on"}
                    },
                    "required": ["source_file"]
                }),
            },
            Tool {
                name: "explain_code".into(),
                description: "Provide a detailed explanation of code functionality.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "source_file": {"type": "string", "description": "File to explain"},
                        "line_range": {"type": "string", "description": "Specific line range to explain"},
                        "detail_level": {"type": "string", "description": "Detail level (high, medium, low)"}
                    },
                    "required": ["source_file"]
                }),
            },
            // ── C-enhanced Tools (v0.3.0) ──
            Tool {
                name: "parse_rust_code".into(),
                description: "Parse Rust code to extract functions, structs, traits, and imports (simplified AST).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {"type": "string", "description": "Rust source code to parse"}
                    },
                    "required": ["code"]
                }),
            },
            Tool {
                name: "parse_python_code".into(),
                description: "Parse Python code to extract functions, classes, and imports.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {"type": "string", "description": "Python source code to parse"}
                    },
                    "required": ["code"]
                }),
            },
            Tool {
                name: "enhanced_debug_analysis".into(),
                description: "Comprehensive debug analysis with breakpoint suggestions and log injection points.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "source_file": {"type": "string", "description": "Source file to analyze"},
                        "error_message": {"type": "string", "description": "Error message to analyze"},
                        "error_location": {"type": "string", "description": "Specific line/location of error"}
                    },
                    "required": ["source_file"]
                }),
            },
            Tool {
                name: "run_tests".into(),
                description: "Run tests for a file using the appropriate framework (cargo test, pytest, npm test, go test).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "test_file": {"type": "string", "description": "Test file or directory to run"},
                        "test_pattern": {"type": "string", "description": "Pattern to filter tests (optional)"},
                        "watch_mode": {"type": "boolean", "description": "Run in watch mode (optional)"}
                    },
                    "required": ["test_file"]
                }),
            },
            Tool {
                name: "detect_framework".into(),
                description: "Detect project type, language, framework, and test framework from a directory.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "directory": {"type": "string", "description": "Directory to analyze"}
                    },
                    "required": ["directory"]
                }),
            },
            ],
        }
    }

    /// Create a ToolExecutor that runs tools with this registry.
    pub fn executor(&self) -> impl ToolExecutor {
        DefaultToolExecutor
    }

    /// Create a cached ToolExecutor with performance optimizations.
    pub fn cached_executor(&self) -> CachedToolExecutor {
        CachedToolExecutor::new()
    }

    pub fn to_openai_format(&self) -> Vec<ToolDef> {
        self.tools.iter().map(|t| ToolDef {
            tool_type: "function".into(),
            function: FunctionDef {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            },
        }).collect()
    }

    pub fn get(&self, name: &str) -> Option<&Tool> { self.tools.iter().find(|t| t.name == name) }
    pub fn add(&mut self, tool: Tool) { self.tools.push(tool); }
    pub fn names(&self) -> Vec<&str> { self.tools.iter().map(|t| t.name.as_str()).collect() }
}

// ── Default Tool Executor ──

struct DefaultToolExecutor;

#[async_trait]
impl ToolExecutor for DefaultToolExecutor {
    async fn execute(&self, name: &str, args_json: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(args_json) {
            Ok(v) => v,
            Err(e) => return ToolResult::Error(format!("Invalid arguments: {}", e)),
        };

        match name {
            "read_file" => {
                let path = args["path"].as_str().unwrap_or("");
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        let truncated = content.len() > 20000;
                        let display = if truncated { &content[..20000] } else { &content };
                        if truncated {
                            ToolResult::Success(format!(r#"{{"content": "{}", "truncated": true, "total_size": {}}}"#, display.escape_default(), content.len()))
                        } else {
                            ToolResult::Success(format!(r#"{{"content": "{}"}}"#, display.escape_default()))
                        }
                    }
                    Err(e) => ToolResult::Error(e.to_string()),
                }
            }
            "write_file" => {
                let path = args["path"].as_str().unwrap_or("");
                let content = args["content"].as_str().unwrap_or("");
                match std::fs::write(path, content) {
                    Ok(_) => ToolResult::Success(format!(r#"{{"success": true, "path": "{}"}}"#, path)),
                    Err(e) => ToolResult::Error(e.to_string()),
                }
            }
            "search_code" => {
                let pattern = args["pattern"].as_str().unwrap_or("");
                let dir = args["directory"].as_str().unwrap_or(".");
                match search_in_dir(pattern, dir) {
                    Ok(matches) => ToolResult::Success(serde_json::to_string(&matches).unwrap_or_default()),
                    Err(e) => ToolResult::Error(e.to_string()),
                }
            }
            "execute_command" => {
                let command = args["command"].as_str().unwrap_or("");
                let working_dir = args["working_dir"].as_str().unwrap_or(".");
                let timeout_secs = args["timeout"].as_u64().unwrap_or(30);

                // ── Sandbox: directory validation ──
                let safe_dir = if working_dir == "." || working_dir.is_empty() {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                } else {
                    let p = std::path::PathBuf::from(working_dir);
                    if !p.exists() { return ToolResult::Error(format!("Directory does not exist: {}", working_dir)); }
                    // Resolve to absolute to prevent escape
                    p.canonicalize().unwrap_or(p)
                };

                // ── Sandbox: block dangerous commands ──
                let lower = command.to_lowercase();
                let blocked = ["rm -rf /", "del /f /s", "format ", "shutdown", "reboot",
                               ":(){ :|:& };:", "mkfs", "dd if=", "> /dev/sd"];
                for b in &blocked {
                    if lower.contains(b) {
                        return ToolResult::Error(format!("Blocked dangerous command pattern: {}", b));
                    }
                }

                // ── Sandbox: timeout via tokio process ──
                let result = tokio::runtime::Handle::current().block_on(async {
                    let child = if cfg!(target_os = "windows") {
                        tokio::process::Command::new("cmd")
                            .args(["/C", command])
                            .current_dir(&safe_dir)
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .spawn()
                    } else {
                        tokio::process::Command::new("sh")
                            .args(["-c", command])
                            .current_dir(&safe_dir)
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .spawn()
                    };

                    let child = match child {
                        Ok(c) => c,
                        Err(e) => return ToolResult::Error(format!("Failed to spawn: {}", e)),
                    };

                    match tokio::time::timeout(
                        std::time::Duration::from_secs(timeout_secs),
                        child.wait_with_output(),
                    ).await {
                        Ok(Ok(o)) => {
                            let stdout = String::from_utf8_lossy(&o.stdout);
                            let stderr = String::from_utf8_lossy(&o.stderr);

                            let stdout = if stdout.len() > 102400 {
                                format!("{}...(truncated, {} bytes total)", &stdout[..102400], stdout.len())
                            } else {
                                stdout.to_string()
                            };
                            let stderr = if stderr.len() > 102400 {
                                format!("{}...(truncated)", &stderr[..102400])
                            } else {
                                stderr.to_string()
                            };

                            ToolResult::Success(format!(
                                r#"{{"stdout": "{}", "stderr": "{}", "exit_code": {}}}"#,
                                stdout.escape_default(), stderr.escape_default(), o.status.code().unwrap_or(-1)
                            ))
                        }
                        Ok(Err(e)) => ToolResult::Error(format!("Process error: {}", e)),
                        Err(_elapsed) => {
                            ToolResult::Error(format!("Command timed out after {}s", timeout_secs))
                        }
                    }
                });

                result
            }
            "list_directory" => {
                let path = args["path"].as_str().unwrap_or(".");
                match std::fs::read_dir(path) {
                    Ok(entries) => {
                        let items: Vec<String> = entries
                            .filter_map(|e| e.ok())
                            .map(|e| {
                                let ft = e.file_type().ok().map(|t| if t.is_dir() { "dir" } else { "file" }.to_string()).unwrap_or_default();
                                format!("{} [{}]", e.file_name().to_string_lossy(), ft)
                            })
                            .take(200).collect();
                        ToolResult::Success(format!(r#"{{"items": {}}}"#, serde_json::to_string(&items).unwrap_or_default()))
                    }
                    Err(e) => ToolResult::Error(e.to_string()),
                }
            }
            "fetch_url" => {
                let url = args["url"].as_str().unwrap_or("");
                if url.is_empty() {
                    ToolResult::Error("URL is required".into())
                } else {
                    match crate::tools::browser::fetch_url(url) {
                        Ok(content) => ToolResult::Success(content),
                        Err(e) => ToolResult::Error(e),
                    }
                }
            }
            "lsp_diagnostics" => {
                let file_path = args["file_path"].as_str().unwrap_or("");
                let workspace = args.get("workspace_root")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                if file_path.is_empty() {
                    ToolResult::Error("file_path is required".into())
                } else {
                    let result = crate::tools::lsp_diagnostics::run_diagnostics(file_path, workspace);
                    let content = format!(
                        "LSP diagnostics for {}: {} errors, {} warnings\n\n{}",
                        file_path, result.error_count, result.warning_count, result.diagnostics
                    );
                    ToolResult::Success(content)
                }
            }
            // ── Bulk operations (v0.2.0) ──
            "read_files" => {
                let paths: Vec<String> = args["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();

                if paths.is_empty() {
                    return ToolResult::Error("paths array is required and must not be empty".into());
                }

                let results = tokio::runtime::Handle::current().block_on(
                    BulkFileOps::read_files(&paths)
                );
                ToolResult::Success(BulkFileOps::format_read_results(&results))
            }
            "write_files" => {
                let files: Vec<(String, String)> = args["files"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| {
                                let path = v["path"].as_str()?;
                                let content = v["content"].as_str()?;
                                Some((path.to_string(), content.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                
                if files.is_empty() {
                    return ToolResult::Error("files array is required and must not be empty".into());
                }
                
                let results = tokio::runtime::Handle::current().block_on(
                    BulkFileOps::write_files(&files)
                );
                ToolResult::Success(BulkFileOps::format_write_results(&results))
            }
            // ── Advanced Tools Execution ──
            "generate_tests" => {
                let request: crate::tools::advanced_tools::TestGenerationRequest = match serde_json::from_value(args) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::Error(format!("Invalid request: {}", e)),
                };
                
                let result = crate::tools::advanced_tools::generate_tests(request);
                
                if let Some(test_file) = &result.test_file {
                    let _ = std::fs::write(test_file, &result.test_code);
                }
                
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            "analyze_error" => {
                let request: crate::tools::advanced_tools::DebugAnalysisRequest = match serde_json::from_value(args) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::Error(format!("Invalid request: {}", e)),
                };
                
                let result = crate::tools::advanced_tools::analyze_error(request);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            "refactor_suggest" => {
                let request: crate::tools::advanced_tools::RefactorRequest = match serde_json::from_value(args) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::Error(format!("Invalid request: {}", e)),
                };
                
                let result = crate::tools::advanced_tools::analyze_for_refactoring(request);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            "explain_code" => {
                let request: crate::tools::advanced_tools::ExplainCodeRequest = match serde_json::from_value(args) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::Error(format!("Invalid request: {}", e)),
                };
                
                let result = crate::tools::advanced_tools::explain_code(request);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            // ── C-enhanced Tools (v0.3.0) ──
            "parse_rust_code" => {
                let code = args["code"].as_str().unwrap_or("");
                let result = crate::tools::advanced_tools::parse_rust_code(code);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            "parse_python_code" => {
                let code = args["code"].as_str().unwrap_or("");
                let result = crate::tools::advanced_tools::parse_python_code(code);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            "enhanced_debug_analysis" => {
                let request: crate::tools::advanced_tools::EnhancedDebugRequest = match serde_json::from_value(args) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::Error(format!("Invalid request: {}", e)),
                };
                
                let result = crate::tools::advanced_tools::enhanced_debug_analysis(request);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            "run_tests" => {
                let request: crate::tools::advanced_tools::TestRunRequest = match serde_json::from_value(args) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::Error(format!("Invalid request: {}", e)),
                };
                
                let result = crate::tools::advanced_tools::run_tests(request);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            "detect_framework" => {
                let dir = args["directory"].as_str().unwrap_or(".");
                let result = crate::tools::advanced_tools::detect_framework(dir);
                ToolResult::Success(serde_json::to_string(&result).unwrap_or_else(|_| format!("{:?}", result)))
            }
            other => ToolResult::Error(format!("Unknown tool: {}", other)),
        }
    }
}

// ── Cached Tool Executor (v0.2.0) ──

/// A tool executor that wraps DefaultToolExecutor with a result cache.
/// Same tool + same args → instant return from cache.
pub struct CachedToolExecutor {
    cache: ToolResultCache,
}

impl CachedToolExecutor {
    pub fn new() -> Self {
        Self {
            cache: ToolResultCache::default(),
        }
    }

    pub fn with_config(config: ToolCacheConfig) -> Self {
        Self {
            cache: ToolResultCache::new(config),
        }
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> ToolCacheStats {
        self.cache.stats()
    }

    /// Get cache hit rate.
    pub fn cache_hit_rate(&self) -> f64 {
        self.cache.hit_rate()
    }

    /// Invalidate cache for a specific tool.
    pub fn invalidate_cache(&self, tool_name: &str) {
        self.cache.invalidate_tool(tool_name);
    }
}

#[async_trait]
impl ToolExecutor for CachedToolExecutor {
    async fn execute(&self, name: &str, args_json: &str) -> ToolResult {
        // Check cache first
        if let Some(cached) = self.cache.get(name, args_json) {
            return ToolResult::Success(cached);
        }

        // Execute via default executor
        let result = DefaultToolExecutor.execute(name, args_json).await;

        // Cache the result (only successes)
        if let ToolResult::Success(ref msg) = result {
            self.cache.put(name, args_json, msg);
        }

        result
    }
}

impl Default for CachedToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

fn search_in_dir(pattern: &str, dir: &str) -> anyhow::Result<Vec<String>> {
    let regex = regex::Regex::new(pattern)?;
    let mut results = Vec::new();
    for entry in walkdir::WalkDir::new(dir).max_depth(5).into_iter().filter_map(|e| e.ok()).take(500) {
        if !entry.file_type().is_file() { continue; }
        let ext = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "rs"|"py"|"js"|"ts"|"go"|"java"|"c"|"cpp"|"h"|"toml"|"yaml"|"json"|"md"|"txt") { continue; }
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            for (ln, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(format!("{}:{}: {}", entry.path().display(), ln + 1, line.trim()));
                    if results.len() >= 50 { break; }
                }
            }
        }
        if results.len() >= 50 { break; }
    }
    Ok(results)
}
