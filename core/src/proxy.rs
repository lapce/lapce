use std::collections::HashMap;
use std::io::BufReader;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::{path::PathBuf, process::Child, sync::Arc};

use alacritty_terminal::term::cell::Cell;
use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use crossbeam_utils::sync::WaitGroup;
use druid::{ExtEventSink, WidgetId};
use druid::{Target, WindowId};
use flate2::read::GzDecoder;
use lapce_proxy::dispatch::{FileNodeItem, NewBufferResponse};
use lapce_proxy::plugin::PluginDescription;
use lapce_proxy::terminal::TermId;
use lsp_types::CompletionItem;
use lsp_types::Position;
use lsp_types::ProgressParams;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::WorkDoneProgress;
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::Value;
use xi_rope::RopeDelta;
use xi_rpc::Callback;
use xi_rpc::Handler;
use xi_rpc::RpcLoop;
use xi_rpc::RpcPeer;

use crate::command::LapceUICommand;
use crate::state::LapceWorkspace;
use crate::state::LapceWorkspaceType;
use crate::terminal::RawTerminal;
use crate::{buffer::BufferId, command::LAPCE_UI_COMMAND};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub enum TermEvent {
    NewTerminal(Arc<Mutex<RawTerminal>>),
    UpdateContent(String),
    CloseTerminal,
}

#[derive(Clone)]
pub struct LapceProxy {
    peer: Arc<Mutex<Option<RpcPeer>>>,
    process: Arc<Mutex<Option<Child>>>,
    initiated: Arc<Mutex<bool>>,
    cond: Arc<Condvar>,
    term_tx: Sender<(TermId, TermEvent)>,
    pub tab_id: WidgetId,
}

impl LapceProxy {
    pub fn new(tab_id: WidgetId, term_tx: Sender<(TermId, TermEvent)>) -> Self {
        let proxy = Self {
            peer: Arc::new(Mutex::new(None)),
            process: Arc::new(Mutex::new(None)),
            initiated: Arc::new(Mutex::new(false)),
            cond: Arc::new(Condvar::new()),
            term_tx,
            tab_id,
        };
        proxy
    }

