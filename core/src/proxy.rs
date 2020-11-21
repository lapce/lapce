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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    StartLspServer {
        exec_path: String,
        language_id: String,
        options: Option<Value>,
    },
    UpdateGit {
        buffer_id: BufferId,
        line_changes: HashMap<usize, char>,
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
            Notification::StartLspServer {
                exec_path,
                language_id,
                options,
            } => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .lsp
                    .lock()
                    .start_server(&exec_path, &language_id, options);
            }
            Notification::UpdateGit {
                buffer_id,
                line_changes,
            } => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .buffers
                    .get_mut(&buffer_id)
                    .unwrap()
                    .line_changes = line_changes;
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
    workspace: PathBuf,
) {
    thread::spawn(move || {
        let child = Command::new("/Users/Lulu/lapce/target/debug/lapce-proxy")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn();
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
        proxy.initialize(workspace);
        {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            *state.proxy.lock() = Some(proxy);
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
