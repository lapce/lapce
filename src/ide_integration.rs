//! IDE Integration — Enhanced bidirectional sync between deepseek-carp and Lapce IDE.
//!
//! ## Features
//!
//! - **Session Sync**: Share conversation state between TUI and Lapce GUI
//! - **File Sync**: Real-time file edit notifications
//! - **Cursor Sync**: Share cursor position for context-aware suggestions
//! - **Diagnostic Sync**: Share LSP diagnostics between IDE and AI
//! - **Workspace Sync**: Share workspace state and RAG index
//! - **LSP Integration**: Real-time error monitoring from IDE
//! - **Variable Tracking**: Suggest variables to watch during debugging
//!
//! ## Integration with dscarp-lapce
//!
//! This module provides the bridge between deepseek-carp's AI capabilities
//! and Lapce IDE's rich editing environment.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::tools::lsp_client_v2::{
    LspClientV2, Position as LspPosition,
    Diagnostic as LspDiagnostic, LanguageServerMap,
};

// ═══════════════════════════════════════════════════════════════════════════
// SESSION SYNC
// ═══════════════════════════════════════════════════════════════════════════

/// Shared session state for TUI ↔ Lapce GUI sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSessionState {
    pub session_id: String,
    pub messages: Vec<SyncMessage>,
    pub active_plan: Option<String>,
    pub swarm_status: Option<SwarmSyncStatus>,
    pub last_activity: u64,
}

/// Sync-friendly message format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    pub role: String,
    pub content: String,
    pub timestamp: u64,
    pub metadata: HashMap<String, String>,
}

/// Swarm status for sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmSyncStatus {
    pub total_tasks: usize,
    pub completed: usize,
    pub running: usize,
    pub failed: usize,
}

/// Session sync manager with enhanced features.
pub struct SessionSyncManager {
    sessions: RwLock<HashMap<String, SharedSessionState>>,
    subscribers: RwLock<Vec<Box<dyn SessionSubscriber + Send + Sync>>>,
}

impl SessionSyncManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            subscribers: RwLock::new(Vec::new()),
        }
    }

    /// Update session state and notify subscribers.
    pub fn update_session(&self, state: SharedSessionState) {
        let mut sessions = self.sessions.write().expect("unwrap failed: ide_integration.rs:77");
        sessions.insert(state.session_id.clone(), state.clone());
        drop(sessions);
        
        // Notify all subscribers
        let subscribers = self.subscribers.read().expect("unwrap failed: ide_integration.rs:82");
        for sub in subscribers.iter() {
            sub.on_session_update(&state);
        }
    }

    /// Get session state.
    pub fn get_session(&self, session_id: &str) -> Option<SharedSessionState> {
        let sessions = self.sessions.read().expect("unwrap failed: ide_integration.rs:90");
        sessions.get(session_id).cloned()
    }

    /// Subscribe to session updates.
    pub fn subscribe(&self, subscriber: Box<dyn SessionSubscriber + Send + Sync>) {
        let mut subscribers = self.subscribers.write().expect("unwrap failed: ide_integration.rs:96");
        subscribers.push(subscriber);
    }

    /// List all active sessions.
    pub fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().expect("unwrap failed: ide_integration.rs:102");
        sessions.keys().cloned().collect()
    }

    /// Sync session from Lapce IDE.
    pub fn sync_from_lapce(&self, session_id: &str, messages: Vec<SyncMessage>) {
        if let Some(mut state) = self.get_session(session_id) {
            state.messages.extend(messages);
            state.last_activity = current_timestamp();
            self.update_session(state);
        }
    }

    /// Export session for Lapce.
    pub fn export_for_lapce(&self, session_id: &str) -> Option<String> {
        self.get_session(session_id)
            .map(|s| serde_json::to_string_pretty(&s).unwrap_or_default())
    }
}

impl Default for SessionSyncManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Session update subscriber trait.
pub trait SessionSubscriber: Send + Sync {
    fn on_session_update(&self, state: &SharedSessionState);
}

// ═══════════════════════════════════════════════════════════════════════════
// FILE SYNC
// ═══════════════════════════════════════════════════════════════════════════

