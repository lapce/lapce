use crate::buffer::{Buffer, BufferId};
use crate::core_proxy::CoreProxy;
use crate::lsp::LspCatalog;
use crate::plugin::PluginCatalog;
use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use jsonrpc_lite::{self, JsonRpc};
use lapce_rpc::{self, Call, RequestId, RpcObject};
use lsp_types::Position;
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::Value;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::{collections::HashMap, io};
use xi_rpc::RpcPeer;
use xi_rpc::{Handler, RpcCtx};

#[derive(Clone)]
pub struct Dispatcher {
    pub sender: Arc<Sender<Value>>,
    pub workspace: Arc<Mutex<PathBuf>>,
    buffers: Arc<Mutex<HashMap<BufferId, Buffer>>>,
    plugins: Arc<Mutex<PluginCatalog>>,
    pub lsp: Arc<Mutex<LspCatalog>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    Initialize { workspace: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Request {
    NewBuffer {
        buffer_id: BufferId,
        path: PathBuf,
    },
    GetCompletion {
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBufferResponse {
    pub content: String,
}

impl Dispatcher {
    pub fn new(sender: Sender<Value>) -> Dispatcher {
        let plugins = PluginCatalog::new();
        let dispatcher = Dispatcher {
            sender: Arc::new(sender),
            workspace: Arc::new(Mutex::new(PathBuf::new())),
            buffers: Arc::new(Mutex::new(HashMap::new())),
            plugins: Arc::new(Mutex::new(plugins)),
            lsp: Arc::new(Mutex::new(LspCatalog::new())),
        };
        dispatcher.lsp.lock().dispatcher = Some(dispatcher.clone());
        dispatcher.plugins.lock().reload();
        dispatcher.plugins.lock().start_all(dispatcher.clone());
        dispatcher
    }

    pub fn mainloop(&self, receiver: Receiver<Value>) -> Result<()> {
        for msg in receiver {
            eprintln!("receive msg {}", msg);
            let rpc: RpcObject = msg.into();
            if rpc.is_response() {
            } else {
                match rpc.into_rpc::<Notification, Request>() {
                    Ok(Call::Request(id, request)) => {
                        self.handle_request(id, request);
                    }
                    Ok(Call::Notification(notification)) => {
                        self.handle_notification(notification)
                    }
                    Err(e) => {}
                }
            }
        }
        Ok(())
    }

    pub fn next<R: BufRead>(
        &self,
        reader: &mut R,
        s: &mut String,
    ) -> Result<RpcObject> {
        s.clear();
        let _ = reader.read_line(s)?;
        if s.is_empty() {
            Err(anyhow!("empty line"))
        } else {
            self.parse(s)
        }
    }

    fn parse(&self, s: &str) -> Result<RpcObject> {
        let val = serde_json::from_str::<Value>(&s)?;
        if !val.is_object() {
            Err(anyhow!("not json object"))
        } else {
            Ok(val.into())
        }
    }

    fn handle_notification(&self, rpc: Notification) {
        match rpc {
            Notification::Initialize { workspace } => {
                *self.workspace.lock() = workspace;
            }
        }
    }

    fn handle_request(&self, id: RequestId, rpc: Request) {
        match rpc {
            Request::NewBuffer { buffer_id, path } => {
                let buffer = Buffer::new(buffer_id, path);
                let content = buffer.rope.to_string();
                self.buffers.lock().insert(buffer_id, buffer);
                let resp = NewBufferResponse { content };
                eprintln!("proxy receive new buffer");
                self.sender.send(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": resp,
                }));
            }
            Request::GetCompletion {
                buffer_id,
                position,
                request_id,
            } => {
                let buffers = self.buffers.lock();
                let buffer = buffers.get(&buffer_id).unwrap();
                self.lsp
                    .lock()
                    .get_completion(id, request_id, buffer, position);
            }
        }
    }
}