    pub fn start(&self, workspace: LapceWorkspace, event_sink: ExtEventSink) {
        let proxy = self.clone();
        *proxy.initiated.lock() = false;
        let tab_id = self.tab_id;
        let term_tx = self.term_tx.clone();
        thread::spawn(move || {
            let mut child = match workspace.kind {
                LapceWorkspaceType::Local => Command::new(
                    std::env::current_exe()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("lapce-proxy"),
                )
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn(),
                LapceWorkspaceType::RemoteSSH(user, host) => {
                    let cmd = Command::new("ssh")
                        .arg(format!("{}@{}", user, host))
                        .arg("-o")
                        .arg("ControlMaster=auto")
                        .arg("-o")
                        .arg("ControlPath=~/.ssh/cm-%r@%h:%p")
                        .arg("-o")
                        .arg("ControlPersist=30m")
                        .arg("test")
                        .arg("-a")
                        .arg(format!("~/.lapce/lapce-proxy-{}", VERSION))
                        .output()
                        .unwrap();
                    println!("ssh check proxy file {:?}", cmd.status);
                    if !cmd.status.success() {
                        let url = format!("https://github.com/lapce/lapce/releases/download/v{VERSION}/lapce-proxy-linux.gz");
                        let mut resp =
                            reqwest::blocking::get(url).expect("request failed");
                        let local_path = format!("/tmp/lapce-proxy-{}", VERSION);
                        let mut out = std::fs::File::create(&local_path)
                            .expect("failed to create file");
                        let mut gz = GzDecoder::new(&mut resp);
                        std::io::copy(&mut gz, &mut out)
                            .expect("failed to copy content");

                        Command::new("ssh")
                            .arg(format!("{}@{}", user, host))
                            .arg("-o")
                            .arg("ControlMaster=auto")
                            .arg("-o")
                            .arg("ControlPath=~/.ssh/cm-%r@%h:%p")
                            .arg("-o")
                            .arg("ControlPersist=30m")
                            .arg("mkdir")
                            .arg("~/.lapce/")
                            .output()
                            .unwrap();

                        Command::new("scp")
                            .arg("-o")
                            .arg("ControlMaster=auto")
                            .arg("-o")
                            .arg("ControlPath=~/.ssh/cm-%r@%h:%p")
                            .arg("-o")
                            .arg("ControlPersist=30m")
                            .arg(&local_path)
                            .arg(format!(
                                "{user}@{host}:~/.lapce/lapce-proxy-{VERSION}"
                            ))
                            .output()
                            .unwrap();

                        Command::new("ssh")
                            .arg(format!("{}@{}", user, host))
                            .arg("-o")
                            .arg("ControlMaster=auto")
                            .arg("-o")
                            .arg("ControlPath=~/.ssh/cm-%r@%h:%p")
                            .arg("-o")
                            .arg("ControlPersist=30m")
                            .arg("chmod")
                            .arg("+x")
                            .arg(format!("~/.lapce/lapce-proxy-{}", VERSION))
                            .output()
                            .unwrap();
                    }

                    Command::new("ssh")
                        .arg(format!("{}@{}", user, host))
                        .arg("-o")
                        .arg("ControlMaster=auto")
                        .arg("-o")
                        .arg("ControlPath=~/.ssh/cm-%r@%h:%p")
                        .arg("-o")
                        .arg("ControlPersist=30m")
                        .arg(format!("~/.lapce/lapce-proxy-{}", VERSION))
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                }
            };
            if child.is_err() {
                println!("can't start proxy {:?}", child);
                return;
            }
            let mut child = child.unwrap();
            let child_stdin = child.stdin.take().unwrap();
            let child_stdout = child.stdout.take().unwrap();
            let mut looper = RpcLoop::new(child_stdin);
            let peer: RpcPeer = Box::new(looper.get_raw_peer());
            {
                *proxy.peer.lock() = Some(peer);
                let mut process = proxy.process.lock();
                let mut old_process = process.take();
                *process = Some(child);
                if let Some(mut old) = old_process {
                    old.kill();
                }
            }
            proxy.initialize(workspace.path.clone());
            {
                *proxy.initiated.lock() = true;
                proxy.cond.notify_all();
            }

            let mut handler = ProxyHandlerNew {
                tab_id,
                term_tx,
                event_sink,
            };
            if let Err(e) =
                looper.mainloop(|| BufReader::new(child_stdout), &mut handler)
            {
                println!("proxy main loop failed {:?}", e);
            }
            println!("proxy main loop exit");
        });
    }

    fn wait(&self) {
        let mut initiated = self.initiated.lock();
        if !*initiated {
            self.cond.wait(&mut initiated);
        }
    }

    pub fn initialize(&self, workspace: PathBuf) {
        self.peer.lock().as_ref().unwrap().send_rpc_notification(
            "initialize",
            &json!({
                "workspace": workspace,
            }),
        )
    }

    pub fn terminal_close(&self, term_id: TermId) {
        self.peer.lock().as_ref().unwrap().send_rpc_notification(
            "terminal_close",
            &json!({
                "term_id": term_id,
            }),
        )
    }

    pub fn terminal_resize(&self, term_id: TermId, width: usize, height: usize) {
        self.wait();
        self.peer.lock().as_ref().unwrap().send_rpc_notification(
            "terminal_resize",
            &json!({
                "term_id": term_id,
                "width": width,
                "height": height,
            }),
        )
    }

    pub fn terminal_write(&self, term_id: TermId, content: &str) {
        self.wait();
        self.peer.lock().as_ref().unwrap().send_rpc_notification(
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
        raw: Arc<Mutex<RawTerminal>>,
    ) {
        self.term_tx.send((term_id, TermEvent::NewTerminal(raw)));
        self.wait();
        self.peer.lock().as_ref().unwrap().send_rpc_notification(
            "new_terminal",
            &json!({
                "term_id": term_id,
                "cwd": cwd,
            }),
        )
    }

    pub fn install_plugin(&self, plugin: &PluginDescription) {
        self.peer
            .lock()
            .as_ref()
            .unwrap()
            .send_rpc_notification("install_plugin", &json!({ "plugin": plugin }));
    }

    pub fn new_buffer(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: Box<dyn Callback>,
    ) {
        self.wait();
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
            "new_buffer",
            &json!({ "buffer_id": buffer_id, "path": path }),
            f,
        );
    }