/// File change event for IDE ↔ AI sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangeEvent {
    pub path: PathBuf,
    pub change_type: FileChangeType,
    pub content: Option<String>,
    pub timestamp: u64,
    pub source: FileChangeSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileChangeType {
    Created,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileChangeSource {
    Ide,
    Ai,
    External,
}

/// File sync manager for real-time file notifications.
pub struct FileSyncManager {
    watched_files: RwLock<HashMap<PathBuf, u64>>,
    pending_changes: RwLock<Vec<FileChangeEvent>>,
    subscribers: RwLock<Vec<Box<dyn FileSubscriber + Send + Sync>>>,
}

impl FileSyncManager {
    pub fn new() -> Self {
        Self {
            watched_files: RwLock::new(HashMap::new()),
            pending_changes: RwLock::new(Vec::new()),
            subscribers: RwLock::new(Vec::new()),
        }
    }

    /// Watch a file for changes.
    pub fn watch_file(&self, path: PathBuf) {
        let mut watched = self.watched_files.write().expect("unwrap failed: ide_integration.rs:180");
        watched.insert(path, current_timestamp());
    }

    /// Unwatch a file.
    pub fn unwatch_file(&self, path: &PathBuf) {
        let mut watched = self.watched_files.write().expect("unwrap failed: ide_integration.rs:186");
        watched.remove(path);
    }

    /// Notify file change from IDE.
    pub fn notify_ide_change(&self, event: FileChangeEvent) {
        let mut pending = self.pending_changes.write().expect("unwrap failed: ide_integration.rs:192");
        pending.push(event.clone());
        drop(pending);
        
        let subscribers = self.subscribers.read().expect("unwrap failed: ide_integration.rs:196");
        for sub in subscribers.iter() {
            sub.on_file_change(&event);
        }
    }

    /// Notify file change from AI (after edit).
    pub fn notify_ai_change(&self, event: FileChangeEvent) {
        self.notify_ide_change(event);
    }

    /// Get pending changes.
    pub fn get_pending_changes(&self) -> Vec<FileChangeEvent> {
        let mut pending = self.pending_changes.write().expect("unwrap failed: ide_integration.rs:209");
        std::mem::take(&mut *pending)
    }

    /// Subscribe to file changes.
    pub fn subscribe(&self, subscriber: Box<dyn FileSubscriber + Send + Sync>) {
        let mut subscribers = self.subscribers.write().expect("unwrap failed: ide_integration.rs:215");
        subscribers.push(subscriber);
    }

    /// Get all watched files.
    pub fn get_watched_files(&self) -> Vec<PathBuf> {
        let watched = self.watched_files.read().expect("unwrap failed: ide_integration.rs:221");
        watched.keys().cloned().collect()
    }
}

impl Default for FileSyncManager {
    fn default() -> Self {
        Self::new()
    }
}

/// File change subscriber trait.
pub trait FileSubscriber: Send + Sync {
    fn on_file_change(&self, event: &FileChangeEvent);
}

// ═══════════════════════════════════════════════════════════════════════════
// CURSOR SYNC
// ═══════════════════════════════════════════════════════════════════════════

/// Cursor position for context-aware suggestions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPosition {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub selection: Option<(usize, usize)>,
    pub visible_range: Option<(usize, usize)>,
}

/// Cursor sync manager for context-aware AI suggestions.
pub struct CursorSyncManager {
    current_cursor: RwLock<Option<CursorPosition>>,
    history: RwLock<Vec<CursorPosition>>,
}

impl CursorSyncManager {
    pub fn new() -> Self {
        Self {
            current_cursor: RwLock::new(None),
            history: RwLock::new(Vec::new()),
        }
    }

    /// Update cursor position from IDE.
    pub fn update_cursor(&self, pos: CursorPosition) {
        let mut cursor = self.current_cursor.write().expect("unwrap failed: ide_integration.rs:267");
        *cursor = Some(pos.clone());
        drop(cursor);
        
        let mut history = self.history.write().expect("unwrap failed: ide_integration.rs:271");
        history.push(pos);
        
        // Keep only last 100 positions
        if history.len() > 100 {
            let drain_count = history.len() - 100;
            history.drain(0..drain_count);
        }
    }

    /// Get current cursor position.
    pub fn get_cursor(&self) -> Option<CursorPosition> {
        let cursor = self.current_cursor.read().expect("unwrap failed: ide_integration.rs:283");
        cursor.clone()
    }

    /// Get cursor context around current position.
    pub fn get_cursor_context(&self, file_content: &str, context_lines: usize) -> Option<String> {
        let cursor = self.get_cursor()?;
        let lines: Vec<&str> = file_content.lines().collect();
        
        let start = cursor.line.saturating_sub(context_lines);
        let end = (cursor.line + context_lines + 1).min(lines.len());
        
        Some(lines[start..end].join("\n"))
    }

