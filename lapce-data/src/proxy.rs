use std::collections::HashMap;
use std::io::BufReader;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::{path::PathBuf, sync::Arc};

use anyhow::{anyhow, Result};
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use druid::Target;
use druid::{ExtEventSink, WidgetId};
use flate2::read::GzDecoder;
pub use lapce_proxy::VERSION;
use lapce_proxy::{dispatch::Dispatcher, APPLICATION_NAME};
use lapce_rpc::buffer::{BufferHeadResponse, BufferId, NewBufferResponse};
use lapce_rpc::core::{CoreNotification, CoreRequest, CoreResponse, CoreRpcMessage};
use lapce_rpc::plugin::PluginDescription;
use lapce_rpc::proxy::{
    CoreProxyNotification, CoreProxyRequest, NewHandler, ProxyResponse,
    ProxyRpcHandler, ProxyRpcMessage, ReadDirResponse,
};
use lapce_rpc::source_control::FileDiff;
use lapce_rpc::style::SemanticStyles;
use lapce_rpc::terminal::TermId;
use lapce_rpc::{stdio_transport, Callback, RpcError};
use lapce_rpc::{ControlFlow, Handler};
use lapce_rpc::{NewRpcHandler, RpcHandler};
use lsp_types::request::GotoTypeDefinitionResponse;
use lsp_types::{
    CodeActionResponse, CompletionItem, CompletionResponse, DocumentSymbolResponse,
    GotoDefinitionResponse, InlayHint, SymbolInformation, TextEdit,
};
use lsp_types::{Hover, Position};
use lsp_types::{Location, Url};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::json;
use serde_json::Value;
use thiserror::Error;
use xi_rope::{Rope, RopeDelta};

use crate::command::LapceUICommand;
use crate::command::LAPCE_UI_COMMAND;
use crate::config::Config;
use crate::data::{LapceWorkspace, LapceWorkspaceType};
use crate::terminal::RawTerminal;

pub enum TermEvent {
    NewTerminal(Arc<Mutex<RawTerminal>>),
    UpdateContent(String),
    CloseTerminal,
}

#[derive(Clone, Copy, Debug)]
pub enum ProxyStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Error, Debug)]
pub enum RequestError {
    /// Error in deserializing to the expected value
    #[error("failed to deserialize")]
    Deser(serde_json::Error),
    #[error("response failed")]
    Rpc(Value),
}

#[derive(Clone, Copy, Error, Debug, PartialEq, Eq, strum_macros::Display)]
#[strum(ascii_case_insensitive)]
enum HostPlatform {
    UnknownOS,
    #[strum(serialize = "windows")]
    Windows,
    #[strum(serialize = "linux")]
    Linux,
    #[strum(serialize = "darwin")]
    Darwin,
    #[strum(serialize = "bsd")]
    Bsd,
}

/// serialise via strum to arch name that is used
/// in CI artefacts
#[derive(Clone, Copy, Error, Debug, PartialEq, Eq, strum_macros::Display)]
#[strum(ascii_case_insensitive)]
enum HostArchitecture {
    UnknownArch,
    #[strum(serialize = "x86_64")]
    AMD64,
    #[strum(serialize = "x86")]
    X86,
    #[strum(serialize = "aarch64")]
    ARM64,
    #[strum(serialize = "armv7")]
    ARM32v7,
    #[strum(serialize = "armhf")]
    ARM32v6,
}

#[derive(Clone)]
pub struct LapceProxy {
    pub tab_id: WidgetId,
    rpc: RpcHandler,
    new_rpc: ProxyRpcHandler<CoreResponse>,
    proxy_receiver: Arc<Receiver<Value>>,
    new_proxy_sender: Arc<Sender<ProxyRpcMessage>>,
    new_proxy_receiver: Arc<Receiver<ProxyRpcMessage>>,
    term_tx: Sender<(TermId, TermEvent)>,
    event_sink: ExtEventSink,
}

impl NewHandler<CoreRequest, CoreNotification, CoreResponse> for LapceProxy {
    fn handle_notification(&mut self, rpc: CoreNotification) {
        todo!()
    }

    fn handle_request(&mut self, rpc: CoreRequest) {
        todo!()
    }
}

