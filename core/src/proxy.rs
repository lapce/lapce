use std::collections::HashMap;
use std::io::BufReader;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::{path::PathBuf, process::Child, sync::Arc};

use anyhow::{anyhow, Result};
use crossbeam_utils::sync::WaitGroup;
use druid::{ExtEventSink, WidgetId};
use druid::{Target, WindowId};
use lapce_proxy::dispatch::{FileNodeItem, NewBufferResponse};
use lsp_types::CompletionItem;
use lsp_types::Position;
use lsp_types::PublishDiagnosticsParams;
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
use crate::state::LAPCE_APP_STATE;
use crate::{buffer::BufferId, command::LAPCE_UI_COMMAND};

#[derive(Clone)]
pub struct LapceProxy {
    peer: Arc<Mutex<Option<RpcPeer>>>,
    process: Arc<Mutex<Option<Child>>>,
    initiated: Arc<Mutex<bool>>,
    cond: Arc<Condvar>,
    pub tab_id: WidgetId,
}

impl LapceProxy {
    pub fn new(tab_id: WidgetId) -> Self {
        let proxy = Self {
            peer: Arc::new(Mutex::new(None)),
            process: Arc::new(Mutex::new(None)),
            initiated: Arc::new(Mutex::new(false)),
            cond: Arc::new(Condvar::new()),
            tab_id,
        };
        proxy
    }

    pub fn start(&self, workspace: LapceWorkspace, event_sink: ExtEventSink) {
        let proxy = self.clone();
        *proxy.initiated.lock() = false;
        let tab_id = self.tab_id;
        thread::spawn(move || {
            let mut child = match workspace.kind {
                LapceWorkspaceType::Local => {
                    Command::new("/Users/Lulu/.lapce/lapce-proxy")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                }
                LapceWorkspaceType::RemoteSSH(user, host) => Command::new("ssh")
                    .arg(format!("{}@{}", user, host))
                    .arg("/tmp/proxy/target/release/lapce-proxy")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn(),
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
                proxy.cond.notify_one();
            }

            let mut handler = ProxyHandlerNew { tab_id, event_sink };
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

    pub fn new_buffer(&self, buffer_id: BufferId, path: PathBuf) -> Result<String> {
        self.wait();
        let result = self
            .peer
            .lock()
            .as_ref()
            .unwrap()
            .send_rpc_request(
                "new_buffer",
                &json!({ "buffer_id": buffer_id, "path": path }),
            )
            .map_err(|e| anyhow!("{:?}", e))?;

        let resp: NewBufferResponse = serde_json::from_value(result)?;
        return Ok(resp.content);
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
    ListDir {
        items: Vec<FileNodeItem>,
    },
    DiffFiles {
        files: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {}

pub struct ProxyHandler {
    window_id: WindowId,
    tab_id: WidgetId,
}

impl Handler for ProxyHandler {
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
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                let mut editor_split = state.editor_split.lock();
                let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
                if buffer.rev != rev {
                    return;
                }
                buffer.semantic_tokens = Some(tokens);
                buffer.line_highlights = HashMap::new();
            }
            Notification::UpdateGit {
                buffer_id,
                line_changes,
                rev,
            } => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                let mut editor_split = state.editor_split.lock();
                let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
                if buffer.rev != rev {
                    return;
                }
                buffer.line_changes = line_changes;
                LAPCE_APP_STATE.submit_ui_command(
                    LapceUICommand::UpdateLineChanges(buffer_id),
                    self.tab_id,
                );
            }
            Notification::ReloadBuffer {
                buffer_id,
                new_content,
                rev,
            } => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                let mut editor_split = state.editor_split.lock();
                let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
                if buffer.rev + 1 != rev {
                    return;
                }
            }
            Notification::ListDir { mut items } => {
                items.sort();
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                let mut file_explorer = state.file_explorer.lock();
                file_explorer.items = items;
                file_explorer.update_count();
                LAPCE_APP_STATE.submit_ui_command(
                    LapceUICommand::RequestPaint,
                    file_explorer.widget_id,
                );
            }
            Notification::DiffFiles { files } => {
                println!("get diff files {:?}", files);
                let window_id = self.window_id;
                let tab_id = self.tab_id;
                std::thread::spawn(move || {
                    let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
                    let mut source_control = state.source_control.lock();
                    source_control.diff_files = files;
                    LAPCE_APP_STATE.submit_ui_command(
                        LapceUICommand::RequestPaint,
                        source_control.widget_id,
                    );
                });
            }
            Notification::PublishDiagnostics { diagnostics } => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                let mut editor_split = state.editor_split.lock();
                let path = diagnostics.uri.path().to_string();
                editor_split
                    .diagnostics
                    .insert(path.clone(), diagnostics.diagnostics);

                LAPCE_APP_STATE.submit_ui_command(
                    LapceUICommand::RequestPaint,
                    state.status_id,
                );
                for (_, editor) in editor_split.editors.iter() {
                    if let Some(buffer_id) = editor.buffer_id.as_ref() {
                        if let Some(buffer) = editor_split.buffers.get(buffer_id) {
                            if buffer.path == path {
                                LAPCE_APP_STATE.submit_ui_command(
                                    LapceUICommand::RequestPaint,
                                    editor.view_id,
                                );
                            }
                        }
                    }
                }
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

pub fn start_proxy_process(
    window_id: WindowId,
    tab_id: WidgetId,
    workspace: LapceWorkspace,
) {
    thread::spawn(move || {
        let mut child = match workspace.kind {
            LapceWorkspaceType::Local => {
                Command::new("/Users/Lulu/lapce/target/release/lapce-proxy")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
            }
            LapceWorkspaceType::RemoteSSH(user, host) => Command::new("ssh")
                .arg(format!("{}@{}", user, host))
                .arg("/tmp/proxy/target/release/lapce-proxy")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn(),
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
        let proxy = LapceProxy {
            peer: Arc::new(Mutex::new(Some(peer))),
            process: Arc::new(Mutex::new(Some(child))),
            cond: Arc::new(Condvar::new()),
            initiated: Arc::new(Mutex::new(false)),
            tab_id,
        };
        proxy.initialize(workspace.path.clone());
        {
            *proxy.initiated.lock() = true;
            proxy.cond.notify_one();
        }
        {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let mut proxy_state = state.proxy.lock();
            let old_proxy = proxy_state.take();
            *proxy_state = Some(proxy);
            if let Some(old_proxy) = old_proxy {
                let mut process = old_proxy.process.lock();
                let old_process = process.take();
                if let Some(mut old) = old_process {
                    old.kill();
                }
            }
        }

        let mut handler = ProxyHandler { window_id, tab_id };
        if let Err(e) =
            looper.mainloop(|| BufReader::new(child_stdout), &mut handler)
        {
            println!("proxy main loop failed {:?}", e);
        }
        println!("proxy main loop exit");
    });
}

pub struct ProxyHandlerNew {
    tab_id: WidgetId,
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
            Notification::ListDir { items } => {}
            Notification::DiffFiles { files } => {
                self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateDiffFiles(files),
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
