use crossbeam_channel::{Receiver, Sender};
use lsp_types::{CompletionResponse, ProgressParams, PublishDiagnosticsParams};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crate::{
    file::FileNodeItem,
    plugin::{PluginId, VoltInfo, VoltMetadata},
    source_control::DiffInfo,
    terminal::TermId,
    RequestId, RpcError,
};

pub enum CoreRpc {
    Request(RequestId, CoreRequest),
    Notification(CoreNotification),
    Shutdown,
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
        input: String,
        resp: CompletionResponse,
        plugin_id: PluginId,
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
    VoltInstalled {
        volt: VoltMetadata,
    },
    VoltRemoved {
        volt: VoltInfo,
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
    fn handle_request(&mut self, id: RequestId, rpc: CoreRequest);
}

#[derive(Clone)]
pub struct CoreRpcHandler {
    tx: Sender<CoreRpc>,
    rx: Receiver<CoreRpc>,
    id: Arc<AtomicU64>,
    #[allow(clippy::type_complexity)]
    pending: Arc<Mutex<HashMap<u64, Sender<Result<CoreResponse, RpcError>>>>>,
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
                    handler.handle_request(id, rpc);
                }
                CoreRpc::Notification(rpc) => {
                    handler.handle_notification(rpc);
                }
                CoreRpc::Shutdown => {
                    return;
                }
            }
        }
    }

    pub fn rx(&self) -> &Receiver<CoreRpc> {
        &self.rx
    }

    pub fn handle_response(
        &self,
        id: RequestId,
        response: Result<CoreResponse, RpcError>,
    ) {
        let tx = { self.pending.lock().remove(&id) };
        if let Some(tx) = tx {
            let _ = tx.send(response);
        }
    }

    pub fn request(&self, request: CoreRequest) -> Result<CoreResponse, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, tx);
        }
        let _ = self.tx.send(CoreRpc::Request(id, request));
        rx.recv().unwrap_or_else(|_| {
            Err(RpcError {
                code: 0,
                message: "io error".to_string(),
            })
        })
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(CoreRpc::Shutdown);
    }

    pub fn notification(&self, notification: CoreNotification) {
        let _ = self.tx.send(CoreRpc::Notification(notification));
    }

    pub fn proxy_connected(&self) {
        self.notification(CoreNotification::ProxyConnected {});
    }

    pub fn workspace_file_change(&self) {
        self.notification(CoreNotification::WorkspaceFileChange {});
    }

    pub fn diff_info(&self, diff: DiffInfo) {
        self.notification(CoreNotification::DiffInfo { diff });
    }

    pub fn open_file_changed(&self, path: PathBuf, content: String) {
        self.notification(CoreNotification::OpenFileChanged { path, content });
    }

    pub fn completion_response(
        &self,
        request_id: usize,
        input: String,
        resp: CompletionResponse,
        plugin_id: PluginId,
    ) {
        self.notification(CoreNotification::CompletionResponse {
            request_id,
            input,
            resp,
            plugin_id,
        });
    }

    pub fn volt_installed(&self, volt: VoltMetadata) {
        self.notification(CoreNotification::VoltInstalled { volt });
    }

    pub fn volt_removed(&self, volt: VoltInfo) {
        self.notification(CoreNotification::VoltRemoved { volt });
    }

    pub fn publish_diagnostics(&self, diagnostics: PublishDiagnosticsParams) {
        self.notification(CoreNotification::PublishDiagnostics { diagnostics });
    }

    pub fn work_done_progress(&self, progress: ProgressParams) {
        self.notification(CoreNotification::WorkDoneProgress { progress });
    }

    pub fn close_terminal(&self, term_id: TermId) {
        self.notification(CoreNotification::CloseTerminal { term_id });
    }

    pub fn update_terminal(&self, term_id: TermId, content: String) {
        self.notification(CoreNotification::UpdateTerminal { term_id, content });
    }
}

impl Default for CoreRpcHandler {
    fn default() -> Self {
        Self::new()
    }
}
