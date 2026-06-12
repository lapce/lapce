//!
//! Based on Claude Code's LSP implementation, this module provides:
//! - Multi-language server support
//! - Diagnostic synchronization
//! - Code completion with context
//! - Go to definition
//! - Hover information
//! - Symbol search and indexing
//! - Real-time sync with IDE

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Helper macro to create inclusive ranges for LSP completion priorities
macro_rules! range {
    ($range:expr) => ($range);
}

/// LSP Message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRequest {
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<LspError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspError {
    pub code: i32,
    pub message: String,
}

/// LSP Notification types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum LspNotification {
    #[serde(rename = "textDocument/publishDiagnostics")]
    PublishDiagnostics(PublishDiagnosticsParams),
    #[serde(rename = "window/showMessage")]
    ShowMessage(ShowMessageParams),
    #[serde(rename = "telemetry/event")]
    Telemetry(TelemetryEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishDiagnosticsParams {
    pub uri: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<DiagnosticSeverity>,
    pub code: Option<serde_json::Value>,
    pub source: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowMessageParams {
    #[serde(rename = "type")]
    pub message_type: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub data: serde_json::Value,
}

/// LSP Capabilities
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LspCapabilities {
    pub text_document_sync: Option<TextDocumentSyncCapability>,
    pub hover_provider: bool,
    pub completion_provider: Option<CompletionOptions>,
    pub definition_provider: bool,
    pub references_provider: bool,
    pub document_symbol_provider: bool,
    pub workspace_symbol_provider: bool,
    pub code_action_provider: bool,
    pub execute_command_provider: Option<ExecuteCommandOptions>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextDocumentSyncCapability {
    pub kind: TextDocumentSyncKind,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum TextDocumentSyncKind {
    None = 0,
    Full = 1,
    Incremental = 2,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompletionOptions {
    pub trigger_characters: Vec<String>,
    pub resolve_provider: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteCommandOptions {
    pub commands: Vec<String>,
}

/// LSP Server Configuration
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub command: Vec<String>,
    pub root_uri: Option<String>,
    pub initialization_options: Option<serde_json::Value>,
    pub workspace_folder: Option<String>,
    pub environment: HashMap<String, String>,
}

impl Default for LspServerConfig {
    fn default() -> Self {
        Self {
            command: vec!["rust-analyzer".to_string()],
            root_uri: None,
            initialization_options: None,
            workspace_folder: None,
            environment: HashMap::new(),
        }
    }
}

/// Language to LSP server mapping
pub struct LanguageServerMap {
    servers: HashMap<String, LspServerConfig>,
}

impl LanguageServerMap {
    pub fn new() -> Self {
        let mut servers = HashMap::new();

        // Rust - rust-analyzer
        servers.insert(
            "rust".to_string(),
            LspServerConfig {
                command: vec!["rust-analyzer".to_string()],
                ..Default::default()
            },
        );

        // TypeScript/JavaScript - typescript-language-server
        servers.insert(
            "typescript".to_string(),
            LspServerConfig {
                command: vec!["typescript-language-server".to_string(), "--stdio".to_string()],
                ..Default::default()
            },
        );
        servers.insert(
            "javascript".to_string(),
            LspServerConfig {
                command: vec!["javascript-typescript-stdio".to_string()],
                ..Default::default()
            },
        );

        // Python - pyright
        servers.insert(
            "python".to_string(),
            LspServerConfig {
                command: vec!["pyright-langserver".to_string(), "--stdio".to_string()],
                ..Default::default()
            },
        );

        // Go - gopls
        servers.insert(
            "go".to_string(),
            LspServerConfig {
                command: vec!["gopls".to_string()],
                ..Default::default()
            },
        );

        // C/C++ - clangd
        servers.insert(
            "c".to_string(),
            LspServerConfig {
                command: vec!["clangd".to_string()],
                ..Default::default()
            },
        );
        servers.insert(
            "cpp".to_string(),
            LspServerConfig {
                command: vec!["clangd".to_string()],
                ..Default::default()
            },
        );

        Self { servers }
    }

    pub fn get_config(&self, language: &str) -> Option<&LspServerConfig> {
        self.servers.get(language)
    }

    pub fn register(&mut self, language: String, config: LspServerConfig) {
        self.servers.insert(language, config);
    }
}

impl Default for LanguageServerMap {
    fn default() -> Self {
        Self::new()
    }
}

/// LSP Client V2 - Enhanced client with diagnostic sync
pub struct LspClientV2 {
    config: LspServerConfig,
    capabilities: Arc<RwLock<Option<LspCapabilities>>>,
    diagnostics: Arc<RwLock<HashMap<String, Vec<Diagnostic>>>>,
    pending_requests: Arc<RwLock<HashMap<u64, tokio::sync::oneshot::Sender<LspResponse>>>>,
    document_sync: Arc<RwLock<HashMap<String, DocumentState>>>,
    symbol_index: Arc<RwLock<SymbolIndex>>,
}

#[derive(Debug, Clone)]
pub struct DocumentState {
    pub uri: String,
    pub content: String,
    pub version: i32,
    pub language_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    pub documents: HashMap<String, DocumentSymbols>,
    pub workspace_symbols: Vec<WorkspaceSymbol>,
}

#[derive(Debug, Clone)]
pub struct DocumentSymbols {
    pub uri: String,
    pub symbols: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[repr(u32)]
pub enum SymbolKind {
    File = 1,
    Module = 2,
    Namespace = 3,
    Package = 4,
    Class = 5,
    Method = 6,
    Property = 7,
    Field = 8,
    Constructor = 9,
    Enum = 10,
    Interface = 11,
    Function = 12,
    Variable = 13,
    Constant = 14,
    String = 15,
    Number = 16,
    Boolean = 17,
    Array = 18,
    Object = 19,
    Key = 20,
    Null = 21,
    EnumMember = 22,
    Event = 23,
    Operator = 24,
    TypeParameter = 25,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub container_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

impl LspClientV2 {
    pub fn new(config: LspServerConfig) -> Self {
        Self {
            config,
            capabilities: Arc::new(RwLock::new(None)),
            diagnostics: Arc::new(RwLock::new(HashMap::new())),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            document_sync: Arc::new(RwLock::new(HashMap::new())),
            symbol_index: Arc::new(RwLock::new(SymbolIndex::default())),
        }
    }

    /// Initialize the LSP server
    pub async fn initialize(&self, root_uri: &str) -> anyhow::Result<LspCapabilities> {
        let params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": self.client_capabilities(),
        });

        // Send initialize request
        let response = self.send_request("initialize", params).await?;

        // Extract capabilities from response
        let capabilities = response
            .get("capabilities")
            .and_then(|c| serde_json::from_value::<LspCapabilities>(c.clone()).ok())
            .unwrap_or_default();

        *self.capabilities.write().await = Some(capabilities.clone());

        // Send initialized notification
        self.send_notification("initialized", serde_json::json!({})).await?;

        Ok(capabilities)
    }

    /// Get client capabilities
    fn client_capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "textDocument": {
                "publishDiagnostics": {
                    "relatedInformation": true,
                    "versionSupport": true,
                    "tagSupport": {
                        "valueSet": [1, 2]
                    }
                },
                "synchronization": {
                    "willSave": true,
                    "willSaveWaitUntil": true,
                    "didSave": true,
                    "didChange": {
                        "dynamicRegistration": true,
                        "willSave": true,
                        "didSave": true,
                        "didChange": {
                            "textDocumentSyncKind": 2
                        }
                    }
                },
                "completion": {
                    "dynamicRegistration": true,
                    "completionItem": {
                        "snippetSupport": true,
                        "commitCharactersSupport": true,
                        "parameterInformation": {
                            "labelOffsetSupport": true
                        }
                    },
                    "contextSupport": true
                },
                "hover": {
                    "dynamicRegistration": true,
                    "contentFormat": ["markdown", "plaintext"]
                },
                "definition": {
                    "dynamicRegistration": true,
                    "linkSupport": true
                },
                "references": {
                    "dynamicRegistration": true
                },
                "documentSymbol": {
                    "dynamicRegistration": true,
                    "hierarchicalDocumentSymbolSupport": true,
                    "symbolKind": {
                        "valueSet": range!(1..=25)
                    }
                },
                "codeAction": {
                    "dynamicRegistration": true,
                    "codeActionLiteralSupport": {
                        "codeActionKind": {
                            "valueSet": ["quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"]
                        }
                    }
                }
            },
            "workspace": {
                "applyEdit": true,
                "workspaceEdit": {
                    "documentChanges": true,
                    "resourceOperations": ["create", "rename", "delete"]
                },
                "symbol": {
                    "dynamicRegistration": true,
                    "symbolKind": {
                        "valueSet": range!(1..=26)
                    }
                },
                "executeCommand": {
                    "dynamicRegistration": true
                }
            }
        })
    }

    /// Open a document
    pub async fn open_document(&self, uri: &str, language_id: &str, content: &str) -> anyhow::Result<()> {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": content
            }
        });

        // Store document state
        {
            let mut docs = self.document_sync.write().await;
            docs.insert(
                uri.to_string(),
                DocumentState {
                    uri: uri.to_string(),
                    content: content.to_string(),
                    version: 1,
                    language_id: language_id.to_string(),
                },
            );
        }

        self.send_notification("textDocument/didOpen", params).await?;
        Ok(())
    }

    /// Update a document (incremental sync)
    pub async fn update_document(&self, uri: &str, changes: Vec<TextDocumentContentChangeEvent>) -> anyhow::Result<()> {
        // Update document state
        {
            let mut docs = self.document_sync.write().await;
            if let Some(doc) = docs.get_mut(uri) {
                for change in &changes {
                    if let Some(range) = &change.range {
                        // Apply incremental change
                        doc.content = Self::apply_text_change(&doc.content, range, &change.text);
                    } else {
                        // Full document replacement
                        doc.content = change.text.clone();
                    }
                }
                doc.version += 1;
            }
        }

        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "version": self.document_sync.read().await.get(uri).map(|d| d.version).unwrap_or(1)
            },
            "contentChanges": changes.iter().map(|c| {
                if let Some(range) = &c.range {
                    serde_json::json!({
                        "range": range,
                        "rangeLength": c.range_length.unwrap_or(c.text.len() as u32),
                        "text": c.text
                    })
                } else {
                    serde_json::json!({
                        "text": c.text
                    })
                }
            }).collect::<Vec<_>>()
        });

        self.send_notification("textDocument/didChange", params).await?;
        Ok(())
    }

    /// Apply a text change to content
    fn apply_text_change(content: &str, range: &Range, new_text: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let start_line = range.start.line as usize;
        let end_line = range.end.line as usize;
        let start_char = range.start.character as usize;
        let end_char = range.end.character as usize;

        if start_line >= lines.len() {
            return content.to_string();
        }

        let start_line_text = lines[start_line];
        let end_line_text = if end_line < lines.len() {
            lines[end_line]
        } else {
            ""
        };

        let before = &start_line_text[..start_char.min(start_line_text.len())];
        let after = &end_line_text[end_char.min(end_line_text.len())..];

        let mut result = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i < start_line {
                result.push_str(line);
                result.push('\n');
            } else if i == start_line {
                result.push_str(before);
                result.push_str(new_text);
                result.push_str(after);
            } else if i > end_line {
                result.push('\n');
                result.push_str(line);
            }
        }

        result
    }

    /// Get hover information
    pub async fn get_hover(&self, uri: &str, position: Position) -> anyhow::Result<Option<HoverResult>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": position
        });

        let response = self.send_request("textDocument/hover", params).await?;

        if let Some(result) = response.get("contents") {
            let hover = serde_json::from_value::<HoverResult>(result.clone())
                .unwrap_or_else(|_| HoverResult {
                    contents: vec![HoverContent::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: result.as_str().unwrap_or("").to_string(),
                    })],
                    range: None,
                });
            Ok(Some(hover))
        } else {
            Ok(None)
        }
    }

    /// Get completion items
    pub async fn get_completion(&self, uri: &str, position: Position, context: Option<CompletionContext>) -> anyhow::Result<Option<CompletionResult>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": position,
            "context": context
        });

        let response = self.send_request("textDocument/completion", params).await?;

        if !response.is_null() {
            let completion = serde_json::from_value::<CompletionResult>(response)?;
            Ok(Some(completion))
        } else {
            Ok(None)
        }
    }

    /// Go to definition
    pub async fn goto_definition(&self, uri: &str, position: Position) -> anyhow::Result<Option<Vec<Location>>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": position
        });

        let response = self.send_request("textDocument/definition", params).await?;

        if let Some(result) = response.get(0) {
            let locations = serde_json::from_value::<Vec<Location>>(response.clone())
                .unwrap_or_else(|_| {
                    vec![serde_json::from_value(result.clone()).unwrap_or(Location {
                        uri: uri.to_string(),
                        range: Range {
                            start: position,
                            end: position,
                        },
                    })]
                });
            Ok(Some(locations))
        } else {
            Ok(None)
        }
    }

    /// Find references
    pub async fn find_references(&self, uri: &str, position: Position) -> anyhow::Result<Option<Vec<Location>>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": position,
            "context": {
                "includeDeclaration": true
            }
        });

        let response = self.send_request("textDocument/references", params).await?;

        if !response.is_null() {
            let locations = serde_json::from_value::<Vec<Location>>(response)?;
            Ok(Some(locations))
        } else {
            Ok(None)
        }
    }

    /// Get document symbols
    pub async fn get_document_symbols(&self, uri: &str) -> anyhow::Result<Vec<DocumentSymbol>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });

        let response = self.send_request("textDocument/documentSymbol", params).await?;

        if !response.is_null() {
            let symbols = serde_json::from_value::<Vec<DocumentSymbol>>(response)?;
            
            // Update symbol index
            {
                let mut index = self.symbol_index.write().await;
                index.documents.insert(
                    uri.to_string(),
                    DocumentSymbols {
                        uri: uri.to_string(),
                        symbols: symbols.clone(),
                    },
                );
            }

            Ok(symbols)
        } else {
            Ok(vec![])
        }
    }

    /// Search workspace symbols
    pub async fn search_workspace_symbols(&self, query: &str) -> anyhow::Result<Vec<WorkspaceSymbol>> {
        let params = serde_json::json!({
            "query": query
        });

        let response = self.send_request("workspace/symbol", params).await?;

        if !response.is_null() {
            let symbols = serde_json::from_value::<Vec<WorkspaceSymbol>>(response)?;
            
            // Update workspace symbol index
            {
                let mut index = self.symbol_index.write().await;
                index.workspace_symbols = symbols.clone();
            }

            Ok(symbols)
        } else {
            Ok(vec![])
        }
    }

    /// Get cached diagnostics for a document
    pub async fn get_diagnostics(&self, uri: &str) -> Option<Vec<Diagnostic>> {
        let diagnostics = self.diagnostics.read().await;
        diagnostics.get(uri).cloned()
    }

    /// Get all cached diagnostics
    pub async fn get_all_diagnostics(&self) -> HashMap<String, Vec<Diagnostic>> {
        self.diagnostics.read().await.clone()
    }

    /// Handle diagnostic notification
    pub async fn handle_diagnostics(&self, params: PublishDiagnosticsParams) {
        let mut diagnostics = self.diagnostics.write().await;
        diagnostics.insert(params.uri.clone(), params.diagnostics);
    }

    /// Execute a code action
    pub async fn execute_code_action(&self, uri: &str, range: Range, diagnostics: Vec<Diagnostic>) -> anyhow::Result<Option<Vec<CodeAction>>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "range": range,
            "context": {
                "diagnostics": diagnostics,
                "triggerKind": 2
            }
        });

        let response = self.send_request("textDocument/codeAction", params).await?;

        if !response.is_null() {
            let actions = serde_json::from_value::<Vec<CodeAction>>(response)?;
            Ok(Some(actions))
        } else {
            Ok(None)
        }
    }

    /// Send request and wait for response
    async fn send_request(&self, _method: &str, _params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        // In a real implementation, this would communicate with the LSP server
        // For now, return an empty response
        Ok(serde_json::Value::Null)
    }

    /// Send notification (no response expected)
    async fn send_notification(&self, _method: &str, _params: serde_json::Value) -> anyhow::Result<()> {
        // In a real implementation, this would communicate with the LSP server
        Ok(())
    }

    /// Shutdown the LSP server
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.send_request("shutdown", serde_json::json!({})).await?;
        self.send_notification("exit", serde_json::json!({})).await?;
        Ok(())
    }

    /// Get the LSP server configuration.
    pub fn config(&self) -> &LspServerConfig {
        &self.config
    }

    /// Get the number of pending LSP requests.
    pub async fn pending_request_count(&self) -> usize {
        self.pending_requests.read().await.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocumentContentChangeEvent {
    pub range: Option<Range>,
    pub range_length: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverResult {
    pub contents: Vec<HoverContent>,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoverContent {
    MarkupContent(MarkupContent),
    PlainString(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkupContent {
    pub kind: MarkupKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkupKind {
    Plaintext,
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionContext {
    pub trigger_kind: CompletionTriggerKind,
    pub trigger_character: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u32)]
pub enum CompletionTriggerKind {
    Invoked = 1,
    TriggerCharacter = 2,
    TriggerForIncompleteCompletions = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompletionResult {
    Array(Vec<CompletionItem>),
    ItemList(CompletionList),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionList {
    pub is_incomplete: bool,
    pub items: Vec<CompletionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: Option<CompletionItemKind>,
    pub detail: Option<String>,
    pub documentation: Option<serde_json::Value>,
    pub insert_text: Option<String>,
    pub insert_text_format: Option<InsertTextFormat>,
    pub text_edit: Option<TextEdit>,
    pub command: Option<Command>,
    pub commit_characters: Option<Vec<String>>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u32)]
pub enum CompletionItemKind {
    Text = 1,
    Method = 2,
    Function = 3,
    Constructor = 4,
    Field = 5,
    Variable = 6,
    Class = 7,
    Interface = 8,
    Module = 9,
    Property = 10,
    Unit = 11,
    Value = 12,
    Enum = 13,
    Keyword = 14,
    Snippet = 15,
    Color = 16,
    File = 17,
    Reference = 18,
    Folder = 19,
    EnumMember = 20,
    Constant = 21,
    Struct = 22,
    Event = 23,
    Operator = 24,
    TypeParameter = 25,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u32)]
pub enum InsertTextFormat {
    PlainText = 1,
    Snippet = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub title: String,
    pub command: String,
    pub arguments: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAction {
    pub title: String,
    pub kind: Option<CodeActionKind>,
    pub diagnostics: Option<Vec<Diagnostic>>,
    pub edit: Option<WorkspaceEdit>,
    pub command: Option<Command>,
    pub is_preferred: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodeActionKind {
    QuickFix,
    Refactor,
    RefactorExtract,
    RefactorInline,
    RefactorRewrite,
    Source,
    SourceOrganizeImports,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEdit {
    pub changes: Option<HashMap<String, Vec<TextEdit>>>,
    pub document_changes: Option<Vec<DocumentChange>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DocumentChange {
    TextEdit(TextEdit),
    CreateFile(CreateFile),
    RenameFile(RenameFile),
    DeleteFile(DeleteFile),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFile {
    pub kind: String,
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameFile {
    pub kind: String,
    pub old_uri: String,
    pub new_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteFile {
    pub kind: String,
    pub uri: String,
}

// =====================================================================
// LapceBridge section
// =====================================================================

/// Lapce LSP bridge -?connect deepseek-carp to Lapce's own LSP process.
///
/// Lapce runs one language server per file-type over JSON-RPC on stdio.
/// This bridge spawns the server, talks JSON-RPC, and exposes async methods.
#[derive(Debug, Clone)]
pub struct LapceLspBridge {
    pub server_name: String,
    pub workspace_root: std::path::PathBuf,
}

impl LapceLspBridge {
    pub fn new(server_name: &str, workspace_root: std::path::PathBuf) -> Self {
        Self {
            server_name: server_name.to_string(),
            workspace_root,
        }
    }

    pub fn spawn_command(&self) -> anyhow::Result<std::process::Command> {
        match self.server_name.as_str() {
            "rust-analyzer" => Ok(std::process::Command::new("rust-analyzer")),
            "tsserver" => {
                let mut cmd = std::process::Command::new("typescript-language-server");
                cmd.arg("--stdio");
                Ok(cmd)
            }
            "pyright" => {
                let mut cmd = std::process::Command::new("pyright-langserver");
                cmd.arg("--stdio");
                Ok(cmd)
            }
            other => Err(anyhow::anyhow!(
                "LapceBridge: no spawn rule for server '{other}'"
            )),
        }
    }

    /// Minimal detection: if project has Cargo.toml -> rust-analyzer;
    /// has package.json -> tsserver; has pyproject.toml -> pyright; etc.
    pub fn detect_for(root: &std::path::Path) -> String {
        let cargo = root.join("Cargo.toml");
        let package = root.join("package.json");
        let pyproject = root.join("pyproject.toml");
        let requirements = root.join("requirements.txt");
        let go_mod = root.join("go.mod");
        let build_gradle = root.join("build.gradle");
        let cmake = root.join("CMakeLists.txt");

        if cargo.exists() {
            "rust-analyzer".to_string()
        } else if package.exists() {
            "tsserver".to_string()
        } else if pyproject.exists() || requirements.exists() {
            "pyright".to_string()
        } else if go_mod.exists() {
            "gopls".to_string()
        } else if build_gradle.exists() {
            "kotlin-language-server".to_string()
        } else if cmake.exists() {
            "clangd".to_string()
        } else {
            "".to_string()
        }
    }
}

/// Raw JSON-RPC envelope used by the bridge.
#[derive(Debug, Clone, Default)]
pub struct LspRpcMessage {
    pub id: Option<u64>,
    pub method: Option<String>,
    pub result: Option<serde_json::Value>,
    pub params: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

impl std::str::FromStr for LspRpcMessage {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        // Find Content-Length header
        let mut header_end = None;
        for i in 0..bytes.len().saturating_sub(3) {
            if bytes[i] == b'\r' && bytes[i + 1] == b'\n' && bytes[i + 2] == b'\r'
                && i + 3 < bytes.len() && bytes[i + 3] == b'\n'
            {
                header_end = Some(i);
                break;
            }
        }
        let header_end = header_end.ok_or(())?;

        let header_text = &s[..header_end];
        let mut content_length = None;
        for line in header_text.split("\r\n") {
            if let Some(rest) = line.strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
            {
                content_length = Some(rest.trim().parse::<usize>().map_err(|_| ())?);
                break;
            }
        }
        let content_length = content_length.ok_or(())?;

        let body_start = header_end + 4;
        let body_end = body_start + content_length;
        if body_end > s.len() {
            return Err(());
        }
        let body = &s[body_start..body_end];

        let v: serde_json::Value = serde_json::from_str(body).map_err(|_| ())?;

        let id = v.get("id").and_then(|x| {
            if let Some(n) = x.as_u64() {
                Some(n)
            } else if let Some(s) = x.as_str() {
                s.parse::<u64>().ok()
            } else {
                None
            }
        });
        let method = v.get("method").and_then(|x| x.as_str().map(|s| s.to_string()));
        let result = v.get("result").cloned();
        let params = v.get("params").cloned();
        let error = v.get("error").cloned();

        Ok(LspRpcMessage {
            id,
            method,
            result,
            params,
            error,
        })
    }
}

/// Encode a JSON-RPC message with Content-Length framing.
pub fn encode_message(
    id: Option<u64>,
    method: Option<&str>,
    params: Option<&serde_json::Value>,
    result: Option<&serde_json::Value>,
    error: Option<&serde_json::Value>,
) -> String {
    let mut map = serde_json::Map::new();
    map.insert("jsonrpc".to_string(), serde_json::Value::String("2.0".to_string()));
    if let Some(id) = id {
        map.insert("id".to_string(), serde_json::Value::from(id));
    }
    if let Some(m) = method {
        map.insert("method".to_string(), serde_json::Value::String(m.to_string()));
    }
    if let Some(p) = params {
        map.insert("params".to_string(), p.clone());
    }
    if let Some(r) = result {
        map.insert("result".to_string(), r.clone());
    }
    if let Some(e) = error {
        map.insert("error".to_string(), e.clone());
    }

    let body = serde_json::Value::Object(map);
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let len = body_str.len();
    format!("Content-Length: {}\r\n\r\n{}", len, body_str)
}

/// Try to decode a raw JSON-RPC frame (handles Content-Length header).
pub fn try_parse_message(text: &str) -> Option<LspRpcMessage> {
    text.parse::<LspRpcMessage>().ok()
}

// ---------------------------------------------------------------------
// LSP notification / request builders (return the full wire text).
// ---------------------------------------------------------------------

pub fn did_open(uri: &str, lang: &str, content: &str, version: u32) -> String {
    let params = serde_json::json!({
        "textDocument": {
            "uri": uri,
            "languageId": lang,
            "version": version,
            "text": content,
        }
    });
    encode_message(None, Some("textDocument/didOpen"), Some(&params), None, None)
}

pub fn did_change(uri: &str, version: u32, new_text: &str) -> String {
    let params = serde_json::json!({
        "textDocument": { "uri": uri, "version": version },
        "contentChanges": [ { "text": new_text } ]
    });
    encode_message(None, Some("textDocument/didChange"), Some(&params), None, None)
}

pub fn did_save(uri: &str) -> String {
    let params = serde_json::json!({
        "textDocument": { "uri": uri }
    });
    encode_message(None, Some("textDocument/didSave"), Some(&params), None, None)
}

pub fn did_close(uri: &str) -> String {
    let params = serde_json::json!({
        "textDocument": { "uri": uri }
    });
    encode_message(None, Some("textDocument/didClose"), Some(&params), None, None)
}

pub fn completion(id: u64, uri: &str, line: u32, col: u32) -> String {
    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": col }
    });
    encode_message(Some(id), Some("textDocument/completion"), Some(&params), None, None)
}

pub fn definition(id: u64, uri: &str, line: u32, col: u32) -> String {
    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": col }
    });
    encode_message(Some(id), Some("textDocument/definition"), Some(&params), None, None)
}




/// Minimal async JSON-RPC bridge that spawns an LSP subprocess and provides
/// `send` / `recv` over tokio Child stdin/stdout. This is the glue between
/// `LapceLspBridge` and `LspBridge`  it actually talks JSON-RPC.
pub struct LapceBridgeConn {
    pub bridge: LapceLspBridge,
    pub child: tokio::process::Child,
    pub next_id: u64,
    pub pending: std::collections::HashMap<u64, tokio::sync::oneshot::Sender<LspRpcMessage>>,
    pub buffer: String,
}

impl LapceBridgeConn {
    pub async fn spawn(bridge: LapceLspBridge) -> anyhow::Result<Self> {
        let mut cmd_shell = bridge.spawn_command()?;
        let mut tokio_cmd = tokio::process::Command::new(cmd_shell.get_program());
        tokio_cmd.args(cmd_shell.get_args().collect::<Vec<_>>());
        tokio_cmd.stdout(std::process::Stdio::piped());
        tokio_cmd.stdin(std::process::Stdio::piped());
        let child = tokio_cmd.spawn()?;
        Ok(Self {
            bridge,
            child,
            next_id: 1,
            pending: std::collections::HashMap::new(),
            buffer: String::new(),
        })
    }

    pub async fn send_request(&mut self, method: &str, params: serde_json::Value) -> anyhow::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let frame = encode_message(Some(id), Some(method), Some(&params), None, None);
        if let Some(mut stdin) = self.child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(frame.as_bytes()).await?;
            self.child.stdin = Some(stdin);
        }
        Ok(id)
    }

    pub async fn drain_to_end(&mut self) -> anyhow::Result<Vec<LspRpcMessage>> {
        use tokio::io::AsyncReadExt;
        if let Some(mut stdout) = self.child.stdout.take() {
            let mut raw = String::new();
            stdout.read_to_string(&mut raw).await?;
            self.child.stdout = Some(stdout);
            Ok(raw.lines().filter_map(|l| l.trim().parse::<LspRpcMessage>().ok()).collect())
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        let _ = self.send_request("shutdown", serde_json::json!({})).await;
        let _ = self.child.wait().await;
        Ok(())
    }
}

