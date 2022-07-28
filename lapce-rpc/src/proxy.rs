use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crossbeam_channel::{Receiver, Sender};
use lsp_types::{CompletionItem, Position};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xi_rope::RopeDelta;

use crate::{
    buffer::BufferId,
    core::CoreResponse,
    file::FileNodeItem,
    plugin::{PluginDescription, PluginId, PluginResponse},
    source_control::FileDiff,
    terminal::TermId,
    RequestId, RpcError, RpcMessage,
};

enum ProxyRpcMessage {
    Request(RequestId, CoreProxyRequest),
    Notification(CoreProxyNotification),
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
        buffer_id: BufferId,
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
        buffer_id: BufferId,
        completion_item: Box<CompletionItem>,
    },
    GetHover {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
    GetSignature {
        buffer_id: BufferId,
        position: Position,
    },
    GetReferences {
        buffer_id: BufferId,
        position: Position,
    },
    GetDefinition {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
    GetTypeDefinition {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
    GetInlayHints {
        buffer_id: BufferId,
    },
    GetSemanticTokens {
        buffer_id: BufferId,
    },
    GetCodeActions {
        buffer_id: BufferId,
        position: Position,
    },
    GetDocumentSymbols {
        buffer_id: BufferId,
    },
    GetWorkspaceSymbols {
        /// The search query
        query: String,
        /// THe id of the buffer it was used in, which tells us what LSP to query
        buffer_id: BufferId,
    },
    GetDocumentFormatting {
        buffer_id: BufferId,
    },
    GetFiles {
        path: String,
    },
    ReadDir {
        path: PathBuf,
    },
    Save {
        rev: u64,
        buffer_id: BufferId,
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
    },
    Shutdown {},
    Completion {
        request_id: usize,
        path: PathBuf,
        position: Position,
    },
    Update {
        buffer_id: BufferId,
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
    GitCommit {
        message: String,
        diffs: Vec<FileDiff>,
    },
    GitCheckout {
        branch: String,
    },
    GitDiscardFileChanges {
        file: PathBuf,
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
    GetFilesResponse {
        items: Vec<PathBuf>,
    },
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
}

impl ResponseHandler {
    fn invoke(self, result: Result<CoreProxyResponse, RpcError>) {
        match self {
            ResponseHandler::Callback(f) => f.call(result),
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
            }
        }
    }

    fn request_async(
        &self,
        request: CoreProxyRequest,
        f: impl ProxyCallback + 'static,
    ) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, ResponseHandler::Callback(Box::new(f)));
        }
        let _ = self.tx.send(ProxyRpcMessage::Request(id, request));
    }

    pub fn handle_response(
        &self,
        id: RequestId,
        result: Result<CoreProxyResponse, RpcError>,
    ) {
        if let Some(handler) = { self.pending.lock().remove(&id) } {
            handler.invoke(result);
        }
    }

    pub fn notification(&self, notification: CoreProxyNotification) {
        let _ = self.tx.send(ProxyRpcMessage::Notification(notification));
    }

    pub fn shutdown(&self) {
        self.notification(CoreProxyNotification::Shutdown {});
    }

    pub fn initialize(&self, workspace: Option<PathBuf>) {
        self.notification(CoreProxyNotification::Initialize { workspace });
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
        buffer_id: BufferId,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.request_async(CoreProxyRequest::BufferHead { buffer_id, path }, f);
    }

    pub fn get_files(&self, f: impl ProxyCallback + 'static) {
        self.request_async(
            CoreProxyRequest::GetFiles {
                path: "path".into(),
            },
            f,
        );
    }

    pub fn read_dir(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.request_async(CoreProxyRequest::ReadDir { path }, f);
    }
}