    pub fn update(&self, buffer_id: BufferId, delta: &RopeDelta, rev: u64) {
        self.peer.lock().as_ref().unwrap().send_rpc_notification(
            "update",
            &json!({
                "buffer_id": buffer_id,
                "delta": delta,
                "rev": rev,
            }),
        )
    }

    pub fn save(&self, rev: u64, buffer_id: BufferId, f: Box<dyn Callback>) {
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
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
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
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
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
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
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
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
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
            "get_references",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_files(&self, f: Box<dyn Callback>) {
        if let Some(peer) = self.peer.lock().as_ref() {
            peer.send_rpc_request_async(
                "get_files",
                &json!({
                    "path": "path",
                }),
                f,
            );
        }
    }

    pub fn read_dir(&self, path: &PathBuf, f: Box<dyn Callback>) {
        self.wait();
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
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
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
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
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
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
        if let Some(peer) = self.peer.lock().as_ref() {
            peer.send_rpc_request_async(
                "get_code_actions",
                &json!({
                    "buffer_id": buffer_id,
                    "position": position,
                }),
                f,
            );
        }
    }

    pub fn get_document_formatting(
        &self,
        buffer_id: BufferId,
        f: Box<dyn Callback>,
    ) {
        self.peer.lock().as_ref().unwrap().send_rpc_request_async(
            "get_document_formatting",
            &json!({
                "buffer_id": buffer_id,
            }),
            f,
        );
    }

    pub fn stop(&self) {
        let mut process = self.process.lock();
        if let Some(mut p) = process.as_mut() {
            p.kill();
        }
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
    SemanticTokens {
        rev: u64,
        buffer_id: BufferId,
        path: PathBuf,
        tokens: Vec<(usize, usize, String)>,
    },
    UpdateGit {
        buffer_id: BufferId,
        line_changes: HashMap<usize, char>,
        rev: u64,
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
    InstalledPlugins {
        plugins: HashMap<String, PluginDescription>,
    },
    ListDir {
        items: Vec<FileNodeItem>,
    },
    DiffFiles {
        files: Vec<PathBuf>,
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
    tab_id: WidgetId,
    term_tx: Sender<(TermId, TermEvent)>,
    event_sink: ExtEventSink,
}

impl Handler for ProxyHandlerNew {
    type Notification = Notification;
    type Request = Request;

    fn handle_notification(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Notification,
    ) {
        match rpc {
            Notification::SemanticTokens {
                rev,
                buffer_id,
                path,
                tokens,
            } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSemanticTokens(
                        buffer_id, path, rev, tokens,
                    ),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::UpdateGit {
                buffer_id,
                line_changes,
                rev,
            } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateBufferLineChanges(
                        buffer_id,
                        rev,
                        line_changes,
                    ),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::ReloadBuffer {
                buffer_id,
                new_content,
                rev,
            } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ReloadBuffer(buffer_id, rev, new_content),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::PublishDiagnostics { diagnostics } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PublishDiagnostics(diagnostics),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::WorkDoneProgress { progress } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::WorkDoneProgress(progress),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::InstalledPlugins { plugins } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateInstalledPlugins(plugins),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::ListDir { items } => {}
            Notification::DiffFiles { files } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateDiffFiles(files),
                    Target::Widget(self.tab_id),
                );
            }
            Notification::UpdateTerminal { term_id, content } => {
                self.term_tx
                    .send((term_id, TermEvent::UpdateContent(content)));
            }
            Notification::CloseTerminal { term_id } => {
                self.term_tx.send((term_id, TermEvent::CloseTerminal));
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CloseTerminal(term_id),
                    Target::Widget(self.tab_id),
                );
            }
        }
    }

    fn handle_request(
        &mut self,
        ctx: &xi_rpc::RpcCtx,
        rpc: Self::Request,
    ) -> Result<serde_json::Value, xi_rpc::RemoteError> {
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }
}