    /// Get cursor history.
    pub fn get_history(&self, limit: usize) -> Vec<CursorPosition> {
        let history = self.history.read().expect("unwrap failed: ide_integration.rs:300");
        history.iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for CursorSyncManager {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DIAGNOSTIC SYNC (LSP INTEGRATION)
// ═══════════════════════════════════════════════════════════════════════════

/// LSP diagnostic for AI context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticInfo {
    pub file: PathBuf,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub source: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

impl DiagnosticSeverity {
    pub fn to_weight(&self) -> f64 {
        match self {
            Self::Error => 4.0,
            Self::Warning => 2.0,
            Self::Info => 1.0,
            Self::Hint => 0.5,
        }
    }
}

/// Diagnostic sync manager for sharing LSP results.
pub struct DiagnosticSyncManager {
    diagnostics: RwLock<HashMap<PathBuf, Vec<DiagnosticInfo>>>,
    error_count: RwLock<usize>,
}

impl DiagnosticSyncManager {
    pub fn new() -> Self {
        Self {
            diagnostics: RwLock::new(HashMap::new()),
            error_count: RwLock::new(0),
        }
    }

    /// Update diagnostics from IDE (LSP).
    pub fn update_diagnostics(&self, file: PathBuf, diags: Vec<DiagnosticInfo>) {
        let mut diagnostics = self.diagnostics.write().expect("unwrap failed: ide_integration.rs:368");
        diagnostics.insert(file, diags.clone());
        drop(diagnostics);
        
        // Update error count
        let mut count = self.error_count.write().expect("unwrap failed: ide_integration.rs:373");
        *count = diags.iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count();
    }

    /// Get diagnostics for a file.
    pub fn get_diagnostics(&self, file: &PathBuf) -> Vec<DiagnosticInfo> {
        let diagnostics = self.diagnostics.read().expect("unwrap failed: ide_integration.rs:381");
        diagnostics.get(file).cloned().unwrap_or_default()
    }

    /// Get all errors (for AI context).
    pub fn get_all_errors(&self) -> Vec<DiagnosticInfo> {
        let diagnostics = self.diagnostics.read().expect("unwrap failed: ide_integration.rs:387");
        diagnostics.values()
            .flat_map(|d| d.iter())
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .cloned()
            .collect()
    }

    /// Get total error count.
    pub fn get_error_count(&self) -> usize {
        *self.error_count.read().expect("unwrap failed: ide_integration.rs:397")
    }

    /// Get diagnostics weighted by severity.
    pub fn get_weighted_diagnostics(&self) -> Vec<(DiagnosticInfo, f64)> {
        let diagnostics = self.diagnostics.read().expect("unwrap failed: ide_integration.rs:402");
        diagnostics.values()
            .flat_map(|d| d.iter())
            .map(|d| (d.clone(), d.severity.to_weight()))
            .collect()
    }

    /// Format diagnostics for AI prompt.
    pub fn format_for_prompt(&self) -> String {
        let errors = self.get_all_errors();
        if errors.is_empty() {
            return "No errors.".to_string();
        }
        
        let mut s = String::from("## Current Errors\n\n");
        for err in &errors {
            s.push_str(&format!(
                "- {}:{}:{}: {} ({})\n",
                err.file.display(),
                err.line,
                err.column,
                err.message,
                err.source
            ));
        }
        s
    }

    /// Clear diagnostics for a file.
    pub fn clear_diagnostics(&self, file: &PathBuf) {
        let mut diagnostics = self.diagnostics.write().expect("unwrap failed: ide_integration.rs:432");
        diagnostics.remove(file);
    }
}

impl Default for DiagnosticSyncManager {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WORKSPACE SYNC
// ═══════════════════════════════════════════════════════════════════════════

/// Workspace state for AI context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub root: PathBuf,
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub git_status: Option<String>,
    pub language_servers: Vec<String>,
}

/// Workspace sync manager.
pub struct WorkspaceSyncManager {
    state: RwLock<Option<WorkspaceState>>,
}

impl WorkspaceSyncManager {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(None),
        }
    }

    /// Update workspace state from IDE.
    pub fn update_state(&self, state: WorkspaceState) {
        let mut ws = self.state.write().expect("unwrap failed: ide_integration.rs:472");
        *ws = Some(state);
    }

    /// Get workspace state.
    pub fn get_state(&self) -> Option<WorkspaceState> {
        let ws = self.state.read().expect("unwrap failed: ide_integration.rs:478");
        ws.clone()
    }

    /// Format workspace context for AI.
    pub fn format_context(&self) -> String {
        let state = match self.get_state() {
            Some(s) => s,
            None => return "No workspace.".to_string(),
        };
        
        let mut ctx = String::new();
        ctx.push_str(&format!("Workspace: {}\n", state.root.display()));
        
        if let Some(ref branch) = state.git_branch {
            ctx.push_str(&format!("Git branch: {}\n", branch));
        }
        
        if !state.open_files.is_empty() {
            ctx.push_str(&format!("Open files: {}\n", state.open_files.len()));
        }
        
        if let Some(ref active) = state.active_file {
            ctx.push_str(&format!("Active file: {}\n", active.display()));
        }
        
        ctx
    }
}

impl Default for WorkspaceSyncManager {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// VARIABLE TRACKING
// ═══════════════════════════════════════════════════════════════════════════

/// Variable information for debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    pub var_type: String,
    pub scope: String,
    pub line: usize,
    pub is_mutable: bool,
}

/// Variable tracking suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableTrackingSuggestion {
    pub variable: VariableInfo,
    pub reason: String,
    pub priority: u8, // 1-5
}

