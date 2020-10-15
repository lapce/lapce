use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};

use crate::xi_thread::XiPeer;

#[derive(Clone)]
pub struct Core {
    state: Arc<Mutex<CoreState>>,
}

struct CoreState {
    xi_peer: XiPeer,
    id: u64,
    pending: BTreeMap<u64, Box<Callback>>,
}

trait Callback: Send + Sync {
    fn call(self: Box<Self>, result: &Value);
}

pub trait Handler {
    fn notification(&self, method: &str, params: &Value);
}

impl<F: FnOnce(&Value) + Send + Sync> Callback for F {
    fn call(self: Box<F>, result: &Value) {
        (*self)(result)
    }
}

impl Core {
    /// Sets up a new RPC connection, also starting a thread to receive
    /// responses.
    ///
    /// The handler is invoked for incoming RPC notifications. Note that
    /// it must be `Send` because it is called from a dedicated thread.
    pub fn new<H>(xi_peer: XiPeer, rx: Receiver<Value>, handler: H) -> Core
    where
        H: Handler + Send + 'static,
    {
        let state = CoreState {
            xi_peer,
            id: 0,
            pending: BTreeMap::new(),
        };
        let core = Core {
            state: Arc::new(Mutex::new(state)),
        };
        let rx_core_handle = core.clone();
        thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                if let Value::String(ref method) = msg["method"] {
                    handler.notification(&method, &msg["params"]);
                } else if let Some(id) = msg["id"].as_u64() {
                    let mut state = rx_core_handle.state.lock().unwrap();
                    if let Some(callback) = state.pending.remove(&id) {
                        callback.call(&msg["result"]);
                    } else {
                        println!("unexpected result")
                    }
                } else {
                    println!("got {:?} at rpc level", msg);
                }
            }
        });
        core
    }

    pub fn no_move(&self, view_id: &str, is_selection: bool, line_selection: bool, caret: bool) {
        self.send_notification(
            "edit",
            &json!({
                "view_id": view_id,
                "method": "no_move",
                "params": {"is_selection": is_selection, "line_selection": line_selection, "caret": caret},
            }),
        );
    }

    pub fn send_notification(&self, method: &str, params: &Value) {
        let cmd = json!({
            "method": method,
            "params": params,
        });
        let state = self.state.lock().unwrap();
        state.xi_peer.send_json(&cmd);
    }

    /// Calls the callback with the result (from a different thread).
    pub fn send_request<F>(&self, method: &str, params: &Value, callback: F)
    where
        F: FnOnce(&Value) + Send + Sync + 'static,
    {
        let mut state = self.state.lock().unwrap();
        let id = state.id;
        let cmd = json!({
            "method": method,
            "params": params,
            "id": id,
        });
        state.xi_peer.send_json(&cmd);
        state.pending.insert(id, Box::new(callback));
        state.id += 1;
    }
}
