use std::{
    collections::HashMap,
    path::{Path, PathBuf},
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
    core::{CoreRequest, CoreResponse},
    file::FileNodeItem,
    plugin::{PluginDescription, PluginId, PluginResponse},
    source_control::FileDiff,
    terminal::TermId,
    RequestId, RpcError, RpcMessage,
};

pub type CoreProxyRpcMessage =
    RpcMessage<CoreProxyRequest, CoreProxyNotification, CoreProxyResponse>;

pub enum ProxyRpcMessage {
    Core(RpcMessage<CoreProxyRequest, CoreProxyNotification, ProxyResponse>),
    Plugin(RpcMessage<CoreProxyRequest, CoreProxyNotification, ProxyResponse>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreProxyNotification {
    Initialize {
        workspace: PathBuf,
    },
    Shutdown {},
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
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginProxyRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginProxyNotification {
    StartLspServer {
        exec_path: String,
        language_id: String,
        options: Option<Value>,
        system_lsp: Option<bool>,
    },
    DownloadFile {
        url: String,
        path: PathBuf,
    },
    LockFile {
        path: PathBuf,
    },
    MakeFileExecutable {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginProxyResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadDirResponse {
    pub items: HashMap<PathBuf, FileNodeItem>,
}

pub trait NewCallback<Resp>: Send {
    fn call(self: Box<Self>, result: Result<Resp, RpcError>);
}

impl<Resp, F: Send + FnOnce(Result<Resp, RpcError>)> NewCallback<Resp> for F {
    fn call(self: Box<F>, result: Result<Resp, RpcError>) {
        (*self)(result)
    }
}

pub trait ProxyCallback: Send {
    fn call(self: Box<Self>, result: Result<CoreProxyResponse, RpcError>);
}

impl<F: Send + FnOnce(Result<CoreProxyResponse, RpcError>)> ProxyCallback for F {
    fn call(self: Box<F>, result: Result<CoreProxyResponse, RpcError>) {
        (*self)(result)
    }
}

enum NewResponseHandler<Resp> {
    Callback(Box<dyn NewCallback<Resp>>),
}

impl<Resp> NewResponseHandler<Resp> {
    fn invoke(self, result: Result<Resp, RpcError>) {
        match self {
            NewResponseHandler::Callback(f) => f.call(result),
        }
    }
}

pub trait ProxyHandler {
    fn handle_notification(&mut self, rpc: CoreProxyNotification);
    fn handle_request(&mut self, id: RequestId, rpc: CoreProxyRequest);
}

#[derive(Clone)]
pub struct ProxyRpcHandler {
    tx: Sender<CoreProxyRpcMessage>,
    rx: Receiver<CoreProxyRpcMessage>,
    id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, u64>>>,
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
        for msg in &self.rx {
            match msg {
                RpcMessage::Request(_, _) => todo!(),
                RpcMessage::Response(_, _) => todo!(),
                RpcMessage::Notification(_) => todo!(),
                RpcMessage::Error(_, _) => todo!(),
            }
        }
    }

    fn send_request_async(&self, request: CoreProxyRequest, f: impl ProxyCallback) {}

    pub fn handle_response(&self, response: Result<CoreProxyResponse, RpcError>) {}

    pub fn send_notification(&self, notification: CoreProxyNotification) {
        let _ = self.tx.send(RpcMessage::Notification(notification));
    }

    pub fn shutdown(&self) {
        self.send_notification(CoreProxyNotification::Shutdown {});
    }

    pub fn initialize(&self, workspace: Option<PathBuf>) {
        self.send_notification(CoreProxyNotification::Initialize { workspace });
    }

    pub fn new_buffer(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.send_request_async(CoreProxyRequest::NewBuffer { buffer_id, path }, f);
    }

    pub fn get_buffer_head(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: impl ProxyCallback + 'static,
    ) {
        self.send_request_async(CoreProxyRequest::BufferHead { buffer_id, path }, f);
    }

    pub fn get_files(&self, f: impl ProxyCallback + 'static) {
        self.send_request_async(
            CoreProxyRequest::GetFiles {
                path: "path".into(),
            },
            f,
        );
    }

    pub fn read_dir(&self, path: PathBuf, f: impl ProxyCallback + 'static) {
        self.send_request_async(CoreProxyRequest::ReadDir { path }, f);
    }
}