/// Variable tracking manager for debugging assistance.
pub struct VariableTrackingManager {
    variables: RwLock<Vec<VariableInfo>>,
    watch_suggestions: RwLock<Vec<VariableTrackingSuggestion>>,
}

impl VariableTrackingManager {
    pub fn new() -> Self {
        Self {
            variables: RwLock::new(Vec::new()),
            watch_suggestions: RwLock::new(Vec::new()),
        }
    }

    /// Update variables from IDE.
    pub fn update_variables(&self, vars: Vec<VariableInfo>) {
        let mut variables = self.variables.write().expect("unwrap failed: ide_integration.rs:552");
        *variables = vars;
    }

    /// Get all variables.
    pub fn get_variables(&self) -> Vec<VariableInfo> {
        self.variables.read().expect("rwlock poisoned: ide_integration.rs:558").clone()
    }

    /// Add watch suggestion.
    pub fn add_watch_suggestion(&self, suggestion: VariableTrackingSuggestion) {
        let mut suggestions = self.watch_suggestions.write().expect("unwrap failed: ide_integration.rs:563");
        suggestions.push(suggestion);
        
        // Keep only top 20 suggestions
        if suggestions.len() > 20 {
            suggestions.sort_by_key(|b| std::cmp::Reverse(b.priority));
            suggestions.truncate(20);
        }
    }

    /// Get watch suggestions sorted by priority.
    pub fn get_watch_suggestions(&self) -> Vec<VariableTrackingSuggestion> {
        let suggestions = self.watch_suggestions.read().expect("unwrap failed: ide_integration.rs:575");
        let mut sorted = suggestions.clone();
        sorted.sort_by_key(|b| std::cmp::Reverse(b.priority));
        sorted
    }

    /// Clear watch suggestions.
    pub fn clear_watch_suggestions(&self) {
        let mut suggestions = self.watch_suggestions.write().expect("unwrap failed: ide_integration.rs:583");
        suggestions.clear();
    }
}

impl Default for VariableTrackingManager {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// UNIFIED IDE INTEGRATION
// ═══════════════════════════════════════════════════════════════════════════

/// Unified IDE integration manager combining all sync components.
pub struct IdeIntegration {
    pub session: SessionSyncManager,
    pub file: FileSyncManager,
    pub cursor: CursorSyncManager,
    pub diagnostic: DiagnosticSyncManager,
    pub workspace: WorkspaceSyncManager,
    pub variable: VariableTrackingManager,
}

impl IdeIntegration {
    pub fn new() -> Self {
        Self {
            session: SessionSyncManager::new(),
            file: FileSyncManager::new(),
            cursor: CursorSyncManager::new(),
            diagnostic: DiagnosticSyncManager::new(),
            workspace: WorkspaceSyncManager::new(),
            variable: VariableTrackingManager::new(),
        }
    }

