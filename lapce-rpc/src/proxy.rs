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
use xi_rope::RopeDelta;

use crate::{
    buffer::BufferId,
    core::{CoreRequest, CoreResponse},
    file::FileNodeItem,
    plugin::PluginDescription,
    source_control::FileDiff,
    terminal::TermId,
    RequestId, RpcError, RpcMessage,
};

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
pub enum PluginProxyNotification {}

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

pub trait NewHandler<Req, Notif, Resp> {
    fn handle_notification(&mut self, rpc: Notif);
    fn handle_request(&mut self, rpc: Req);
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

#[derive(Clone)]
pub struct ProxyRpcHandler<Resp> {
    sender: Sender<ProxyRpcMessage>,
    id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, NewResponseHandler<Resp>>>>,
}

impl<Resp> ProxyRpcHandler<Resp> {
    pub fn new(sender: Sender<ProxyRpcMessage>) -> Self {
        Self {
            sender,
            id: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn mainloop<H, Req, Notif>(
        &mut self,
        receiver: Receiver<RpcMessage<Req, Notif, Resp>>,
        handler: &mut H,
    ) where
        H: NewHandler<Req, Notif, Resp>,
    {
        for msg in receiver {
            match msg {
                RpcMessage::Request(id, request) => {
                    handler.handle_request(request);
                }
                RpcMessage::Notification(notification) => {
                    handler.handle_notification(notification);
                }
                RpcMessage::Response(id, resp) => {
                    self.handle_response(id, Ok(resp));
                }
                RpcMessage::Error(id, err) => {
                    self.handle_response(id, Err(err));
                }
            }
        }
    }

    fn handle_response(&self, id: u64, resp: Result<Resp, RpcError>) {
        let handler = {
            let mut pending = self.pending.lock();
            pending.remove(&id)
        };
        if let Some(responsehandler) = handler {
            responsehandler.invoke(resp)
        }
    }

    pub fn send_core_request_async(
        &self,
        req: CoreProxyRequest,
        f: Box<dyn NewCallback<Resp>>,
    ) {
        self.send_core_request_common(req, NewResponseHandler::Callback(f));
    }

    fn send_core_request_common(
        &self,
        req: CoreProxyRequest,
        rh: NewResponseHandler<Resp>,
    ) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, rh);
        }
        let msg = ProxyRpcMessage::Core(RpcMessage::Request(id, req));
        if let Err(_e) = self.sender.send(msg) {
            let mut pending = self.pending.lock();
            if let Some(rh) = pending.remove(&id) {
                rh.invoke(Err(RpcError {
                    code: 0,
                    message: "io error".to_string(),
                }));
            }
        }
    }

    pub fn send_core_notification(&self, notification: CoreProxyNotification) {
        let msg = ProxyRpcMessage::Core(RpcMessage::Notification(notification));
        if let Err(_e) = self.sender.send(msg) {}
    }
}
