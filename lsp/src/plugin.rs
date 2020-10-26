use anyhow::Result;
use lapce_core::{
    buffer::BufferId,
    plugin::{PluginBufferInfo, PluginId},
};
use serde_json::{json, Value};
use xi_rope::RopeDelta;
use xi_rpc::{RpcCtx, RpcPeer};

use crate::buffer::Buffer;

#[derive(Clone)]
pub struct CoreProxy {
    plugin_id: PluginId,
    peer: RpcPeer,
}

impl CoreProxy {
    pub fn new(plugin_id: PluginId, rpc_ctx: &RpcCtx) -> Self {
        CoreProxy {
            plugin_id,
            peer: rpc_ctx.get_peer().clone(),
        }
    }

    pub fn start_lsp_server(&mut self, exec_path: &str, language_id: &str) {
        let params = json!({
            "plugin_id": self.plugin_id,
            "buffer_id": BufferId(0),
            "exec_path": exec_path,
            "language_id": language_id,
        });

        self.peer.send_rpc_notification("start_lsp_server", &params);
    }

    pub fn show_completion(
        &mut self,
        buffer_id: BufferId,
        request_id: usize,
        result: &Value,
    ) {
        let params = json!({
            "plugin_id": self.plugin_id,
            "buffer_id": buffer_id,
            "request_id": request_id,
            "result": result,
        });

        self.peer.send_rpc_notification("show_completion", &params);
    }

    pub fn schedule_idle(&mut self, buffer_id: BufferId) {
        let token: usize = buffer_id.0;
        self.peer.schedule_idle(token);
    }
}

pub trait Plugin {
    fn initialize(&mut self, core: CoreProxy);

    fn new_buffer(&mut self, buffer: &mut Buffer);

    fn update(&mut self, buffer: &mut Buffer, delta: &RopeDelta, rev: u64);

    fn get_completion(
        &mut self,
        buffer: &mut Buffer,
        request_id: usize,
        offset: usize,
    );

    fn idle(&mut self, buffer: &mut Buffer);
}
