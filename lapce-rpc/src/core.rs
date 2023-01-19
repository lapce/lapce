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
    CompletionResponse, LogMessageParams, ProgressParams, PublishDiagnosticsParams,
    ShowMessageParams, SignatureHelp,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    dap_types::{DapId, RunDebugConfig, ThreadId},
    file::FileNodeItem,
    plugin::{PluginId, VoltInfo, VoltMetadata},
    source_control::DiffInfo,
    terminal::TermId,
    RequestId, RpcError, RpcMessage,
};

pub enum CoreRpc {
    Request(RequestId, CoreRequest),
    Notification(Box<CoreNotification>), // Box it since clippy complains
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
    SignatureHelpResponse {
        request_id: usize,
        resp: SignatureHelp,
        plugin_id: PluginId,
    },
    ReloadBuffer {
        path: PathBuf,
        content: String,
        rev: u64,
    },
    OpenPaths {
        window_tab_id: Option<(usize, usize)>,
        folders: Vec<PathBuf>,
        files: Vec<PathBuf>,
    },
    WorkspaceFileChange {},
    PublishDiagnostics {
        diagnostics: PublishDiagnosticsParams,
    },
    WorkDoneProgress {
        progress: ProgressParams,
    },
    ShowMessage {
        title: String,
        message: ShowMessageParams,
    },
    LogMessage {
        message: LogMessageParams,
    },
    HomeDir {
        path: PathBuf,
    },
    VoltInstalled {
        volt: VoltMetadata,
        icon: Option<Vec<u8>>,
    },
    VoltInstalling {
        volt: VoltInfo,
        error: String,
    },
    VoltRemoving {
        volt: VoltMetadata,
        error: String,
    },
    VoltRemoved {
        volt: VoltInfo,
        only_installing: bool,
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
        content: Vec<u8>,
    },
    TerminalProcessId {
        term_id: TermId,
        process_id: u32,
    },
    TerminalProcessStopped {
        term_id: TermId,
    },
    RunInTerminal {
        config: RunDebugConfig,
    },
    Log {
        level: String,
        message: String,
    },
    DapStopped {
        dap_id: DapId,
        thread_id: ThreadId,
    },
    DapTerminated {
        dap_id: DapId,
    },
    DapContinued {
        dap_id: DapId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreResponse {}

pub type CoreMessage = RpcMessage<CoreRequest, CoreNotification, CoreResponse>;

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
                    handler.handle_notification(*rpc);
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
        let _ = self.tx.send(CoreRpc::Notification(Box::new(notification)));
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

    pub fn signature_help_response(
        &self,
        request_id: usize,
        resp: SignatureHelp,
        plugin_id: PluginId,
    ) {
        self.notification(CoreNotification::SignatureHelpResponse {
            request_id,
            resp,
            plugin_id,
        });
    }

    pub fn volt_installed(&self, volt: VoltMetadata, icon: Option<Vec<u8>>) {
        self.notification(CoreNotification::VoltInstalled { volt, icon });
    }

    pub fn volt_installing(&self, volt: VoltInfo, error: String) {
        self.notification(CoreNotification::VoltInstalling { volt, error });
    }

    pub fn volt_removing(&self, volt: VoltMetadata, error: String) {
        self.notification(CoreNotification::VoltRemoving { volt, error });
    }

    pub fn volt_removed(&self, volt: VoltInfo, only_installing: bool) {
        self.notification(CoreNotification::VoltRemoved {
            volt,
            only_installing,
        });
    }

    pub fn run_in_terminal(&self, config: RunDebugConfig) {
        self.notification(CoreNotification::RunInTerminal { config });
    }

    pub fn log(&self, level: log::Level, message: String) {
        self.notification(CoreNotification::Log {
            level: level.as_str().to_string(),
            message,
        });
    }

    pub fn publish_diagnostics(&self, diagnostics: PublishDiagnosticsParams) {
        self.notification(CoreNotification::PublishDiagnostics { diagnostics });
    }

    pub fn work_done_progress(&self, progress: ProgressParams) {
        self.notification(CoreNotification::WorkDoneProgress { progress });
    }

    pub fn show_message(&self, title: String, message: ShowMessageParams) {
        self.notification(CoreNotification::ShowMessage { title, message });
    }

    pub fn log_message(&self, message: LogMessageParams) {
        self.notification(CoreNotification::LogMessage { message });
    }

    pub fn terminal_process_id(&self, term_id: TermId, process_id: u32) {
        self.notification(CoreNotification::TerminalProcessId {
            term_id,
            process_id,
        });
    }

    pub fn terminal_process_stopped(&self, term_id: TermId) {
        self.notification(CoreNotification::TerminalProcessStopped { term_id });
    }

    pub fn update_terminal(&self, term_id: TermId, content: Vec<u8>) {
        self.notification(CoreNotification::UpdateTerminal { term_id, content });
    }

    pub fn dap_stopped(&self, dap_id: DapId, thread_id: ThreadId) {
        self.notification(CoreNotification::DapStopped { dap_id, thread_id });
    }

    pub fn dap_continued(&self, dap_id: DapId) {
        self.notification(CoreNotification::DapContinued { dap_id });
    }

    pub fn dap_terminated(&self, dap_id: DapId) {
        self.notification(CoreNotification::DapTerminated { dap_id });
    }

    pub fn home_dir(&self, path: PathBuf) {
        self.notification(CoreNotification::HomeDir { path });
    }
}

impl Default for CoreRpcHandler {
    fn default() -> Self {
        Self::new()
    }
}
