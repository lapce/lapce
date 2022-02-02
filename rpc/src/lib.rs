mod parse;
mod stdio;

use std::collections::HashMap;
use std::io::stdin;
use std::io::stdout;
use std::io::BufReader;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use jsonrpc_lite::JsonRpc;
use parking_lot::Mutex;
pub use parse::Call;
pub use parse::RequestId;
pub use parse::RpcObject;
use serde::de::DeserializeOwned;
use serde_json::json;
use serde_json::Value;
use stdio::IoThreads;

pub use stdio::stdio_transport;

pub fn stdio() -> (Sender<Value>, Receiver<Value>) {
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

#[derive(PartialEq)]
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
                    Err(e) => {}
                }
            }
        }
    }

    pub fn send_rpc_notification(&self, method: &str, params: &Value) {
        if let Err(e) = self.sender.send(json!({
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
        if let Err(e) = self.sender.send(json!({
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

    pub fn send_rpc_request(
        &self,
        method: &str,
        params: &Value,
    ) -> Result<Value, Value> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send_rpc_request_common(method, params, ResponseHandler::Chan(tx));
        rx.recv().unwrap_or(Err(json!("io error")))
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
        match handler {
            Some(responsehandler) => responsehandler.invoke(resp),
            None => (),
        }
    }

    fn respond(&self, id: u64, result: Result<Value, Value>) {
        let mut response = json!({ "id": id });
        match result {
            Ok(result) => response["result"] = result,
            Err(error) => response["error"] = json!(error),
        };
        self.sender.send(response);
    }
}
