use std::collections::HashMap;
use std::io::BufReader;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::{path::PathBuf, process::Child, sync::Arc};

use anyhow::{anyhow, Result};
use druid::WidgetId;
use druid::WindowId;
use lapce_proxy::dispatch::NewBufferResponse;
use lsp_types::Position;
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::Value;
use xi_rope::RopeDelta;
use xi_rpc::Callback;
use xi_rpc::Handler;
use xi_rpc::RpcLoop;
use xi_rpc::RpcPeer;

use crate::buffer::BufferId;
use crate::command::LapceUICommand;
use crate::state::LapceWorkspace;
use crate::state::LapceWorkspaceType;
use crate::state::LAPCE_APP_STATE;

#[derive(Clone)]
pub struct LapceProxy {
    peer: RpcPeer,
    process: Arc<Mutex<Child>>,
}

impl LapceProxy {
    pub fn initialize(&self, workspace: PathBuf) {
        self.peer.send_rpc_notification(
            "initialize",
            &json!({
                "workspace": workspace,
            }),
        )
    }

    pub fn new_buffer(&self, buffer_id: BufferId, path: PathBuf) -> Result<String> {
        let result = self
            .peer
            .send_rpc_request(
                "new_buffer",
                &json!({ "buffer_id": buffer_id, "path": path }),
            )
            .map_err(|e| anyhow!("{:?}", e))?;

        let resp: NewBufferResponse = serde_json::from_value(result)?;
        return Ok(resp.content);
    }

    pub fn update(&self, buffer_id: BufferId, delta: &RopeDelta, rev: u64) {
        self.peer.send_rpc_notification(
            "update",
            &json!({
                "buffer_id": buffer_id,
                "delta": delta,
                "rev": rev,
            }),
        )
    }

    pub fn save(&self, rev: u64, buffer_id: BufferId, f: Box<dyn Callback>) {
        self.peer.send_rpc_request_async(
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
        self.peer.send_rpc_request_async(
            "get_completion",
            &json!({
                "request_id": request_id,
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
        self.peer.send_rpc_request_async(
            "get_document_formatting",
            &json!({
                "buffer_id": buffer_id,
            }),
            f,
        );
    }

    pub fn stop(&self) {
        self.process.lock().kill();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    SemanticTokens {
        rev: u64,
        buffer_id: BufferId,
        tokens: Vec<(usize, usize, String)>,
    },
    UpdateGit {
        buffer_id: BufferId,
        line_changes: HashMap<usize, char>,
        rev: u64,
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
                for (view_id, editor) in editor_split.editors.iter() {
                    if editor.buffer_id.as_ref() == Some(&buffer_id) {
                        LAPCE_APP_STATE.submit_ui_command(
                            LapceUICommand::FillTextLayouts,
                            view_id.clone(),
                        );
                    }
                }
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
                Command::new("/Users/Lulu/lapce/target/debug/lapce-proxy")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
            }
            LapceWorkspaceType::RemoteSSH(user, host) => Command::new("ssh")
                .arg(format!("{}@{}", user, host))
                .arg("./.lapce/lapce-proxy")
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
            peer,
            process: Arc::new(Mutex::new(child)),
        };
        proxy.initialize(workspace.path.clone());
        {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let mut proxy_state = state.proxy.lock();
            let old_proxy = proxy_state.take();
            *proxy_state = Some(proxy);
            if let Some(old_proxy) = old_proxy {
                old_proxy.process.lock().kill();
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
