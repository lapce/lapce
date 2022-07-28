use crossbeam_channel::{Receiver, Sender};
use lsp_types::{CompletionResponse, ProgressParams, PublishDiagnosticsParams};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicU64, Arc},
};

use crate::{
    file::FileNodeItem, plugin::PluginDescription, source_control::DiffInfo,
    terminal::TermId, RequestId, RpcError,
};

enum CoreRpc {
    Request(RequestId, CoreRequest),
    Notification(CoreNotification),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreNotification {
    ProxyConnected {},
    OpenFileChanged {
        path: PathBuf,
        content: String,
    },
    CompletionResponse {
        request_id: usize,
        resp: CompletionResponse,
    },
    ReloadBuffer {
        path: PathBuf,
        content: String,
        rev: u64,
    },
    WorkspaceFileChange {},
    PublishDiagnostics {
        diagnostics: PublishDiagnosticsParams,
    },
    WorkDoneProgress {
        progress: ProgressParams,
    },
    HomeDir {
        path: PathBuf,
    },
    InstalledPlugins {
        plugins: HashMap<String, PluginDescription>,
    },
    DisabledPlugins {
        plugins: HashMap<String, PluginDescription>,
    },
    ListDir {
        items: Vec<FileNodeItem>,
    },
    DiffFiles {
        files: Vec<PathBuf>,
    },
    DiffInfo {
        diff: DiffInfo,
    },
    UpdateTerminal {
        term_id: TermId,
        content: String,
    },
    CloseTerminal {
        term_id: TermId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreResponse {}

pub trait CoreHandler {
    fn handle_notification(&mut self, rpc: CoreNotification);
    fn handle_request(&mut self, rpc: CoreRequest);
}

#[derive(Clone)]
pub struct CoreRpcHandler {
    tx: Sender<CoreRpc>,
    rx: Receiver<CoreRpc>,
    id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, u64>>>,
}

impl CoreRpcHandler {
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
        H: CoreHandler,
    {
        for msg in &self.rx {
            match msg {
                CoreRpc::Request(id, rpc) => {
                    handler.handle_request(rpc);
                }
                CoreRpc::Notification(rpc) => {
                    handler.handle_notification(rpc);
                }
            }
        }
    }

    pub fn handle_response(&self, response: Result<CoreResponse, RpcError>) {}

    pub fn notification(&self, notification: CoreNotification) {
        let _ = self.tx.send(CoreRpc::Notification(notification));
    }

    pub fn proxy_connected(&self) {
        self.notification(CoreNotification::ProxyConnected {});
    }

    pub fn completion_response(&self, request_id: usize, resp: CompletionResponse) {
        self.notification(CoreNotification::CompletionResponse { request_id, resp });
    }

    pub fn close_terminal(&self, term_id: TermId) {
        self.notification(CoreNotification::CloseTerminal { term_id });
    }

    pub fn update_terminal(&self, term_id: TermId, content: String) {
        self.notification(CoreNotification::UpdateTerminal { term_id, content });
    }
}
