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
use lapce_core::style::LineStyle;
use lapce_proxy::dispatch::FileDiff;
use lapce_proxy::dispatch::FileNodeItem;
use lapce_proxy::dispatch::{DiffInfo, Dispatcher};
use lapce_proxy::plugin::PluginDescription;
use lapce_proxy::terminal::TermId;
use lapce_rpc::RpcHandler;
use lapce_rpc::{stdio_transport, Callback};
use lapce_rpc::{ControlFlow, Handler};
use lsp_types::CompletionItem;
use lsp_types::Position;
use lsp_types::ProgressParams;
use lsp_types::PublishDiagnosticsParams;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use xi_rope::spans::SpansBuilder;
use xi_rope::{Interval, RopeDelta};

use crate::command::LapceUICommand;
use crate::config::Config;
use crate::state::LapceWorkspace;
use crate::state::LapceWorkspaceType;
use crate::terminal::RawTerminal;
use crate::{buffer::BufferId, command::LAPCE_UI_COMMAND};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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

#[derive(Clone)]
pub struct LapceProxy {
    pub tab_id: WidgetId,
    rpc: RpcHandler,
    proxy_receiver: Arc<Receiver<Value>>,
    term_tx: Sender<(TermId, TermEvent)>,
    event_sink: ExtEventSink,
}

impl Handler for LapceProxy {
    type Notification = Notification;
    type Request = Request;

