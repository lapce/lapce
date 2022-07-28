use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use dyn_clone::DynClone;
use jsonrpc_lite::{JsonRpc, Params};
use lapce_rpc::RpcError;
use lsp_types::notification::Notification;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{lsp::NewLspClient, number_from_id, PluginCatalogRpcHandler};

enum ResponseHandler<Resp, Error> {
    Chan(Sender<Result<Resp, Error>>),
    Callback(Box<dyn RpcCallback<Resp, Error>>),
}

impl<Resp, Error> ResponseHandler<Resp, Error> {
    fn invoke(self, result: Result<Resp, Error>) {
        match self {
            ResponseHandler::Chan(tx) => {
                let _ = tx.send(result);
            }
            ResponseHandler::Callback(f) => f.call(result),
        }
    }
}

pub trait ClonableCallback:
    FnOnce(Result<Value, RpcError>) + Send + DynClone
{
}

impl<F: Send + FnOnce(Result<Value, RpcError>) + DynClone> ClonableCallback for F {}

pub trait RpcCallback<Resp, Error>: Send {
    fn call(self: Box<Self>, result: Result<Resp, Error>);
}

impl<Resp, Error, F: Send + FnOnce(Result<Resp, Error>)> RpcCallback<Resp, Error>
    for F
{
    fn call(self: Box<F>, result: Result<Resp, Error>) {
        (*self)(result)
    }
}

pub enum PluginHandlerNotification {
    Initilize,
}

pub enum PluginServerRpc {
    Handler(PluginHandlerNotification),
    ServerRequest {
        id: u64,
        method: &'static str,
        params: Params,
    },
    ServerNotification {
        method: &'static str,
        params: Params,
    },
    ServerResponse {
        id: u64,
        result: Value,
    },
    ServerError {
        id: u64,
        error: RpcError,
    },
    HostRequest {
        id: u64,
        method: String,
        params: Params,
    },
    HostNotification {
        method: String,
        params: Params,
    },
}

#[derive(Clone)]
pub struct PluginServerRpcHandler {
    rpc_tx: Sender<PluginServerRpc>,
    rpc_rx: Receiver<PluginServerRpc>,
    io_tx: Sender<String>,
    id: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<u64, ResponseHandler<Value, RpcError>>>>,
}

pub trait PluginServerHandler {
    fn method_registered(&mut self, method: &'static str) -> bool;
    fn handle_host_notification(&mut self, method: String, params: Params);
    fn handle_handler_notification(
        &mut self,
        notification: PluginHandlerNotification,
    );
}

impl PluginServerRpcHandler {
    pub fn new(io_tx: Sender<String>) -> Self {
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded();

        let rpc = Self {
            rpc_tx,
            rpc_rx,
            io_tx,
            id: Arc::new(AtomicU64::new(0)),
            server_pending: Arc::new(Mutex::new(HashMap::new())),
        };

        rpc.initilize();
        rpc
    }

    fn initilize(&self) {
        self.handle_rpc(PluginServerRpc::Handler(
            PluginHandlerNotification::Initilize,
        ));
    }

    fn send_server_rpc(&self, msg: String) {
        let _ = self.io_tx.send(msg);
    }

    pub fn handle_rpc(&self, rpc: PluginServerRpc) {
        let _ = self.rpc_tx.send(rpc);
    }

    pub fn server_notification<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
    ) {
        let params = Params::from(serde_json::to_value(params).unwrap());
        let _ = self
            .rpc_tx
            .send(PluginServerRpc::ServerNotification { method, params });
    }

