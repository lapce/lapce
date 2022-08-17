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
    Position, SymbolInformation, TextDocumentItem, TextEdit,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use xi_rope::RopeDelta;

use crate::{
    buffer::BufferId,
    file::FileNodeItem,
    plugin::{PluginId, VoltInfo, VoltMetadata},
    source_control::FileDiff,
    style::SemanticStyles,
    terminal::TermId,
    RequestId, RpcError,
};

pub enum ProxyRpc {
    Request(RequestId, ProxyRequest),
    Notification(ProxyNotification),
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum ProxyRequest {
    NewBuffer {
        buffer_id: BufferId,
        path: PathBuf,
    },
    BufferHead {
        path: PathBuf,
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
pub enum ProxyNotification {
    Initialize {
        workspace: Option<PathBuf>,
        disabled_volts: Vec<String>,
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
    InstallVolt {
        volt: VoltInfo,
    },
    RemoveVolt {
        volt: VoltMetadata,
    },
    DisableVolt {
        volt: VoltInfo,
    },
    EnableVolt {
        volt: VoltInfo,
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
pub enum ProxyResponse {
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
        #[allow(clippy::type_complexity)]
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
    fn call(self: Box<Self>, result: Result<ProxyResponse, RpcError>);
}

impl<F: Send + FnOnce(Result<ProxyResponse, RpcError>)> ProxyCallback for F {
    fn call(self: Box<F>, result: Result<ProxyResponse, RpcError>) {
        (*self)(result)
    }
}

enum ResponseHandler {
    Callback(Box<dyn ProxyCallback>),
    Chan(Sender<Result<ProxyResponse, RpcError>>),
}

impl ResponseHandler {
    fn invoke(self, result: Result<ProxyResponse, RpcError>) {
        match self {
            ResponseHandler::Callback(f) => f.call(result),
            ResponseHandler::Chan(tx) => {
                let _ = tx.send(result);
            }
        }
    }
}

pub trait ProxyHandler {
    fn handle_notification(&mut self, rpc: ProxyNotification);
    fn handle_request(&mut self, id: RequestId, rpc: ProxyRequest);
}

#[derive(Clone)]
pub struct ProxyRpcHandler {
    tx: Sender<ProxyRpc>,
    rx: Receiver<ProxyRpc>,
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

    pub fn rx(&self) -> &Receiver<ProxyRpc> {
        &self.rx
    }

    pub fn mainloop<H>(&self, handler: &mut H)
    where
        H: ProxyHandler,
    {
        use ProxyRpc::*;
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

    fn request_common(&self, request: ProxyRequest, rh: ResponseHandler) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, rh);
        }
        let _ = self.tx.send(ProxyRpc::Request(id, request));
    }

    fn request(&self, request: ProxyRequest) -> Result<ProxyResponse, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.request_common(request, ResponseHandler::Chan(tx));
        rx.recv().unwrap_or_else(|_| {
            Err(RpcError {
                code: 0,
                message: "io error".to_string(),
            })
        })
    }

    pub fn request_async(
        &self,
        request: ProxyRequest,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_common(request, ResponseHandler::Callback(Box::new(f)))
    }

    pub fn handle_response(
        &self,
        id: RequestId,
        result: Result<ProxyResponse, RpcError>,
    ) {
        let handler = { self.pending.lock().remove(&id) };
        if let Some(handler) = handler {
            handler.invoke(result);
        }
    }

    pub fn notification(&self, notification: ProxyNotification) {
        let _ = self.tx.send(ProxyRpc::Notification(notification));
    }

    pub fn git_init(&self) {
        self.notification(ProxyNotification::GitInit {});
    }

    pub fn git_commit(&self, message: String, diffs: Vec<FileDiff>) {
        self.notification(ProxyNotification::GitCommit { message, diffs });
    }

    pub fn git_checkout(&self, branch: String) {
        self.notification(ProxyNotification::GitCheckout { branch });
    }

    pub fn install_volt(&self, volt: VoltInfo) {
        self.notification(ProxyNotification::InstallVolt { volt });
    }

    pub fn remove_volt(&self, volt: VoltMetadata) {
        self.notification(ProxyNotification::RemoveVolt { volt });
    }

    pub fn disable_volt(&self, volt: VoltInfo) {
        self.notification(ProxyNotification::DisableVolt { volt });
    }

    pub fn enable_volt(&self, volt: VoltInfo) {
        self.notification(ProxyNotification::EnableVolt { volt });
    }

    pub fn shutdown(&self) {
        self.notification(ProxyNotification::Shutdown {});
        let _ = self.tx.send(ProxyRpc::Shutdown);
    }

    pub fn initialize(
        &self,
        workspace: Option<PathBuf>,
        disabled_volts: Vec<String>,
        plugin_configurations: HashMap<String, serde_json::Value>,
    ) {
        self.notification(ProxyNotification::Initialize {
            workspace,
            disabled_volts,
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
        self.notification(ProxyNotification::Completion {
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
        self.notification(ProxyNotification::NewTerminal {
            term_id,
            cwd,
            shell,
        })
    }

    pub fn terminal_close(&self, term_id: TermId) {
        self.notification(ProxyNotification::TerminalClose { term_id });
    }

    pub fn terminal_resize(&self, term_id: TermId, width: usize, height: usize) {
        self.notification(ProxyNotification::TerminalResize {
            term_id,
            width,
            height,
        });
    }

    pub fn terminal_write(&self, term_id: TermId, content: &str) {
        self.notification(ProxyNotification::TerminalWrite {
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
        self.request_async(ProxyRequest::NewBuffer { buffer_id, path }, f);
    }

    pub fn get_buffer_head(
        &self,
        _buffer_id: BufferId,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(ProxyRequest::BufferHead { path }, f);
    }

    pub fn create_file(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(ProxyRequest::CreateFile { path }, f);
    }

    pub fn create_directory(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(ProxyRequest::CreateDirectory { path }, f);
    }

    pub fn trash_path(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(ProxyRequest::TrashPath { path }, f);
    }

    pub fn rename_path(
        &self,
        from: PathBuf,
        to: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(ProxyRequest::RenamePath { from, to }, f);
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
            ProxyRequest::SaveBufferAs {
                buffer_id,
                path,
                rev,
                content,
            },
            f,
        );
    }

    pub fn global_search(&self, pattern: String, f: impl ProxyCallback + 'static) {
        self.request_async(ProxyRequest::GlobalSearch { pattern }, f);
    }

    pub fn save(&self, rev: u64, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(ProxyRequest::Save { rev, path }, f);
    }

    pub fn get_files(&self, f: impl ProxyCallback + 'static) {
        self.request_async(
            ProxyRequest::GetFiles {
                path: "path".into(),
            },
            f,
        );
    }

    pub fn get_open_files_content(&self) -> Result<ProxyResponse, RpcError> {
        self.request(ProxyRequest::GetOpenFilesContent {})
    }

    pub fn read_dir(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(ProxyRequest::ReadDir { path }, f);
    }

    pub fn completion_resolve(
        &self,
        plugin_id: PluginId,
        completion_item: CompletionItem,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(
            ProxyRequest::CompletionResolve {
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
            ProxyRequest::GetHover {
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
            ProxyRequest::GetDefinition {
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
            ProxyRequest::GetTypeDefinition {
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
        self.request_async(ProxyRequest::GetReferences { path, position }, f);
    }

    pub fn get_code_actions(
        &self,
        path: PathBuf,
        position: Position,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(ProxyRequest::GetCodeActions { path, position }, f);
    }

    pub fn get_document_formatting(
        &self,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(ProxyRequest::GetDocumentFormatting { path }, f);
    }

    pub fn get_semantic_tokens(
        &self,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(ProxyRequest::GetSemanticTokens { path }, f);
    }

    pub fn get_document_symbols(
        &self,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(ProxyRequest::GetDocumentSymbols { path }, f);
    }

    pub fn get_workspace_symbols(
        &self,
        query: String,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(ProxyRequest::GetWorkspaceSymbols { query }, f);
    }

    pub fn get_inlay_hints(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(ProxyRequest::GetInlayHints { path }, f);
    }

    pub fn update(&self, path: PathBuf, delta: RopeDelta, rev: u64) {
        self.notification(ProxyNotification::Update { path, delta, rev });
    }

    pub fn git_discard_files_changes(&self, files: Vec<PathBuf>) {
        self.notification(ProxyNotification::GitDiscardFilesChanges { files });
    }

    pub fn git_discard_workspace_changes(&self) {
        self.notification(ProxyNotification::GitDiscardWorkspaceChanges {});
    }
}

impl Default for ProxyRpcHandler {
    fn default() -> Self {
        Self::new()
    }
}