    /// Get full context for AI prompt.
    pub fn get_full_context(&self) -> String {
        let mut ctx = String::new();
        
        // Workspace context
        ctx.push_str(&self.workspace.format_context());
        ctx.push('\n');
        
        // Diagnostic context
        ctx.push_str(&self.diagnostic.format_for_prompt());
        ctx.push('\n');
        
        // Cursor context
        if let Some(cursor) = self.cursor.get_cursor() {
            ctx.push_str(&format!(
                "Cursor: {}:{}:{}\n",
                cursor.file.display(),
                cursor.line,
                cursor.column
            ));
        }
        
        // Variable tracking
        let watch_suggestions = self.variable.get_watch_suggestions();
        if !watch_suggestions.is_empty() {
            ctx.push_str("\n## Suggested Watch Variables\n\n");
            for suggestion in watch_suggestions.iter().take(5) {
                ctx.push_str(&format!(
                    "- {} ({}) - {}\n",
                    suggestion.variable.name,
                    suggestion.variable.var_type,
                    suggestion.reason
                ));
            }
        }
        
        ctx
    }

    /// Sync with Lapce IDE.
    pub fn sync_with_lapce(&self, lapce_state: LapceSyncState) {
        // Update workspace
        self.workspace.update_state(WorkspaceState {
            root: lapce_state.root.clone(),
            open_files: lapce_state.open_files.clone(),
            active_file: lapce_state.active_file.clone(),
            git_branch: lapce_state.git_branch.clone(),
            git_status: lapce_state.git_status.clone(),
            language_servers: lapce_state.language_servers.clone(),
        });

        // Update cursor
        if let Some(cursor) = lapce_state.cursor {
            self.cursor.update_cursor(cursor);
        }

        // Update diagnostics
        for (file, diags) in lapce_state.diagnostics {
            self.diagnostic.update_diagnostics(file, diags);
        }

        // Update variables
        if !lapce_state.variables.is_empty() {
            self.variable.update_variables(lapce_state.variables);
        }
    }

    /// Export state for Lapce.
    pub fn export_for_lapce(&self) -> LapceSyncState {
        let workspace = self.workspace.get_state().unwrap_or(WorkspaceState {
            root: PathBuf::new(),
            open_files: vec![],
            active_file: None,
            git_branch: None,
            git_status: None,
            language_servers: vec![],
        });

        LapceSyncState {
            root: workspace.root,
            open_files: workspace.open_files,
            active_file: workspace.active_file,
            git_branch: workspace.git_branch,
            git_status: workspace.git_status,
            language_servers: workspace.language_servers,
            cursor: self.cursor.get_cursor(),
            diagnostics: HashMap::new(),
            variables: vec![],
        }
    }
}

impl Default for IdeIntegration {
    fn default() -> Self {
        Self::new()
    }
}

/// State to sync with Lapce IDE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LapceSyncState {
    pub root: PathBuf,
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub git_status: Option<String>,
    pub language_servers: Vec<String>,
    pub cursor: Option<CursorPosition>,
    pub diagnostics: HashMap<PathBuf, Vec<DiagnosticInfo>>,
    pub variables: Vec<VariableInfo>,
}

// ═══════════════════════════════════════════════════════════════════════════
// LSP HELPER (using LspClientV2)
// ═══════════════════════════════════════════════════════════════════════════

/// LSP helper that wraps LspClientV2 for IDE integration.
pub struct LspHelper {
    client: LspClientV2,
    server_map: LanguageServerMap,
    root_uri: String,
}

impl LspHelper {
    /// Create a new LSP helper with the given language and root URI.
    pub fn new(language: &str, root_uri: &str) -> Self {
        let server_map = LanguageServerMap::new();
        let config = server_map.get_config(language)
            .cloned()
            .unwrap_or_default();
        let mut full_config = config;
        full_config.root_uri = Some(root_uri.to_string());

        Self {
            client: LspClientV2::new(full_config),
            server_map,
            root_uri: root_uri.to_string(),
        }
    }

    /// Get diagnostics for a file via LSP.
    pub async fn get_diagnostics_for_file(&self, uri: &str) -> Vec<LspDiagnostic> {
        self.client.get_diagnostics(uri).await.unwrap_or_default()
    }

    /// Get all cached diagnostics.
    pub async fn get_all_diagnostics(&self) -> HashMap<String, Vec<LspDiagnostic>> {
        self.client.get_all_diagnostics().await
    }

    /// Open a document in the LSP server.
    pub async fn open_document(&self, uri: &str, language_id: &str, content: &str) -> anyhow::Result<()> {
        self.client.open_document(uri, language_id, content).await
    }