impl Handler for LapceProxy {
    type Notification = CoreNotification;
    type Request = CoreRequest;

    fn handle_notification(&mut self, rpc: Self::Notification) -> ControlFlow {
        use lapce_rpc::core::CoreNotification::*;
        match rpc {
            OpenFileChanged { path, content } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::OpenFileChanged {
                        path,
                        content: Rope::from(content),
                    },
                    Target::Widget(self.tab_id),
                );
            }
            ReloadBuffer { path, content, rev } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ReloadBuffer {
                        path,
                        rev,
                        content: Rope::from(content),
                    },
                    Target::Widget(self.tab_id),
                );
            }
            PublishDiagnostics { diagnostics } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PublishDiagnostics(diagnostics),
                    Target::Widget(self.tab_id),
                );
            }
            WorkDoneProgress { progress } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::WorkDoneProgress(progress),
                    Target::Widget(self.tab_id),
                );
            }
            InstalledPlugins { plugins } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateInstalledPlugins(plugins.clone()),
                    Target::Widget(self.tab_id),
                );
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdatePluginInstallationChange(plugins),
                    Target::Widget(self.tab_id),
                );
            }
            DisabledPlugins { plugins } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateDisabledPlugins(plugins),
                    Target::Auto,
                );
            }
            DiffInfo { diff } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateDiffInfo(diff),
                    Target::Widget(self.tab_id),
                );
            }
            UpdateTerminal { term_id, content } => {
                let _ = self
                    .term_tx
                    .send((term_id, TermEvent::UpdateContent(content)));
            }
            CloseTerminal { term_id } => {
                let _ = self.term_tx.send((term_id, TermEvent::CloseTerminal));
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CloseTerminal(term_id),
                    Target::Widget(self.tab_id),
                );
            }
            ProxyConnected {} => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ProxyUpdateStatus(ProxyStatus::Connected),
                    Target::Widget(self.tab_id),
                );
            }
            HomeDir { path } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::HomeDir(path),
                    Target::Widget(self.tab_id),
                );
            }
            ListDir { .. } | DiffFiles { .. } => {}
            WorkspaceFileChange {} => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::WorkspaceFileChange,
                    Target::Widget(self.tab_id),
                );
            }
        }
        ControlFlow::Continue
    }

    fn handle_request(&mut self, _rpc: Self::Request) -> Result<Value, Value> {
        Err(json!("unimplemented"))
    }
}

