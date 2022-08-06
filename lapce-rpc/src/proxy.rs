use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crossbeam_channel::{Receiver, Sender};
use lsp_types::{
    request::GotoTypeDefinitionResponse, CodeActionResponse, CompletionItem,
    DocumentSymbolResponse, GotoDefinitionResponse, Hover, InlayHint, Location,
    Position, SemanticTokensResult, SymbolInformation, TextDocumentItem, TextEdit,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use xi_rope::RopeDelta;

use crate::{
    buffer::BufferId,
    file::FileNodeItem,
    plugin::{PluginDescription, PluginId},
    source_control::FileDiff,
    style::SemanticStyles,
    terminal::TermId,
    RequestId, RpcError,
};

enum ProxyRpcMessage {
    Request(RequestId, CoreProxyRequest),
    Notification(CoreProxyNotification),
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreProxyRequest {
    NewBuffer {
        buffer_id: BufferId,
        path: PathBuf,
    },
    BufferHead {
        path: PathBuf,
    },
    GetCompletion {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
    GlobalSearch {
        pattern: String,
    },
    CompletionResolve {
        plugin_id: PluginId,
        completion_item: Box<CompletionItem>,
    },
    GetHover {
        request_id: usize,
        path: PathBuf,
        position: Position,
    },
    GetSignature {
        buffer_id: BufferId,
        position: Position,
    },
    GetReferences {
        path: PathBuf,
        position: Position,
    },
    GetDefinition {
        request_id: usize,
        path: PathBuf,
        position: Position,
    },
    GetTypeDefinition {
        request_id: usize,
        path: PathBuf,
        position: Position,
    },
    GetInlayHints {
        path: PathBuf,
    },
    GetSemanticTokens {
        path: PathBuf,
    },
    GetCodeActions {
        path: PathBuf,
        position: Position,
    },
    GetDocumentSymbols {
        path: PathBuf,
    },
    GetWorkspaceSymbols {
        /// The search query
        query: String,
    },
    GetDocumentFormatting {
        path: PathBuf,
    },
    GetOpenFilesContent {},
    GetFiles {
        path: String,
    },
    ReadDir {
        path: PathBuf,
    },
    Save {
        rev: u64,
        path: PathBuf,
    },
    SaveBufferAs {
        buffer_id: BufferId,
        path: PathBuf,
        rev: u64,
        content: String,
    },
    CreateFile {
        path: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    TrashPath {
        path: PathBuf,
    },
    RenamePath {
        from: PathBuf,
        to: PathBuf,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreProxyNotification {
    Initialize {
        workspace: Option<PathBuf>,
        plugin_configurations: HashMap<String, serde_json::Value>,
    },
    OpenFileChanged {
        path: PathBuf,
    },
    Shutdown {},
    Completion {
        request_id: usize,
        path: PathBuf,
        input: String,
        position: Position,
    },
    Update {
        path: PathBuf,
        delta: RopeDelta,
        rev: u64,
    },
    NewTerminal {
        term_id: TermId,
        cwd: Option<PathBuf>,
        shell: String,
    },
    InstallPlugin {
        plugin: PluginDescription,
    },
    DisablePlugin {
        plugin: PluginDescription,
    },
    EnablePlugin {
        plugin: PluginDescription,
    },
    RemovePlugin {
        plugin: PluginDescription,
    },
    GitCommit {
        message: String,
        diffs: Vec<FileDiff>,
    },
    GitCheckout {
        branch: String,
    },
    GitDiscardFilesChanges {
        files: Vec<PathBuf>,
    },
    GitDiscardWorkspaceChanges {},
    GitInit {},
    TerminalWrite {
        term_id: TermId,
        content: String,
    },
    TerminalResize {
        term_id: TermId,
        width: usize,
        height: usize,
    },
    TerminalClose {
        term_id: TermId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreProxyResponse {
    NewBufferResponse {
        content: String,
    },
    BufferHeadResponse {
        version: String,
        content: String,
    },
    ReadDirResponse {
        items: HashMap<PathBuf, FileNodeItem>,
    },
    CompletionResolveResponse {
        item: Box<CompletionItem>,
    },
    HoverResponse {
        request_id: usize,
        hover: Hover,
    },
    GetDefinitionResponse {
        request_id: usize,
        definition: GotoDefinitionResponse,
    },
    GetTypeDefinition {
        request_id: usize,
        definition: GotoTypeDefinitionResponse,
    },
    GetReferencesResponse {
        references: Vec<Location>,
    },
    GetCodeActionsResponse {
        resp: CodeActionResponse,
    },
    GetFilesResponse {
        items: Vec<PathBuf>,
    },
    GetDocumentFormatting {
        edits: Vec<TextEdit>,
    },
    GetDocumentSymbols {
        resp: DocumentSymbolResponse,
    },
    GetWorkspaceSymbols {
        symbols: Vec<SymbolInformation>,
    },
    GetInlayHints {
        hints: Vec<InlayHint>,
    },
    GetSemanticTokens {
        styles: SemanticStyles,
    },
    GetOpenFilesContentResponse {
        items: Vec<TextDocumentItem>,
    },
    GlobalSearchResponse {
        matches: HashMap<PathBuf, Vec<(usize, (usize, usize), String)>>,
    },
    Success {},
    SaveResponse {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadDirResponse {
    pub items: HashMap<PathBuf, FileNodeItem>,
}

pub trait ProxyCallback: Send {
    fn call(self: Box<Self>, result: Result<CoreProxyResponse, RpcError>);
}

impl<F: Send + FnOnce(Result<CoreProxyResponse, RpcError>)> ProxyCallback for F {
    fn call(self: Box<F>, result: Result<CoreProxyResponse, RpcError>) {
        (*self)(result)
    }
}

enum ResponseHandler {
    Callback(Box<dyn ProxyCallback>),
    Chan(Sender<Result<CoreProxyResponse, RpcError>>),
}

impl ResponseHandler {
    fn invoke(self, result: Result<CoreProxyResponse, RpcError>) {
        match self {
            ResponseHandler::Callback(f) => f.call(result),
            ResponseHandler::Chan(tx) => {
                let _ = tx.send(result);
            }
        }
    }
}

pub trait ProxyHandler {
    fn handle_notification(&mut self, rpc: CoreProxyNotification);
    fn handle_request(&mut self, id: RequestId, rpc: CoreProxyRequest);
}

#[derive(Clone)]
pub struct ProxyRpcHandler {
    tx: Sender<ProxyRpcMessage>,
    rx: Receiver<ProxyRpcMessage>,
    id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, ResponseHandler>>>,
}

impl ProxyRpcHandler {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            tx,
            rx,
            id: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn mainloop<H>(&self, handler: &mut H)
    where
        H: ProxyHandler,
    {
        use ProxyRpcMessage::*;
        for msg in &self.rx {
            match msg {
                Request(id, request) => {
                    handler.handle_request(id, request);
                }
                Notification(notification) => {
                    handler.handle_notification(notification);
                }
                Shutdown => {
                    return;
                }
            }
        }
    }

    fn request_common(&self, request: CoreProxyRequest, rh: ResponseHandler) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, rh);
        }
        let _ = self.tx.send(ProxyRpcMessage::Request(id, request));
    }

    fn request(
        &self,
        request: CoreProxyRequest,
    ) -> Result<CoreProxyResponse, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.request_common(request, ResponseHandler::Chan(tx));
        rx.recv().unwrap_or_else(|_| {
            Err(RpcError {
                code: 0,
                message: "io error".to_string(),
            })
        })
    }

    fn request_async(
        &self,
        request: CoreProxyRequest,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_common(request, ResponseHandler::Callback(Box::new(f)))
    }

    pub fn handle_response(
        &self,
        id: RequestId,
        result: Result<CoreProxyResponse, RpcError>,
    ) {
        let handler = { self.pending.lock().remove(&id) };
        if let Some(handler) = handler {
            handler.invoke(result);
        }
    }

    pub fn notification(&self, notification: CoreProxyNotification) {
        let _ = self.tx.send(ProxyRpcMessage::Notification(notification));
    }

    pub fn git_init(&self) {
        self.notification(CoreProxyNotification::GitInit {});
    }

    pub fn git_commit(&self, message: String, diffs: Vec<FileDiff>) {
        self.notification(CoreProxyNotification::GitCommit { message, diffs });
    }

    pub fn git_checkout(&self, branch: String) {
        self.notification(CoreProxyNotification::GitCheckout { branch });
    }

    pub fn install_plugin(&self, plugin: PluginDescription) {
        self.notification(CoreProxyNotification::InstallPlugin { plugin });
    }

    pub fn disable_plugin(&self, plugin: PluginDescription) {
        self.notification(CoreProxyNotification::DisablePlugin { plugin });
    }

    pub fn enable_plugin(&self, plugin: PluginDescription) {
        self.notification(CoreProxyNotification::EnablePlugin { plugin });
    }

    pub fn remove_plugin(&self, plugin: PluginDescription) {
        self.notification(CoreProxyNotification::RemovePlugin { plugin });
    }

    pub fn shutdown(&self) {
        self.notification(CoreProxyNotification::Shutdown {});
        let _ = self.tx.send(ProxyRpcMessage::Shutdown);
    }

    pub fn initialize(
        &self,
        workspace: Option<PathBuf>,
        plugin_configurations: HashMap<String, serde_json::Value>,
    ) {
        self.notification(CoreProxyNotification::Initialize {
            workspace,
            plugin_configurations,
        });
    }

    pub fn completion(
        &self,
        request_id: usize,
        path: PathBuf,
        input: String,
        position: Position,
    ) {
        self.notification(CoreProxyNotification::Completion {
            request_id,
            path,
            input,
            position,
        });
    }

    pub fn new_terminal(
        &self,
        term_id: TermId,
        cwd: Option<PathBuf>,
        shell: String,
    ) {
        self.notification(CoreProxyNotification::NewTerminal {
            term_id,
            cwd,
            shell,
        })
    }

    pub fn terminal_close(&self, term_id: TermId) {
        self.notification(CoreProxyNotification::TerminalClose { term_id });
    }

    pub fn terminal_resize(&self, term_id: TermId, width: usize, height: usize) {
        self.notification(CoreProxyNotification::TerminalResize {
            term_id,
            width,
            height,
        });
    }

    pub fn terminal_write(&self, term_id: TermId, content: &str) {
        self.notification(CoreProxyNotification::TerminalWrite {
            term_id,
            content: content.to_string(),
        });
    }

    pub fn new_buffer(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::NewBuffer { buffer_id, path }, f);
    }

    pub fn get_buffer_head(
        &self,
        _buffer_id: BufferId,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::BufferHead { path }, f);
    }

    pub fn create_file(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::CreateFile { path }, f);
    }

    pub fn create_directory(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::CreateDirectory { path }, f);
    }

    pub fn trash_path(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::TrashPath { path }, f);
    }

    pub fn rename_path(
        &self,
        from: PathBuf,
        to: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::RenamePath { from, to }, f);
    }

    pub fn save_buffer_as(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        rev: u64,
        content: String,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(
            CoreProxyRequest::SaveBufferAs {
                buffer_id,
                path,
                rev,
                content,
            },
            f,
        );
    }

    pub fn global_search(&self, pattern: String, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::GlobalSearch { pattern }, f);
    }

    pub fn save(&self, rev: u64, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::Save { rev, path }, f);
    }

    pub fn get_files(&self, f: impl ProxyCallback + 'static) {
        self.request_async(
            CoreProxyRequest::GetFiles {
                path: "path".into(),
            },
            f,
        );
    }

    pub fn get_open_files_content(&self) -> Result<CoreProxyResponse, RpcError> {
        self.request(CoreProxyRequest::GetOpenFilesContent {})
    }

    pub fn read_dir(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::ReadDir { path }, f);
    }

    pub fn completion_resolve(
        &self,
        plugin_id: PluginId,
        completion_item: CompletionItem,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(
            CoreProxyRequest::CompletionResolve {
                plugin_id,
                completion_item: Box::new(completion_item),
            },
            f,
        );
    }

    pub fn get_hover(
        &self,
        request_id: usize,
        path: PathBuf,
        position: Position,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(
            CoreProxyRequest::GetHover {
                request_id,
                path,
                position,
            },
            f,
        );
    }

    pub fn get_definition(
        &self,
        request_id: usize,
        path: PathBuf,
        position: Position,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(
            CoreProxyRequest::GetDefinition {
                request_id,
                path,
                position,
            },
            f,
        );
    }

    pub fn get_type_definition(
        &self,
        request_id: usize,
        path: PathBuf,
        position: Position,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(
            CoreProxyRequest::GetTypeDefinition {
                request_id,
                path,
                position,
            },
            f,
        );
    }

    pub fn get_references(
        &self,
        path: PathBuf,
        position: Position,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::GetReferences { path, position }, f);
    }

    pub fn get_code_actions(
        &self,
        path: PathBuf,
        position: Position,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::GetCodeActions { path, position }, f);
    }

    pub fn get_document_formatting(
        &self,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::GetDocumentFormatting { path }, f);
    }

    pub fn get_semantic_tokens(
        &self,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::GetSemanticTokens { path }, f);
    }

    pub fn get_document_symbols(
        &self,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::GetDocumentSymbols { path }, f);
    }

    pub fn get_workspace_symbols(
        &self,
        query: String,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::GetWorkspaceSymbols { query }, f);
    }

    pub fn get_inlay_hints(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::GetInlayHints { path }, f);
    }

    pub fn update(&self, path: PathBuf, delta: RopeDelta, rev: u64) {
        self.notification(CoreProxyNotification::Update { path, delta, rev });
    }

    pub fn git_discard_files_changes(&self, files: Vec<PathBuf>) {
        self.notification(CoreProxyNotification::GitDiscardFilesChanges { files });
    }

    pub fn git_discard_workspace_changes(&self) {
        self.notification(CoreProxyNotification::GitDiscardWorkspaceChanges {});
    }
}

impl Default for ProxyRpcHandler {
    fn default() -> Self {
        Self::new()
    }
}
