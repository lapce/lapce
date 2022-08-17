pub mod buffer;
pub mod core;
pub mod counter;
pub mod file;
pub mod lsp;
mod parse;
pub mod plugin;
pub mod proxy;
pub mod source_control;
pub mod stdio;
pub mod style;
pub mod terminal;

use std::collections::HashMap;
use std::io::stdin;
use std::io::stdout;
use std::io::BufReader;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
pub use parse::Call;
pub use parse::RequestId;
pub use parse::RpcObject;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serde_json::Value;

pub use stdio::stdio_transport;

pub enum RpcMessage<Req, Notif, Resp> {
    Request(RequestId, Req),
    Response(RequestId, Resp),
    Notification(Notif),
    Error(RequestId, RpcError),
}

impl<Req: Serialize, Notif: Serialize, Resp: Serialize>
    RpcMessage<Req, Notif, Resp>
{
    pub fn to_value(&self) -> Result<Value> {
        let value = match self {
            RpcMessage::Request(id, req) => {
                let mut msg = serde_json::to_value(req)?;
                msg.as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!(""))?
                    .insert("id".into(), (*id).into());
                msg
            }
            RpcMessage::Response(_, _) => todo!(),
            RpcMessage::Notification(_) => todo!(),
            RpcMessage::Error(_, _) => todo!(),
        };
        Ok(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

pub fn stdio<S, D>() -> (Sender<S>, Receiver<D>)
where
    S: 'static + Serialize + Send + Sync,
    D: 'static + DeserializeOwned + Send + Sync,
{
    let stdout = stdout();
    let stdin = BufReader::new(stdin());
    let (writer_sender, writer_receiver) = crossbeam_channel::unbounded();
    let (reader_sender, reader_receiver) = crossbeam_channel::unbounded();
    stdio::stdio_transport(stdout, writer_receiver, stdin, reader_sender);
    (writer_sender, reader_receiver)
}

pub trait Callback: Send {
    fn call(self: Box<Self>, result: Result<Value, Value>);
}

impl<F: Send + FnOnce(Result<Value, Value>)> Callback for F {
    fn call(self: Box<F>, result: Result<Value, Value>) {
        (*self)(result)
    }
}

pub trait NewCallback<Resp>: Send {
    fn call(self: Box<Self>, result: Result<Resp, RpcError>);
}

impl<Resp, F: Send + FnOnce(Result<Resp, RpcError>)> NewCallback<Resp> for F {
    fn call(self: Box<F>, result: Result<Resp, RpcError>) {
        (*self)(result)
    }
}

enum ResponseHandler {
    Chan(Sender<Result<Value, Value>>),
    Callback(Box<dyn Callback>),
}

impl ResponseHandler {
    fn invoke(self, result: Result<Value, Value>) {
        match self {
            ResponseHandler::Chan(tx) => {
                let _ = tx.send(result);
            }
            ResponseHandler::Callback(f) => f.call(result),
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum ControlFlow {
    Continue,
    Exit,
}

pub trait Handler {
    type Notification: DeserializeOwned;
    type Request: DeserializeOwned;

    fn handle_notification(&mut self, rpc: Self::Notification) -> ControlFlow;
    fn handle_request(&mut self, rpc: Self::Request) -> Result<Value, Value>;
}

pub trait NewHandler<Req, Notif, Resp> {
    fn handle_notification(&mut self, rpc: Notif);
    fn handle_request(&mut self, rpc: Req);
}

#[derive(Clone)]
pub struct RpcHandler {
    sender: Sender<Value>,
    id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, ResponseHandler>>>,
}

impl RpcHandler {
    pub fn new(sender: Sender<Value>) -> Self {
        Self {
            sender,
            id: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn mainloop<H>(&mut self, receiver: Receiver<Value>, handler: &mut H)
    where
        H: Handler,
    {
        for msg in receiver {
            let rpc: RpcObject = msg.into();
            if rpc.is_response() {
                let id = rpc.get_id().unwrap();
                match rpc.into_response() {
                    Ok(resp) => {
                        self.handle_response(id, resp);
                    }
                    Err(msg) => {
                        self.handle_response(id, Err(json!(msg)));
                    }
                }
            } else {
                match rpc.into_rpc::<H::Notification, H::Request>() {
                    Ok(Call::Request(id, request)) => {
                        let result = handler.handle_request(request);
                        self.respond(id, result);
                    }
                    Ok(Call::Notification(notification)) => {
                        if handler.handle_notification(notification)
                            == ControlFlow::Exit
                        {
                            return;
                        }
                    }
                    Err(_e) => {}
                }
            }
        }
    }

    pub fn send_rpc_notification(&self, method: &str, params: &Value) {
        if let Err(_e) = self.sender.send(json!({
            "method": method,
            "params": params,
        })) {}
    }

    fn send_rpc_request_common(
        &self,
        method: &str,
        params: &Value,
        rh: ResponseHandler,
    ) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, rh);
        }
        if let Err(_e) = self.sender.send(json!({
            "id": id,
            "method": method,
            "params": params,
        })) {
            let mut pending = self.pending.lock();
            if let Some(rh) = pending.remove(&id) {
                rh.invoke(Err(json!("io error")));
            }
        }
    }

    fn send_rpc_request_value_common<T: serde::Serialize>(
        &self,
        request: T,
        rh: ResponseHandler,
    ) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock();
            pending.insert(id, rh);
        }
        let mut request = serde_json::to_value(request).unwrap();
        request
            .as_object_mut()
            .unwrap()
            .insert("id".to_string(), json!(id));

        if let Err(_e) = self.sender.send(request) {
            let mut pending = self.pending.lock();
            if let Some(rh) = pending.remove(&id) {
                rh.invoke(Err(json!("io error")));
            }
        }
    }

    pub fn send_rpc_request_value<T: serde::Serialize>(
        &self,
        request: T,
    ) -> Result<Value, Value> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send_rpc_request_value_common(request, ResponseHandler::Chan(tx));
        rx.recv().unwrap_or_else(|_| Err(json!("io error")))
    }

    pub fn send_rpc_request_value_async<T: serde::Serialize>(
        &self,
        request: T,
        f: Box<dyn Callback>,
    ) {
        self.send_rpc_request_value_common(request, ResponseHandler::Callback(f));
    }

    pub fn send_rpc_request(
        &self,
        method: &str,
        params: &Value,
    ) -> Result<Value, Value> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send_rpc_request_common(method, params, ResponseHandler::Chan(tx));
        rx.recv().unwrap_or_else(|_| Err(json!("io error")))
    }

    pub fn send_rpc_request_async(
        &self,
        method: &str,
        params: &Value,
        f: Box<dyn Callback>,
    ) {
        self.send_rpc_request_common(method, params, ResponseHandler::Callback(f));
    }

    fn handle_response(&self, id: u64, resp: Result<Value, Value>) {
        let handler = {
            let mut pending = self.pending.lock();
            pending.remove(&id)
        };
        if let Some(responsehandler) = handler {
            responsehandler.invoke(resp)
        }
    }

    fn respond(&self, id: u64, result: Result<Value, Value>) {
        let mut response = json!({ "id": id });
        match result {
            Ok(result) => response["result"] = result,
            Err(error) => response["error"] = json!(error),
        };

        #[allow(deprecated)]
        let _ = self.sender.send(response);
    }
}