    fn handle_notification(&mut self, rpc: Self::Notification) -> ControlFlow {
        match rpc {
            Notification::SemanticStyles {
                rev,
                buffer_id,
                path,
                styles,
                len,
            } => {
                let event_sink = self.event_sink.clone();
                let tab_id = self.tab_id;
                rayon::spawn(move || {
                    let mut styles_span = SpansBuilder::new(len);
                    for style in styles {
                        styles_span.add_span(
                            Interval::new(style.start, style.end),
                            style.style,
                        );
                    }
                    let styles_span = Arc::new(styles_span.build());
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSemanticStyles(
                            buffer_id,
                            path,
                            rev,
                            styles_span,
                        ),
                        Target::Widget(tab_id),
                    );
                });
            }
            Notification::ReloadBuffer {
                buffer_id,
                new_content,
                rev,
            } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ReloadBuffer(buffer_id, rev, new_content),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::PublishDiagnostics { diagnostics } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PublishDiagnostics(diagnostics),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::WorkDoneProgress { progress } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::WorkDoneProgress(progress),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::InstalledPlugins { plugins } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateInstalledPlugins(plugins),
                    Target::Widget(self.tab_id),
                );
            }
            #[allow(unused_variables)]
            Notification::ListDir { items } => {}
            #[allow(unused_variables)]
            Notification::DiffFiles { files } => {}
            Notification::DiffInfo { diff } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateDiffInfo(diff),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::UpdateTerminal { term_id, content } => {
                let _ = self
                    .term_tx
                    .send((term_id, TermEvent::UpdateContent(content)));
            }
            Notification::CloseTerminal { term_id } => {
                let _ = self.term_tx.send((term_id, TermEvent::CloseTerminal));
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CloseTerminal(term_id),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::ProxyConnected {} => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ProxyUpdateStatus(ProxyStatus::Connected),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::HomeDir { path } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::HomeDir(path),
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
        let proxy = Self {
            tab_id,
            rpc,
            proxy_receiver: Arc::new(proxy_receiver),
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
        match workspace.kind {
            LapceWorkspaceType::Local => {
                let proxy_reciever = (*self.proxy_receiver).clone();
                thread::spawn(move || {
                    let dispatcher = Dispatcher::new(core_sender);
                    let _ = dispatcher.mainloop(proxy_reciever);
                });

                let mut proxy = self.clone();
                let mut handler = self.clone();
                proxy.rpc.mainloop(core_receiver, &mut handler);
            }
            LapceWorkspaceType::RemoteSSH(user, host) => {
                let ssh_args = &[
                    "-o",
                    "ControlMaster=auto",
                    "-o",
                    "ControlPath=~/.ssh/cm-%r@%h:%p",
                    "-o",
                    "ControlPersist=30m",
                    "-o",
                    "ConnectTimeout=15",
                ];
                let mut cmd = Command::new("ssh");
                #[cfg(target_os = "windows")]
                let cmd = cmd.creation_flags(0x08000000);
                let cmd = cmd
                    .arg(format!("{}@{}", user, host))
                    .args(ssh_args)
                    .arg("test")
                    .arg("-e")
                    .arg(format!("~/.lapce/lapce-proxy-{}", VERSION))
                    .output()?;
                if !cmd.status.success() {
                    let local_proxy_file = Config::dir()
                        .ok_or_else(|| anyhow!("can't find config dir"))?
                        .join(format!("lapce-proxy-{}", VERSION));
                    if !local_proxy_file.exists() {
                        let url = format!("https://github.com/lapce/lapce/releases/download/v{VERSION}/lapce-proxy-linux.gz");
                        let mut resp =
                            reqwest::blocking::get(url).expect("request failed");
                        let mut out = std::fs::File::create(&local_proxy_file)
                            .expect("failed to create file");
                        let mut gz = GzDecoder::new(&mut resp);
                        std::io::copy(&mut gz, &mut out)
                            .expect("failed to copy content");
                    }

                    let mut cmd = Command::new("ssh");
                    #[cfg(target_os = "windows")]
                    let cmd = cmd.creation_flags(0x08000000);
                    cmd.arg(format!("{}@{}", user, host))
                        .args(ssh_args)
                        .arg("mkdir")
                        .arg("~/.lapce/")
                        .output()?;

                    let mut cmd = Command::new("scp");
                    #[cfg(target_os = "windows")]
                    let cmd = cmd.creation_flags(0x08000000);
                    cmd.args(ssh_args)
                        .arg(&local_proxy_file)
                        .arg(format!("{user}@{host}:~/.lapce/lapce-proxy-{VERSION}"))
                        .output()?;

                    let mut cmd = Command::new("ssh");
                    #[cfg(target_os = "windows")]
                    let cmd = cmd.creation_flags(0x08000000);
                    cmd.arg(format!("{}@{}", user, host))
                        .args(ssh_args)
                        .arg("chmod")
                        .arg("+x")
                        .arg(format!("~/.lapce/lapce-proxy-{}", VERSION))
                        .output()?;
                }

                let mut child = Command::new("ssh");
                #[cfg(target_os = "windows")]
                let child = child.creation_flags(0x08000000);
                let mut child = child
                    .arg(format!("{}@{}", user, host))
                    .args(ssh_args)
                    .arg(format!("~/.lapce/lapce-proxy-{}", VERSION))
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

                let proxy_reciever = (*self.proxy_receiver).clone();
                stdio_transport(stdin, proxy_reciever, stdout, core_sender);

                let mut handler = self.clone();
                let mut proxy = self.clone();
                proxy.rpc.mainloop(core_receiver, &mut handler);
            }
        }
        Ok(())
    }

    pub fn initialize(&self, workspace: PathBuf) {
        self.rpc.send_rpc_notification(
            "initialize",
            &json!({
                "workspace": workspace,
            }),
        )
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

    pub fn git_commit(&self, message: &str, diffs: Vec<FileDiff>) {
        self.rpc.send_rpc_notification(
            "git_commit",
            &json!({
                "message": message,
                "diffs": diffs,
            }),
        )
    }

    pub fn install_plugin(&self, plugin: &PluginDescription) {
        self.rpc
            .send_rpc_notification("install_plugin", &json!({ "plugin": plugin }));
    }

    pub fn get_buffer_head(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "buffer_head",
            &json!({ "buffer_id": buffer_id, "path": path, }),
            f,
        );
    }

    pub fn global_search(&self, pattern: String, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "global_search",
            &json!({ "pattern": pattern }),
            f,
        );
    }

    pub fn new_buffer(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "new_buffer",
            &json!({ "buffer_id": buffer_id, "path": path }),
            f,
        );
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

    pub fn get_completion(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_completion",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn completion_resolve(
        &self,
        buffer_id: BufferId,
        completion_item: CompletionItem,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "completion_resolve",
            &json!({
                "buffer_id": buffer_id,
                "completion_item": completion_item,
            }),
            f,
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
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_references",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_files(&self, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "get_files",
            &json!({
                "path": "path",
            }),
            f,
        );
    }

    pub fn read_dir(&self, path: &Path, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "read_dir",
            &json!({
                "path": path,
            }),
            f,
        );
    }

    pub fn get_definition(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_definition",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_document_symbols(&self, buffer_id: BufferId, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "get_document_symbols",
            &json!({
                "buffer_id": buffer_id,
            }),
            f,
        );
    }

    pub fn get_code_actions(
        &self,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_code_actions",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_document_formatting(
        &self,
        buffer_id: BufferId,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_document_formatting",
            &json!({
                "buffer_id": buffer_id,
            }),
            f,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CursorShape {
    /// Cursor is a block like `▒`.
    Block,

    /// Cursor is an underscore like `_`.
    Underline,

    /// Cursor is a vertical bar `⎸`.
    Beam,

    /// Cursor is a box like `☐`.
    HollowBlock,

    /// Invisible cursor.
    Hidden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    ProxyConnected {},
    SemanticStyles {
        rev: u64,
        buffer_id: BufferId,
        path: PathBuf,
        len: usize,
        styles: Vec<LineStyle>,
    },
    ReloadBuffer {
        buffer_id: BufferId,
        new_content: String,
        rev: u64,
    },
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
pub enum Request {}

pub struct ProxyHandlerNew {
    #[allow(dead_code)]
    tab_id: WidgetId,

    #[allow(dead_code)]
    term_tx: Sender<(TermId, TermEvent)>,

    #[allow(dead_code)]
    event_sink: ExtEventSink,
}
//
// impl Handler for ProxyHandlerNew {
//     type Notification = Notification;
//     type Request = Request;
//
//     fn handle_notification(
//         &mut self,
//         ctx: &xi_rpc::RpcCtx,
//         rpc: Self::Notification,
//     ) {
//         match rpc {
//             Notification::SemanticTokens {
//                 rev,
//                 buffer_id,
//                 path,
//                 tokens,
//             } => {
//                 self.event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::UpdateSemanticTokens(
//                         buffer_id, path, rev, tokens,
//                     ),
//                     Target::Widget(self.tab_id),
//                 );
//             }
//             Notification::ReloadBuffer {
//                 buffer_id,
//                 new_content,
//                 rev,
//             } => {
//                 self.event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::ReloadBuffer(buffer_id, rev, new_content),
//                     Target::Widget(self.tab_id),
//                 );
//             }
//             Notification::PublishDiagnostics { diagnostics } => {
//                 self.event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::PublishDiagnostics(diagnostics),
//                     Target::Widget(self.tab_id),
//                 );
//             }
//             Notification::WorkDoneProgress { progress } => {
//                 self.event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::WorkDoneProgress(progress),
//                     Target::Widget(self.tab_id),
//                 );
//             }
//             Notification::InstalledPlugins { plugins } => {
//                 self.event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::UpdateInstalledPlugins(plugins),
//                     Target::Widget(self.tab_id),
//                 );
//             }
//             Notification::ListDir { items } => {}
//             Notification::DiffFiles { files } => {}
//             Notification::FileDiffs { diffs } => {
//                 self.event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::UpdateFileDiffs(diffs),
//                     Target::Widget(self.tab_id),
//                 );
//             }
//             Notification::UpdateTerminal { term_id, content } => {
//                 self.term_tx
//                     .send((term_id, TermEvent::UpdateContent(content)));
//             }
//             Notification::CloseTerminal { term_id } => {
//                 self.term_tx.send((term_id, TermEvent::CloseTerminal));
//                 self.event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::CloseTerminal(term_id),
//                     Target::Widget(self.tab_id),
//                 );
//             }
//         }
//     }
//
//     fn handle_request(
//         &mut self,
//         ctx: &xi_rpc::RpcCtx,
//         rpc: Self::Request,
//     ) -> Result<serde_json::Value, xi_rpc::RemoteError> {
//         Err(xi_rpc::RemoteError::InvalidRequest(None))
//     }
// }
