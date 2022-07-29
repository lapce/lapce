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
use lapce_core::buffer::RopeText;
use lapce_rpc::RpcError;
use lsp_types::{
    notification::{DidChangeTextDocument, DidOpenTextDocument, Notification},
    DidChangeTextDocumentParams, Range, ServerCapabilities,
    TextDocumentContentChangeEvent, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, VersionedTextDocumentIdentifier,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xi_rope::{Rope, RopeDelta};

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
    DidChangeTextDocument {
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        change: Arc<
            Mutex<(
                Option<TextDocumentContentChangeEvent>,
                Option<TextDocumentContentChangeEvent>,
            )>,
        >,
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
    fn handle_did_change_text_document(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        rev: u64,
        delta: RopeDelta,
        text: Rope,
        change: Arc<
            Mutex<(
                Option<TextDocumentContentChangeEvent>,
                Option<TextDocumentContentChangeEvent>,
            )>,
        >,
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
                PluginServerRpc::DidChangeTextDocument {
                    document,
                    delta,
                    text,
                    change,
                } => {}
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
    server_rpc: PluginServerRpcHandler,
    server_capabilities: ServerCapabilities,
}

impl PluginHostHandler {
    pub fn new(
        workspace: Option<PathBuf>,
        server_rpc: PluginServerRpcHandler,
        catalog_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        Self {
            workspace,
            catalog_rpc,
            server_rpc,
            server_capabilities: ServerCapabilities::default(),
        }
    }

    pub fn method_registered(&mut self, method: &'static str) -> bool {
        match method {
            DidOpenTextDocument::METHOD => {
                match &self.server_capabilities.text_document_sync {
                    Some(TextDocumentSyncCapability::Kind(kind)) => {
                        kind != &TextDocumentSyncKind::NONE
                    }
                    Some(TextDocumentSyncCapability::Options(options)) => options
                        .open_close
                        .or_else(|| {
                            options
                                .change
                                .map(|kind| kind != TextDocumentSyncKind::NONE)
                        })
                        .unwrap_or(false),
                    None => false,
                }
            }
            _ => false,
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

    pub fn handle_did_change_text_document(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        rev: u64,
        delta: RopeDelta,
        text: Rope,
        change: Arc<
            Mutex<(
                Option<TextDocumentContentChangeEvent>,
                Option<TextDocumentContentChangeEvent>,
            )>,
        >,
    ) {
        let kind = match &self.server_capabilities.text_document_sync {
            Some(TextDocumentSyncCapability::Kind(kind)) => *kind,
            Some(TextDocumentSyncCapability::Options(options)) => {
                options.change.unwrap_or(TextDocumentSyncKind::NONE)
            }
            None => TextDocumentSyncKind::NONE,
        };

        let mut existing = change.lock();
        let change = match kind {
            TextDocumentSyncKind::FULL => {
                if let Some(c) = existing.0.as_ref() {
                    c.clone()
                } else {
                    let text = text.to_string();
                    let change = TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text,
                    };
                    existing.0 = Some(change.clone());
                    change
                }
            }
            TextDocumentSyncKind::INCREMENTAL => {
                if let Some(c) = existing.1.as_ref() {
                    c.clone()
                } else {
                    let change = get_document_content_change(&text, &delta)
                        .unwrap_or_else(|| TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: text.to_string(),
                        });
                    existing.1 = Some(change.clone());
                    change
                }
            }
            TextDocumentSyncKind::NONE => return,
            _ => return,
        };

        let params = DidChangeTextDocumentParams {
            text_document: document,
            content_changes: vec![change],
        };

        self.server_rpc
            .server_notification(DidChangeTextDocument::METHOD, params);
    }
}

fn get_document_content_change(
    text: &Rope,
    delta: &RopeDelta,
) -> Option<TextDocumentContentChangeEvent> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    let text = RopeText::new(text);

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let (start, end) = interval.start_end();
        let start = if let Some(start) = text.offset_to_position(start) {
            start
        } else {
            log::error!("Failed to convert start offset to Position in document content change insert");
            return None;
        };

        let end = if let Some(end) = text.offset_to_position(end) {
            end
        } else {
            log::error!("Failed to convert end offset to Position in document content change insert");
            return None;
        };

        let text = String::from(node);
        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text,
        };

        return Some(text_document_content_change_event);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let end_position = if let Some(end) = text.offset_to_position(end) {
            end
        } else {
            log::error!("Failed to convert end offset to Position in document content change delete");
            return None;
        };

        let start = if let Some(start) = text.offset_to_position(start) {
            start
        } else {
            log::error!("Failed to convert start offset to Position in document content change delete");
            return None;
        };

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start,
                end: end_position,
            }),
            range_length: None,
            text: String::new(),
        };

        return Some(text_document_content_change_event);
    }

    None
}