    /// Get hover information at a position.
    pub async fn get_hover(&self, uri: &str, line: u32, col: u32) -> Option<String> {
        self.client.get_hover(uri, LspPosition { line, character: col }).await
            .ok()
            .flatten()
            .map(|h| format!("{:?}", h))
    }

    /// Get completion items at a position.
    pub async fn get_completions(&self, uri: &str, line: u32, col: u32) -> usize {
        self.client.get_completion(uri, LspPosition { line, character: col }, None).await
            .ok()
            .and_then(|r| r.map(|c| match c {
                crate::tools::lsp_client_v2::CompletionResult::Array(items) => items.len(),
                crate::tools::lsp_client_v2::CompletionResult::ItemList(list) => list.items.len(),
            }))
            .unwrap_or(0)
    }

    /// Go to definition at a position.
    pub async fn goto_definition(&self, uri: &str, line: u32, col: u32) -> Option<String> {
        self.client.goto_definition(uri, LspPosition { line, character: col }).await
            .ok()
            .flatten()
            .map(|locs| {
                locs.iter().map(|l| format!("{}:{}-{}", l.uri, l.range.start.line, l.range.end.line))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
    }

    /// Convert LSP diagnostics to our DiagnosticInfo format for sync.
    pub fn convert_diagnostics(uri: &str, lsp_diags: &[LspDiagnostic]) -> Vec<DiagnosticInfo> {
        lsp_diags.iter().map(|d| DiagnosticInfo {
            file: PathBuf::from(uri),
            severity: match d.severity {
                Some(crate::tools::lsp_client_v2::DiagnosticSeverity::Error) => DiagnosticSeverity::Error,
                Some(crate::tools::lsp_client_v2::DiagnosticSeverity::Warning) => DiagnosticSeverity::Warning,
                Some(crate::tools::lsp_client_v2::DiagnosticSeverity::Information) => DiagnosticSeverity::Info,
                _ => DiagnosticSeverity::Hint,
            },
            message: d.message.clone(),
            line: d.range.start.line as usize + 1,
            column: d.range.start.character as usize + 1,
            end_line: Some(d.range.end.line as usize + 1),
            end_column: Some(d.range.end.character as usize + 1),
            source: d.source.clone().unwrap_or_else(|| "lsp".to_string()),
            code: d.code.as_ref().map(|c| c.to_string()),
        }).collect()
    }

    /// Get the language server configuration map.
    pub fn server_map(&self) -> &LanguageServerMap {
        &self.server_map
    }

    /// Get the root URI for this LSP session.
    pub fn root_uri(&self) -> &str {
        &self.root_uri
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GLOBAL INSTANCE
// ═══════════════════════════════════════════════════════════════════════════

static IDE_INTEGRATION: std::sync::OnceLock<Arc<IdeIntegration>> = std::sync::OnceLock::new();

/// Get the global IDE integration instance.
pub fn ide_integration() -> Arc<IdeIntegration> {
    IDE_INTEGRATION
        .get_or_init(|| Arc::new(IdeIntegration::new()))
        .clone()
}

/// Helper function to get current timestamp.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════
// LAPCE DIAGNOSTIC BRIDGE — bidirectional sync with dscarp-lapce
// ═══════════════════════════════════════════════════════════════════════════

/// Bridge state for dscarp-lapce integration.
///
/// Provides file-based bidirectional sync:
/// - deepseek-carp writes diagnostics to `.carp/diagnostics/diags.json`
/// - dscarp-lapce reads from that file and pushes to Problem panel
/// - dscarp-lapce writes workspace state to `.carp/workspace/`
/// - deepseek-carp reads workspace state for context-aware planning
pub struct LapceDiagnosticBridge;

impl LapceDiagnosticBridge {
    /// Directory for diagnostic exchange files.
    fn diag_dir(project_root: &std::path::Path) -> std::path::PathBuf {
        project_root.join(".carp").join("diagnostics")
    }

    /// Directory for workspace state exchange.
    fn workspace_dir(project_root: &std::path::Path) -> std::path::PathBuf {
        project_root.join(".carp").join("workspace")
    }

    /// Write latest diagnostics for dscarp-lapce to consume.
    ///
    /// Format: JSON array of VscodeDiagnostic objects.
    /// The Lapce editor plugin polls this file periodically.
    pub fn write_diagnostics(
        project_root: &std::path::Path,
        diagnostics: &[VscodeDiagnostic],
    ) -> anyhow::Result<()> {
        let dir = Self::diag_dir(project_root);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("diags.json");
        let content = serde_json::to_string_pretty(diagnostics)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Read diagnostics written by dscarp-lapce (for AI context).
    pub fn read_diagnostics(project_root: &std::path::Path) -> anyhow::Result<Vec<VscodeDiagnostic>> {
        let path = Self::diag_dir(project_root).join("diags.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)?;
        let diags: Vec<VscodeDiagnostic> = serde_json::from_str(&content)?;
        Ok(diags)
    }

    /// Write workspace state for dscarp-lapce (sent from IDE to Carp).
    pub fn write_workspace_state(
        project_root: &std::path::Path,
        state: &LapceSyncState,
    ) -> anyhow::Result<()> {
        let dir = Self::workspace_dir(project_root);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("state.json");
        let content = serde_json::to_string_pretty(state)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Read workspace state from dscarp-lapce.
    pub fn read_workspace_state(
        project_root: &std::path::Path,
    ) -> anyhow::Result<Option<LapceSyncState>> {
        let path = Self::workspace_dir(project_root).join("state.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let state: LapceSyncState = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    /// Convert a loop summary to VscodeDiagnostic array and write them.
    /// Convenience wrapper for use after `LoopEngine::run_summary()`.
    pub fn push_loop_summary(
        project_root: &std::path::Path,
        target: &str,
        mode: &str,
        summary: &crate::r#loop::LoopSummary,
    ) -> anyhow::Result<()> {
        let diags = summary_to_vscode_diagnostics(summary, target);
        Self::write_diagnostics(project_root, &diags)?;

        // Also write the full diagnostics using the existing bridge
        push_loop_diagnostics(project_root, target, mode, summary, diags)?;

        Ok(())
    }

    /// ── QA / Screenshot Sync (IDE bidirectional bridge) ──

    /// Directory for QA test result exchange.
    fn qa_dir(project_root: &std::path::Path) -> std::path::PathBuf {
        project_root.join(".carp").join("qa")
    }

    /// Directory for screenshot exchange.
    fn screenshots_dir(project_root: &std::path::Path) -> std::path::PathBuf {
        project_root.join(".carp").join("screenshots")
    }

    /// Write QA test results for dscarp-lapce to display in IDE.
    ///
    /// Format: JSON object with test name, status, steps, and screenshot refs.
    /// The Lapce IDE polls this file to show QA results in a dedicated panel.
    pub fn write_qa_results(
        project_root: &std::path::Path,
        suite_name: &str,
        total: usize,
        passed: usize,
        failed: usize,
        details: &[QaResultEntry],
    ) -> anyhow::Result<()> {
        let dir = Self::qa_dir(project_root);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("results.json");

        #[derive(Serialize)]
        struct QaOutput {
            suite: String,
            total: usize,
            passed: usize,
            failed: usize,
            pass_rate_pct: f64,
            timestamp: u64,
            details: Vec<QaResultEntry>,
        }

        let output = QaOutput {
            suite: suite_name.to_string(),
            total,
            passed,
            failed,
            pass_rate_pct: if total > 0 { (passed as f64 / total as f64) * 100.0 } else { 0.0 },
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            details: details.to_vec(),
        };

        let content = serde_json::to_string_pretty(&output)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Read QA results from dscarp-lapce (cross-session access).
    pub fn read_qa_results(
        project_root: &std::path::Path,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let path = Self::qa_dir(project_root).join("results.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;
        Ok(Some(json))
    }
}

/// A single QA result entry for IDE sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaResultEntry {
    pub test_name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub screenshot_ref: Option<String>,
}
#[cfg(test)]
mod lapce_bridge_tests {
    use super::*;

    #[test]
    fn test_lapce_diag_bridge_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let diags = vec![
            VscodeDiagnostic {
                file: "src/main.rs".into(),
                line: 42,
                severity: 0,
                message: "test error".into(),
                source: "deepseek-carp".into(),
            },
        ];

        LapceDiagnosticBridge::write_diagnostics(root, &diags).unwrap();
        let loaded = LapceDiagnosticBridge::read_diagnostics(root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].file, "src/main.rs");
    }

    #[test]
    fn test_lapce_bridge_empty() {
        let dir = tempfile::tempdir().unwrap();
        let diags = LapceDiagnosticBridge::read_diagnostics(dir.path()).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_lapce_workspace_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let state = LapceSyncState {
            root: root.to_path_buf(),
            open_files: vec![root.join("src/main.rs")],
            active_file: Some(root.join("src/main.rs")),
            git_branch: Some("main".into()),
            git_status: Some("clean".into()),
            language_servers: vec!["rust-analyzer".into()],
            cursor: None,
            diagnostics: HashMap::new(),
            variables: vec![],
        };

        LapceDiagnosticBridge::write_workspace_state(root, &state).unwrap();
        let loaded = LapceDiagnosticBridge::read_workspace_state(root).unwrap().unwrap();
        assert_eq!(loaded.git_branch, Some("main".into()));
        assert_eq!(loaded.open_files.len(), 1);
    }
}

/// A single VSCode-compatible diagnostic entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VscodeDiagnostic {
    /// File path (relative to workspace root).
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// Severity: 0=Error, 1=Warning, 2=Info, 3=Hint.
    pub severity: u8,
    /// Diagnostic message.
    pub message: String,
    /// Source label (e.g., "deepseek-carp").
    pub source: String,
}

/// Push LoopEngine results to a VSCode-compatible diagnostics JSON file.
///
/// The file is written to `.carp/diagnostics.json` relative to the given
/// project root. A VSCode extension can watch this file and update the
/// Problems panel accordingly.
///
/// ## Format
///
/// ```json
/// {
///   "version": 1,
///   "target": "src/main.rs",
///   "mode": "review",
///   "passed": false,
///   "total_rounds": 3,
///   "diagnostics": [
///     { "file": "src/main.rs", "line": 42, "severity": 0, "message": "...", "source": "deepseek-carp" }
///   ]
/// }
/// ```
pub fn push_loop_diagnostics(
    project_root: &std::path::Path,
    target: &str,
    mode: &str,
    summary: &crate::r#loop::LoopSummary,
    diags: Vec<VscodeDiagnostic>,
) -> anyhow::Result<()> {
    let dir = project_root.join(".carp");
    std::fs::create_dir_all(&dir)?;

    let payload = serde_json::json!({
        "version": 1,
        "target": target,
        "mode": mode,
        "passed": summary.passed,
        "total_rounds": summary.total_rounds,
        "total_time_ms": summary.total_time_ms,
        "diagnostics": diags,
    });

    let path = dir.join("diagnostics.json");
    std::fs::write(&path, serde_json::to_string_pretty(&payload)?)?;
    tracing::info!("VSCode diagnostics written to {}", path.display());
    Ok(())
}

/// Convert a loop run summary into VSCode diagnostics entries.
///
/// Failed rounds become Error diagnostics, successful ones become Info.
/// Compilation errors from the evaluator verdict are extracted as errors.
pub fn summary_to_vscode_diagnostics(summary: &crate::r#loop::LoopSummary, target: &str) -> Vec<VscodeDiagnostic> {
    let mut diags = Vec::new();

    for result in &summary.results {
        let (severity, message) = match &result.verdict {
            crate::r#loop::LoopVerdict::Failed { reason } => {
                (0u8, format!("Round {}: {}", result.round, reason))
            }
            crate::r#loop::LoopVerdict::Aborted { reason } => {
                (1u8, format!("Round {} aborted: {}", result.round, reason))
            }
            crate::r#loop::LoopVerdict::Passed => {
                (2u8, format!("Round {} passed", result.round))
            }
        };
        diags.push(VscodeDiagnostic {
            file: target.to_string(),
            line: 1,
            severity,
            message,
            source: "deepseek-carp".into(),
        });
    }

    // If the run failed overall, add a top-level error
    if !summary.passed {
        diags.push(VscodeDiagnostic {
            file: target.to_string(),
            line: 1,
            severity: 0,
            message: format!(
                "Loop run failed after {} rounds ({} ms)",
                summary.total_rounds, summary.total_time_ms
            ),
            source: "deepseek-carp".into(),
        });
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ide_integration() {
        let ide = IdeIntegration::new();
        
        // Test cursor sync
        let cursor = CursorPosition {
            file: PathBuf::from("test.rs"),
            line: 10,
            column: 5,
            selection: None,
            visible_range: None,
        };
        ide.cursor.update_cursor(cursor);
        
        assert!(ide.cursor.get_cursor().is_some());
    }

    #[test]
    fn test_diagnostic_weighting() {
        let error = DiagnosticSeverity::Error;
        let warning = DiagnosticSeverity::Warning;
        
        assert!(error.to_weight() > warning.to_weight());
    }
}