impl LapceProxy {
    pub fn new(
        tab_id: WidgetId,
        workspace: LapceWorkspace,
        term_tx: Sender<(TermId, TermEvent)>,
        event_sink: ExtEventSink,
    ) -> Self {
        let (proxy_sender, proxy_receiver) = crossbeam_channel::unbounded();
        let rpc = RpcHandler::new(proxy_sender);

        let (new_proxy_sender, new_proxy_receiver) = crossbeam_channel::unbounded();
        let new_rpc = ProxyRpcHandler::new(new_proxy_sender.clone());

        let proxy = Self {
            tab_id,
            rpc,
            new_rpc,
            proxy_receiver: Arc::new(proxy_receiver),
            new_proxy_receiver: Arc::new(new_proxy_receiver),
            new_proxy_sender: Arc::new(new_proxy_sender),
            term_tx,
            event_sink: event_sink.clone(),
        };

        let local_proxy = proxy.clone();
        thread::spawn(move || {
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::ProxyUpdateStatus(ProxyStatus::Connecting),
                Target::Widget(tab_id),
            );
            let _ = local_proxy.start(workspace.clone());
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::ProxyUpdateStatus(ProxyStatus::Disconnected),
                Target::Widget(tab_id),
            );
        });

        proxy
    }

    fn start(&self, workspace: LapceWorkspace) -> Result<()> {
        if let Some(path) = workspace.path.as_ref() {
            self.initialize(path.clone());
        }
        let (core_sender, core_receiver) = crossbeam_channel::unbounded();
        let (new_core_sender, new_core_receiver) = crossbeam_channel::unbounded();
        match workspace.kind {
            LapceWorkspaceType::Local => {
                let proxy_receiver = (*self.proxy_receiver).clone();
                thread::spawn(move || {
                    let dispatcher = Dispatcher::new(core_sender);
                    let _ = dispatcher.mainloop(proxy_receiver);
                });

                let new_proxy_sender = (*self.new_proxy_sender).clone();
                let new_proxy_receiver = (*self.new_proxy_receiver).clone();
                thread::spawn(move || {
                    let mut dispatcher =
                        NewDispatcher::new(new_core_sender, new_proxy_sender);
                    dispatcher.mainloop(new_proxy_receiver);
                });
            }
            LapceWorkspaceType::RemoteSSH(user, host) => {
                self.start_remote(SshRemote { user, host }, core_sender)?;
            }
            LapceWorkspaceType::RemoteWSL => {
                let distro = WslDistro::all()?
                    .into_iter()
                    .find(|distro| distro.default)
                    .ok_or_else(|| anyhow!("no default distro found"))?
                    .name;
                self.start_remote(WslRemote { distro }, core_sender)?;
            }
        }

        let mut proxy = self.clone();
        let mut handler = self.clone();
        proxy.new_rpc.mainloop(new_core_receiver, &mut handler);

        let mut proxy = self.clone();
        let mut handler = self.clone();
        proxy.rpc.mainloop(core_receiver, &mut handler);

        Ok(())
    }

    fn start_remote(
        &self,
        remote: impl Remote,
        core_sender: Sender<Value>,
    ) -> Result<()> {
        remote.connection_debug();

        use HostPlatform::*;
        let (platform, architecture) = self.host_specification(&remote).unwrap();

        if platform == UnknownOS || architecture == HostArchitecture::UnknownArch {
            log::error!(target: "lapce_data::proxy::start_remote", "detected remote host: {platform}/{architecture}");
            return Err(anyhow!("Unknown OS and/or architecture"));
        }

        let proxy_filename = "lapce-proxy";

        // ! Below paths have to be synced with what is
        // ! returned by Config::proxy_directory()
        let remote_proxy_path = match platform {
            Windows => format!("%HOMEDRIVE%%HOMEPATH%\\AppData\\Local\\lapce\\{APPLICATION_NAME}\\data\\proxy"),
            Darwin => format!("~/Library/Application Support/dev.lapce.{APPLICATION_NAME}/proxy"),
            _ => format!("~/.local/share/{APPLICATION_NAME}/proxy").to_lowercase(),
        };

        let remote_proxy_file = match platform {
            Windows => format!("{remote_proxy_path}\\{proxy_filename}.exe"),
            _ => format!("{remote_proxy_path}/{proxy_filename}"),
        };

        let proxy_filename =
            format!("{proxy_filename}-{}-{}", platform, architecture);

        log::debug!(target: "lapce_data::proxy::start_remote", "remote proxy path: {remote_proxy_path}");

        let cmd = match platform {
            Windows => remote
                .command_builder()
                .args(["dir", &remote_proxy_file])
                .status()?,
            _ => remote
                .command_builder()
                .arg("test")
                .arg("-e")
                .arg(&remote_proxy_file)
                .status()?,
        };
        if !cmd.success() {
            let local_proxy_file = Config::proxy_directory()
                .ok_or_else(|| anyhow!("can't find proxy directory"))?
                .join(&proxy_filename);
            if !local_proxy_file.exists() {
                let url = format!("https://github.com/lapce/lapce/releases/download/{}/{proxy_filename}.gz", if VERSION.eq("nightly") { VERSION.to_string() } else { format!("v{VERSION}") });
                let mut resp = reqwest::blocking::get(url).expect("request failed");
                if resp.status().is_success() {
                    let mut out = std::fs::File::create(&local_proxy_file)
                        .expect("failed to create file");
                    let mut gz = GzDecoder::new(&mut resp);
                    std::io::copy(&mut gz, &mut out)
                        .expect("failed to copy content");
                } else {
                    log::error!(target: "lapce_data::proxy::start_remote", "proxy download failed with: {}", resp.status());
                }
            }

            remote
                .command_builder()
                .arg("mkdir")
                .arg(remote_proxy_path)
                .status()?;

            remote.upload_file(&local_proxy_file, &remote_proxy_file)?;
        }

        if platform != Windows {
            remote
                .command_builder()
                .arg("chmod")
                .arg("+x")
                .arg(&remote_proxy_file)
                .status()?;
        }

        let mut child = remote
            .command_builder()
            .arg(&remote_proxy_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("can't find stdin"))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("can't find stdout"))?,
        );

        let proxy_receiver = (*self.proxy_receiver).clone();
        stdio_transport(stdin, proxy_receiver, stdout, core_sender);

        Ok(())
    }

    fn host_specification(
        &self,
        remote: &impl Remote,
    ) -> Result<(HostPlatform, HostArchitecture)> {
        use HostArchitecture::*;
        use HostPlatform::*;

        let cmd = remote.command_builder().arg("uname -sm").output();

        let spec = match cmd {
            Ok(cmd) => {
                let stdout = String::from_utf8_lossy(&cmd.stdout).to_lowercase();
                let stdout = stdout.trim();
                log::debug!(target: "lapce_data::proxy::host_platform", "{}", &stdout);
                match stdout {
                    "" => {
                        let cmd = remote
                            .command_builder()
                            .args(["echo", "%OS%", "%PROCESSOR_ARCHITECTURE%"])
                            .output();
                        match cmd {
                            Ok(cmd) => {
                                let stdout = String::from_utf8_lossy(&cmd.stdout)
                                    .to_lowercase();
                                let stdout = stdout.trim();
                                log::debug!(target: "lapce_data::proxy::host_platform", "{}", &stdout);
                                match stdout.split_once(' ') {
                                    Some((os, arch)) => {
                                        (parse_os(os), parse_arch(arch))
                                    }
                                    None => (UnknownOS, UnknownArch),
                                }
                            }
                            Err(e) => {
                                log::error!(target: "lapce_data::proxy::host_platform", "{e}");
                                (UnknownOS, UnknownArch)
                            }
                        }
                    }
                    v => {
                        if let Some((os, arch)) = v.split_once(' ') {
                            (parse_os(os), parse_arch(arch))
                        } else {
                            (UnknownOS, UnknownArch)
                        }
                    }
                }
            }
            Err(e) => {
                log::error!(target: "lapce_data::proxy::host_platform", "{e}");
                (UnknownOS, UnknownArch)
            }
        };
        Ok(spec)
    }

    pub fn initialize(&self, workspace: PathBuf) {
        self.new_rpc
            .send_core_notification(CoreProxyNotification::Initialize { workspace });
    }

    pub fn terminal_close(&self, term_id: TermId) {
        self.rpc.send_rpc_notification(
            "terminal_close",
            &json!({
                "term_id": term_id,
            }),
        )
    }

    pub fn terminal_resize(&self, term_id: TermId, width: usize, height: usize) {
        self.rpc.send_rpc_notification(
            "terminal_resize",
            &json!({
                "term_id": term_id,
                "width": width,
                "height": height,
            }),
        )
    }

    pub fn terminal_write(&self, term_id: TermId, content: &str) {
        self.rpc.send_rpc_notification(
            "terminal_write",
            &json!({
                "term_id": term_id,
                "content": content,
            }),
        )
    }

    pub fn new_terminal(
        &self,
        term_id: TermId,
        cwd: Option<PathBuf>,
        shell: String,
        raw: Arc<Mutex<RawTerminal>>,
    ) {
        let _ = self.term_tx.send((term_id, TermEvent::NewTerminal(raw)));
        self.rpc.send_rpc_notification(
            "new_terminal",
            &json!({
                "term_id": term_id,
                "cwd": cwd,
                "shell": shell,
            }),
        )
    }

    pub fn git_init(&self) {
        self.rpc.send_rpc_notification("git_init", &json!({}));
    }

    pub fn git_commit(&self, message: &str, diffs: Vec<FileDiff>) {
        self.rpc.send_rpc_notification(
            "git_commit",
            &json!({
                "message": message,
                "diffs": diffs,
            }),
        )
    }

    pub fn git_checkout(&self, branch: &str) {
        self.rpc.send_rpc_notification(
            "git_checkout",
            &json!({
                "branch": branch,
            }),
        )
    }

    pub fn git_discard_file_changes(&self, file: &Path) {
        self.rpc.send_rpc_notification(
            "git_discard_file_changes",
            &json!({
                "file": file,
            }),
        );
    }

    pub fn git_discard_files_changes(&self, files: Vec<PathBuf>) {
        self.rpc.send_rpc_notification(
            "git_discard_files_changes",
            &json!({
                "files": files,
            }),
        );
    }

    pub fn git_discard_workspace_changes(&self) {
        self.rpc
            .send_rpc_notification("git_discard_workspace_changes", &json!({}));
    }

    pub fn install_plugin(&self, plugin: &PluginDescription) {
        self.rpc
            .send_rpc_notification("install_plugin", &json!({ "plugin": plugin }));
    }

    pub fn disable_plugin(&self, plugin: &PluginDescription) {
        self.rpc
            .send_rpc_notification("disable_plugin", &json!({ "plugin": plugin }))
    }

    pub fn enable_plugin(&self, plugin: &PluginDescription) {
        self.rpc
            .send_rpc_notification("enable_plugin", &json!({ "plugin": plugin }))
    }

    pub fn remove_plugin(&self, plugin: &PluginDescription) {
        self.rpc
            .send_rpc_notification("remove_plugin", &json!({ "plugin": plugin }));
    }

    pub fn get_buffer_head(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: impl FnOnce(Result<CoreResponse, RpcError>) + Send + 'static,
    ) {
        self.new_rpc.send_core_request_async(
            CoreProxyRequest::BufferHead { buffer_id, path },
            Box::new(f),
        );
    }

    // TODO: Make this type more explicit
    pub fn global_search(
        &self,
        pattern: String,
        f: impl FnOnce(
                Result<
                    HashMap<PathBuf, Vec<(usize, (usize, usize), String)>>,
                    RequestError,
                >,
            ) + Send
            + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "global_search",
            &json!({ "pattern": pattern }),
            box_json_cb(f),
        );
    }

    pub fn new_buffer(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: impl FnOnce(Result<CoreResponse, RpcError>) + Send + 'static,
    ) {
        self.new_rpc.send_core_request_async(
            CoreProxyRequest::NewBuffer { buffer_id, path },
            Box::new(f),
        );
    }

    pub fn save_buffer_as(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        rev: u64,
        content: String,
        f: Box<dyn Callback>,
    ) {
        let request = CoreProxyRequest::SaveBufferAs {
            buffer_id,
            path,
            rev,
            content,
        };
        self.rpc.send_rpc_request_value_async(request, f);
    }

    pub fn update(&self, buffer_id: BufferId, delta: &RopeDelta, rev: u64) {
        self.rpc.send_rpc_notification(
            "update",
            &json!({
                "buffer_id": buffer_id,
                "delta": delta,
                "rev": rev,
            }),
        )
    }

    pub fn save(&self, rev: u64, buffer_id: BufferId, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "save",
            &json!({
                "rev": rev,
                "buffer_id": buffer_id,
            }),
            f,
        );
    }

    pub fn create_file(&self, path: &Path, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "create_file",
            &json!({
                "path": path,
            }),
            f,
        );
    }

    pub fn create_directory(&self, path: &Path, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "create_directory",
            &json!({
                "path": path,
            }),
            f,
        );
    }

    pub fn trash_path(&self, path: &Path, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "trash_path",
            &json!({
                "path": path,
            }),
            f,
        );
    }

    pub fn rename_path(
        &self,
        from_path: &Path,
        to_path: &Path,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "rename_path",
            &json!({
                "from": from_path,
                "to": to_path,
            }),
            f,
        );
    }

    pub fn get_completion(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: impl FnOnce(Result<CompletionResponse, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_completion",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            box_json_cb(f),
        );
    }

    pub fn completion_resolve(
        &self,
        buffer_id: BufferId,
        completion_item: CompletionItem,
        f: impl FnOnce(Result<CompletionItem, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "completion_resolve",
            &json!({
                "buffer_id": buffer_id,
                "completion_item": completion_item,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_hover(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: impl FnOnce(Result<Hover, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_hover",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_signature(
        &self,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_signature",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_references(
        &self,
        buffer_id: BufferId,
        position: Position,
        f: impl FnOnce(Result<Vec<Location>, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_references",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_files(
        &self,
        f: impl FnOnce(Result<CoreResponse, RpcError>) + Send + 'static,
    ) {
        self.new_rpc.send_core_request_async(
            CoreProxyRequest::GetFiles {
                path: "path".into(),
            },
            Box::new(f),
        );
    }

    pub fn read_dir(
        &self,
        path: &Path,
        f: impl FnOnce(Result<CoreResponse, RpcError>) + Send + 'static,
    ) {
        self.new_rpc.send_core_request_async(
            CoreProxyRequest::ReadDir { path: path.into() },
            Box::new(f),
        );
    }

    pub fn get_definition(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: impl FnOnce(Result<GotoDefinitionResponse, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_definition",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_type_definition(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: impl FnOnce(Result<GotoTypeDefinitionResponse, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_type_definition",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_document_symbols(
        &self,
        buffer_id: BufferId,
        f: impl FnOnce(Result<DocumentSymbolResponse, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_document_symbols",
            &json!({
                "buffer_id": buffer_id,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_workspace_symbols(
        &self,
        buffer_id: BufferId,
        query: &str,
        f: impl FnOnce(Result<Option<Vec<SymbolInformation>>, RequestError>)
            + Send
            + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_workspace_symbols",
            &json!({
                "buffer_id": buffer_id,
                "query": query,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_inlay_hints(
        &self,
        buffer_id: BufferId,
        f: impl FnOnce(Result<Vec<InlayHint>, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_inlay_hints",
            &json!({
                "buffer_id": buffer_id,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_semantic_tokens(
        &self,
        buffer_id: BufferId,
        f: impl FnOnce(Result<SemanticStyles, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_semantic_tokens",
            &json!({
                "buffer_id": buffer_id,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_code_actions(
        &self,
        buffer_id: BufferId,
        position: Position,
        f: impl FnOnce(Result<CodeActionResponse, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_code_actions",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            box_json_cb(f),
        );
    }

    pub fn get_document_formatting(
        &self,
        buffer_id: BufferId,
        f: impl FnOnce(Result<Vec<TextEdit>, RequestError>) + Send + 'static,
    ) {
        self.rpc.send_rpc_request_async(
            "get_document_formatting",
            &json!({
                "buffer_id": buffer_id,
            }),
            box_json_cb(f),
        );
    }

    pub fn stop(&self) {
        self.rpc.send_rpc_notification("shutdown", &json!({}));
        // self.core_sender.send(json!({
        //     "method": "shutdown",
        //     "params": {},
        // }));
    }
}

fn new_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd
}

trait Remote: Sized {
    fn home_dir(&self) -> Result<String> {
        let cmd = self
            .command_builder()
            .arg("echo")
            .arg("-n")
            .arg("$HOME")
            .stdout(Stdio::piped())
            .output()?;

        Ok(String::from_utf8(cmd.stdout)?)
    }

    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()>;

    fn command_builder(&self) -> Command;

    fn connection_debug(&self);
}

struct SshRemote {
    user: String,
    host: String,
}

impl SshRemote {
    #[cfg(target_os = "windows")]
    const SSH_ARGS: &'static [&'static str] = &["-o", "ConnectTimeout=15"];

    #[cfg(not(target_os = "windows"))]
    const SSH_ARGS: &'static [&'static str] = &[
        "-o",
        "ControlMaster=auto",
        "-o",
        "ControlPath=~/.ssh/cm-%r@%h:%p",
        "-o",
        "ControlPersist=30m",
        "-o",
        "ConnectTimeout=15",
    ];

    fn command_builder(user: &str, host: &str) -> Command {
        let mut cmd = new_command("ssh");
        cmd.arg(format!("{}@{}", user, host)).args(Self::SSH_ARGS);

        #[cfg(debug_assertions)]
        cmd.arg("-v");

        cmd
    }
}

impl Remote for SshRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        new_command("scp")
            .args(Self::SSH_ARGS)
            .arg(local.as_ref())
            .arg(dbg!(format!("{}@{}:{remote}", self.user, self.host)))
            .status()?;
        Ok(())
    }

    fn command_builder(&self) -> Command {
        Self::command_builder(&self.user, &self.host)
    }

    fn connection_debug(&self) {
        if let Ok(out) = self.command_builder().arg("-v").arg("exit").output() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            for line in stderr.split_terminator(['\n', '\r']) {
                if line.is_empty() {
                    continue;
                }
                log::debug!(target: "lapce_data::proxy::connection_debug", "{line}");
            }
        } else {
            log::debug!(target: "lapce_data::proxy::connection_debug", "ssh debug output failed");
        }
    }
}

#[derive(Debug)]
struct WslDistro {
    pub name: String,
    pub default: bool,
}

impl WslDistro {
    fn all() -> Result<Vec<WslDistro>> {
        let cmd = new_command("wsl")
            .arg("-l")
            .arg("-v")
            .stdout(Stdio::piped())
            .output()?;

        if !cmd.status.success() {
            return Err(anyhow!("failed to execute `wsl -l -v`"));
        }

        let distros = String::from_utf16(bytemuck::cast_slice(&cmd.stdout))?
            .lines()
            .skip(1)
            .filter_map(|line| {
                let line = line.trim_start();
                let default = line.starts_with('*');
                let name = line
                    .trim_start_matches('*')
                    .trim_start()
                    .split(' ')
                    .next()?;
                Some(WslDistro {
                    name: name.to_string(),
                    default,
                })
            })
            .collect();

        Ok(distros)
    }
}

struct WslRemote {
    distro: String,
}

impl Remote for WslRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut wsl_path = Path::new(r"\\wsl.localhost\").join(&self.distro);
        if !wsl_path.exists() {
            wsl_path = Path::new(r#"\\wsl$"#).join(&self.distro);
        }
        wsl_path = if remote.starts_with('~') {
            let home_dir = self.home_dir()?;
            wsl_path.join(remote.replacen('~', &home_dir, 1))
        } else {
            wsl_path.join(remote)
        };
        std::fs::copy(local, wsl_path)?;
        Ok(())
    }

    fn command_builder(&self) -> Command {
        let mut cmd = new_command("wsl");
        cmd.arg("-d").arg(&self.distro).arg("--");
        cmd
    }

    fn connection_debug(&self) {}
}

// Rust-analyzer returns paths in the form of "file:///<drive>:/...", which gets parsed into URL
// as "/<drive>://" which is then interpreted by PathBuf::new() as a UNIX-like path from root.
// This function strips the additional / from the beginning, if the first segment is a drive letter.
#[cfg(windows)]
pub fn path_from_url(url: &Url) -> PathBuf {
    let path = url.path();
    if let Some(path) = path.strip_prefix('/') {
        if let Some((maybe_drive_letter, _)) = path.split_once(&['/', '\\']) {
            let b = maybe_drive_letter.as_bytes();
            if b.len() == 2
                && matches!(
                    b[0],
                    b'a'..=b'z' | b'A'..=b'Z'
                )
                && b[1] == b':'
            {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from(path)
}

#[cfg(not(windows))]
pub fn path_from_url(url: &Url) -> PathBuf {
    PathBuf::from(url.path())
}

/// Create a callback that receives an unparsed json result
/// Then parse it to the given type and pass it to the callback
/// Most callbacks currently don't need to do anything beyond parsing it to a specific type
fn box_json_cb<
    T: DeserializeOwned,
    F: FnOnce(Result<T, RequestError>) + Send + 'static,
>(
    f: F,
) -> Box<dyn Callback> {
    Box::new(|res: Result<Value, Value>| {
        f(res.map_err(RequestError::Rpc).and_then(|x| {
            serde_json::from_value::<T>(x).map_err(RequestError::Deser)
        }))
    })
}

fn parse_arch(arch: &str) -> HostArchitecture {
    use HostArchitecture::*;
    // processor architectures be like that
    match arch {
        "amd64" | "x64" | "x86_64" => AMD64,
        "x86" | "i386" | "i586" | "i686" => X86,
        "arm" | "armhf" | "armv6" => ARM32v6,
        "armv7" | "armv7l" => ARM32v7,
        "arm64" | "armv8" | "aarch64" => ARM64,
        _ => UnknownArch,
    }
}

fn parse_os(os: &str) -> HostPlatform {
    use HostPlatform::*;
    match os {
        "linux" => Linux,
        "darwin" => Darwin,
        "windows_nt" => Windows,
        v => {
            if v.ends_with("bsd") {
                Bsd
            } else {
                UnknownOS
            }
        }
    }
}
