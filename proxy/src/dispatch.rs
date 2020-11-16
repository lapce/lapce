use crate::buffer::{Buffer, BufferId};
use crate::core_proxy::CoreProxy;
use crate::plugin::PluginCatalog;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use xi_rpc::RpcPeer;
use xi_rpc::{Handler, RpcCtx};

pub struct Dispatcher {
    peer: RpcPeer,
    buffers: HashMap<BufferId, Buffer>,
    plugins: Arc<Mutex<PluginCatalog>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Request {
    NewBuffer { buffer_id: BufferId, path: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBufferResponse {
    pub content: String,
}

impl Dispatcher {
    pub fn new(peer: RpcPeer) -> Dispatcher {
        let mut plugins = PluginCatalog::new();
        plugins.reload();
        plugins.start_all(CoreProxy::new(peer.clone()));
        Dispatcher {
            peer,
            buffers: HashMap::new(),
            plugins: Arc::new(Mutex::new(plugins)),
        }
    }
}

impl Handler for Dispatcher {
    type Notification = Notification;
    type Request = Request;

    fn handle_notification(&mut self, ctx: &RpcCtx, rpc: Self::Notification) {}

    fn handle_request(
        &mut self,
        ctx: &RpcCtx,
        rpc: Self::Request,
    ) -> Result<Value, xi_rpc::RemoteError> {
        match rpc {
            Request::NewBuffer { buffer_id, path } => {
                let buffer = Buffer::new(buffer_id, path);
                let content = buffer.rope.to_string();
                self.buffers.insert(buffer_id, buffer);
                let resp = NewBufferResponse { content };
                return Ok(serde_json::to_value(resp).unwrap());
            }
        }
        Err(xi_rpc::RemoteError::InvalidRequest(None))
    }
}
