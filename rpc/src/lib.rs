mod parse;
mod stdio;

use crossbeam_channel::{Receiver, Sender};
use jsonrpc_lite::JsonRpc;
pub use parse::Call;
pub use parse::RequestId;
pub use parse::RpcObject;
use serde_json::Value;
use stdio::IoThreads;

pub fn stdio() -> (Sender<Value>, Receiver<Value>, IoThreads) {
    stdio::stdio_transport()
}
