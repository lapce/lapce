use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender};
use lsp_types::{
    CancelParams, CompletionResponse, LogMessageParams, ProgressParams,
    PublishDiagnosticsParams, ShowMessageParams, SignatureHelp,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    RequestId, RpcError, RpcMessage,
    dap_types::{
        self, DapId, RunDebugConfig, Scope, StackFrame, Stopped, ThreadId, Variable,
    },
    file::PathObject,
    plugin::{PluginId, VoltInfo, VoltMetadata},
    proxy::ProxyStatus,
    source_control::DiffInfo,
    terminal::TermId,
};

pub enum CoreRpc {
    Request(RequestId, CoreRequest),
    Notification(Box<CoreNotification>), // Box it since clippy complains
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChanged {
    Change(String),
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreNotification {
    ProxyStatus {
        status: ProxyStatus,
    },
    OpenFileChanged {
        path: PathBuf,
        content: FileChanged,
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
    OpenPaths {
        paths: Vec<PathObject>,
    },
    WorkspaceFileChange,
    PublishDiagnostics {
        diagnostics: PublishDiagnosticsParams,
    },
    ServerStatus {
        params: ServerStatusParams,
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
        target: String,
    },
    LspCancel {
        params: CancelParams,
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
    DiffInfo {
        diff: DiffInfo,
    },
    UpdateTerminal {
        term_id: TermId,
        content: Vec<u8>,
    },
    TerminalLaunchFailed {
        term_id: TermId,
        error: String,
    },
    TerminalProcessId {
        term_id: TermId,
        process_id: Option<u32>,
    },
    TerminalProcessStopped {
        term_id: TermId,
        exit_code: Option<i32>,
    },
    RunInTerminal {
        config: RunDebugConfig,
    },
    Log {
        level: LogLevel,
        message: String,
        target: Option<String>,
    },
    DapStopped {
        dap_id: DapId,
        stopped: Stopped,
        stack_frames: HashMap<ThreadId, Vec<StackFrame>>,
        variables: Vec<(Scope, Vec<Variable>)>,
    },
    DapContinued {
        dap_id: DapId,
    },
    DapBreakpointsResp {
        dap_id: DapId,
        path: PathBuf,
        breakpoints: Vec<dap_types::Breakpoint>,
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
            if let Err(err) = tx.send(response) {
                tracing::error!("{:?}", err);
            }
        }
    }

    pub fn request(&self, request: CoreRequest) -> Result<CoreResponse, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, tx);
        }
        if let Err(err) = self.tx.send(CoreRpc::Request(id, request)) {
            tracing::error!("{:?}", err);
        }
        rx.recv().unwrap_or_else(|_| {
            Err(RpcError {
                code: 0,
                message: "io error".to_string(),
            })
        })
    }

    pub fn shutdown(&self) {
        if let Err(err) = self.tx.send(CoreRpc::Shutdown) {
            tracing::error!("{:?}", err);
        }
    }

    pub fn notification(&self, notification: CoreNotification) {
        if let Err(err) = self.tx.send(CoreRpc::Notification(Box::new(notification)))
        {
            tracing::error!("{:?}", err);
        }
    }

    pub fn workspace_file_change(&self) {
        self.notification(CoreNotification::WorkspaceFileChange);
    }

    pub fn diff_info(&self, diff: DiffInfo) {
        self.notification(CoreNotification::DiffInfo { diff });
    }

    pub fn open_file_changed(&self, path: PathBuf, content: FileChanged) {
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

    pub fn log(&self, level: LogLevel, message: String, target: Option<String>) {
        self.notification(CoreNotification::Log {
            level,
            message,
            target,
        });
    }

    pub fn publish_diagnostics(&self, diagnostics: PublishDiagnosticsParams) {
        self.notification(CoreNotification::PublishDiagnostics { diagnostics });
    }

    pub fn server_status(&self, params: ServerStatusParams) {
        self.notification(CoreNotification::ServerStatus { params });
    }

    pub fn work_done_progress(&self, progress: ProgressParams) {
        self.notification(CoreNotification::WorkDoneProgress { progress });
    }

    pub fn show_message(&self, title: String, message: ShowMessageParams) {
        self.notification(CoreNotification::ShowMessage { title, message });
    }

    pub fn log_message(&self, message: LogMessageParams, target: String) {
        self.notification(CoreNotification::LogMessage { message, target });
    }

    pub fn cancel(&self, params: CancelParams) {
        self.notification(CoreNotification::LspCancel { params });
    }

    pub fn terminal_process_id(&self, term_id: TermId, process_id: Option<u32>) {
        self.notification(CoreNotification::TerminalProcessId {
            term_id,
            process_id,
        });
    }

    pub fn terminal_process_stopped(&self, term_id: TermId, exit_code: Option<i32>) {
        self.notification(CoreNotification::TerminalProcessStopped {
            term_id,
            exit_code,
        });
    }

    pub fn terminal_launch_failed(&self, term_id: TermId, error: String) {
        self.notification(CoreNotification::TerminalLaunchFailed { term_id, error });
    }

    pub fn update_terminal(&self, term_id: TermId, content: Vec<u8>) {
        self.notification(CoreNotification::UpdateTerminal { term_id, content });
    }

    pub fn dap_stopped(
        &self,
        dap_id: DapId,
        stopped: Stopped,
        stack_frames: HashMap<ThreadId, Vec<StackFrame>>,
        variables: Vec<(Scope, Vec<Variable>)>,
    ) {
        self.notification(CoreNotification::DapStopped {
            dap_id,
            stopped,
            stack_frames,
            variables,
        });
    }

    pub fn dap_continued(&self, dap_id: DapId) {
        self.notification(CoreNotification::DapContinued { dap_id });
    }

    pub fn dap_breakpoints_resp(
        &self,
        dap_id: DapId,
        path: PathBuf,
        breakpoints: Vec<dap_types::Breakpoint>,
    ) {
        self.notification(CoreNotification::DapBreakpointsResp {
            dap_id,
            path,
            breakpoints,
        });
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Info = 0,
    Warn = 1,
    Error = 2,
    Debug = 3,
    Trace = 4,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerStatusParams {
    health: String,
    quiescent: bool,
    pub message: Option<String>,
}

impl ServerStatusParams {
    pub fn is_ok(&self) -> bool {
        self.health.as_str() == "ok"
    }
}
