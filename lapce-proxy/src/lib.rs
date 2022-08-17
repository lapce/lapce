pub mod buffer;
pub mod directory;
pub mod dispatch;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use std::{
    io::{stdin, stdout, BufRead, BufReader, Stdin, Stdout, Write},
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::Receiver;
use dispatch::NewDispatcher;
use jsonrpc_lite::JsonRpc;
use lapce_rpc::{
    core::{
        CoreHandler, CoreNotification, CoreRequest, CoreResponse, CoreRpc,
        CoreRpcHandler,
    },
    proxy::{
        CoreProxyNotification, CoreProxyRequest, CoreProxyResponse, ProxyRpcHandler,
    },
    stdio::new_stdio_transport,
    RequestId, RpcError, RpcMessage, RpcObject,
};
use serde_json::Value;

#[cfg(debug_assertions)]
pub const APPLICATION_NAME: &str = "Lapce-debug";

#[cfg(debug_assertions)]
pub const VERSION: &str = "debug";

#[cfg(not(debug_assertions))]
pub const APPLICATION_NAME: &str = "Lapce";

#[cfg(not(debug_assertions))]
pub const VERSION: &str = if env!("RELEASE_TAG_NAME") == "nightly" {
    "nightly"
} else {
    env!("CARGO_PKG_VERSION")
};

pub fn mainloop() {
    let core_rpc = CoreRpcHandler::new();
    let proxy_rpc = ProxyRpcHandler::new();
    let mut dispatcher = NewDispatcher::new(core_rpc.clone(), proxy_rpc.clone());

    let (writer_tx, writer_rx) = crossbeam_channel::unbounded();
    let (reader_tx, reader_rx) = crossbeam_channel::unbounded();
    new_stdio_transport(stdout(), writer_rx, BufReader::new(stdin()), reader_tx);

    let local_core_rpc = core_rpc.clone();
    let local_writer_tx = writer_tx.clone();
    thread::spawn(move || {
        for msg in local_core_rpc.rx() {
            match msg {
                CoreRpc::Request(id, rpc) => {
                    let _ = local_writer_tx.send(RpcMessage::Request(id, rpc));
                }
                CoreRpc::Notification(rpc) => {
                    let _ = local_writer_tx.send(RpcMessage::Notification(rpc));
                }
                CoreRpc::Shutdown => {
                    return;
                }
            }
        }
    });

    let local_proxy_rpc = proxy_rpc.clone();
    let writer_tx = Arc::new(writer_tx);
    thread::spawn(move || {
        for msg in reader_rx {
            match msg {
                RpcMessage::Request(id, req) => {
                    let writer_tx = writer_tx.clone();
                    local_proxy_rpc.request_async(req, move |result| match result {
                        Ok(resp) => {
                            let _ = writer_tx.send(RpcMessage::Response(id, resp));
                        }
                        Err(e) => {
                            let _ = writer_tx.send(RpcMessage::Error(id, e));
                        }
                    });
                }
                RpcMessage::Notification(n) => {
                    local_proxy_rpc.notification(n);
                }
                RpcMessage::Response(id, resp) => {
                    core_rpc.handle_response(id, Ok(resp));
                }
                RpcMessage::Error(id, err) => {
                    core_rpc.handle_response(id, Err(err));
                }
            }
        }
    });

    proxy_rpc.mainloop(&mut dispatcher);
}