    pub fn server_request<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<Value, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.server_request_common(method, params, ResponseHandler::Chan(tx));
        rx.recv().unwrap_or_else(|_| {
            Err(RpcError {
                code: 0,
                message: "io error".to_string(),
            })
        })
    }

    pub fn server_request_async(
        &self,
        method: &'static str,
        params: Value,
        f: impl RpcCallback<Value, RpcError> + 'static,
    ) {
        self.server_request_common(
            method,
            params,
            ResponseHandler::Callback(Box::new(f)),
        );
    }

    fn server_request_common<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        rh: ResponseHandler<Value, RpcError>,
    ) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.server_pending.lock();
            pending.insert(id, rh);
        }
        let params = Params::from(serde_json::to_value(params).unwrap());
        let _ =
            self.rpc_tx
                .send(PluginServerRpc::ServerRequest { id, method, params });
    }

    pub fn handle_server_response(&self, id: u64, result: Result<Value, RpcError>) {
        if let Some(handler) = { self.server_pending.lock().remove(&id) } {
            handler.invoke(result);
        }
    }

    pub fn mainloop<H>(&self, handler: &mut H)
    where
        H: PluginServerHandler,
    {
        for msg in &self.rpc_rx {
            match msg {
                PluginServerRpc::ServerRequest { id, method, params } => {
                    if handler.method_registered(method) {
                        let msg =
                            JsonRpc::request_with_params(id as i64, method, params);
                        let msg = serde_json::to_string(&msg).unwrap();
                        self.send_server_rpc(msg);
                    } else {
                        self.handle_server_response(
                            id,
                            Err(RpcError {
                                code: 0,
                                message: "server not capable".to_string(),
                            }),
                        );
                    }
                }
                PluginServerRpc::ServerNotification { method, params } => {
                    if handler.method_registered(method) {
                        let msg = JsonRpc::notification_with_params(method, params);
                        let msg = serde_json::to_string(&msg).unwrap();
                        self.send_server_rpc(msg);
                    }
                }
                PluginServerRpc::ServerResponse { id, result } => {
                    self.handle_server_response(id, Ok(result));
                }
                PluginServerRpc::ServerError { id, error } => {
                    self.handle_server_response(id, Err(error));
                }
                PluginServerRpc::HostRequest { id, method, params } => todo!(),
                PluginServerRpc::HostNotification { method, params } => {
                    handler.handle_host_notification(method, params);
                }
                PluginServerRpc::Handler(notification) => {
                    handler.handle_handler_notification(notification)
                }
            }
        }
    }
}

pub fn handle_plugin_server_message(message: &str) -> Result<PluginServerRpc> {
    let rpc = match JsonRpc::parse(message) {
        Ok(value @ JsonRpc::Request(_)) => {
            let id = number_from_id(&value.get_id().unwrap());
            PluginServerRpc::HostRequest {
                id,
                method: value.get_method().unwrap().to_string(),
                params: value.get_params().unwrap(),
            }
        }
        Ok(value @ JsonRpc::Notification(_)) => PluginServerRpc::HostNotification {
            method: value.get_method().unwrap().to_string(),
            params: value.get_params().unwrap(),
        },
        Ok(value @ JsonRpc::Success(_)) => {
            let id = number_from_id(&value.get_id().unwrap());
            let result = value.get_result().unwrap().clone();
            PluginServerRpc::ServerResponse { id, result }
        }
        Ok(value @ JsonRpc::Error(_)) => {
            let id = number_from_id(&value.get_id().unwrap());
            let error = value.get_error().unwrap();
            PluginServerRpc::ServerError {
                id,
                error: RpcError {
                    code: error.code,
                    message: error.message.clone(),
                },
            }
        }
        Err(_err) => return Err(anyhow!("parsing error")),
    };
    Ok(rpc)
}

pub enum StartLspServer {}

impl Notification for StartLspServer {
    type Params = StartLspServerParams;
    const METHOD: &'static str = "start_lsp_server";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartLspServerParams {
    pub exec_path: String,
    pub language_id: String,
    pub options: Option<Value>,
    pub system_lsp: Option<bool>,
}

pub struct PluginHostHandler {
    workspace: Option<PathBuf>,
    catalog_rpc: PluginCatalogRpcHandler,
}

impl PluginHostHandler {
    pub fn new(
        workspace: Option<PathBuf>,
        catalog_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        Self {
            workspace,
            catalog_rpc,
        }
    }

    pub fn handle_notification(
        &mut self,
        method: String,
        params: Params,
    ) -> Result<()> {
        match method.as_str() {
            StartLspServer::METHOD => {
                let params: StartLspServerParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                let workspace = self.workspace.clone();
                let catalog_rpc = self.catalog_rpc.clone();
                thread::spawn(move || {
                    NewLspClient::start(
                        catalog_rpc,
                        workspace,
                        params.exec_path,
                        Vec::new(),
                    );
                });
            }
            _ => {}
        }
        Ok(())
    }
}
