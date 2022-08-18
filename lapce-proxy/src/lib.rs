pub mod buffer;
pub mod directory;
pub mod dispatch;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use std::{
    io::{stdin, stdout, BufReader},
    sync::Arc,
    thread,
};

use dispatch::Dispatcher;
use lapce_rpc::{
    core::{CoreRpc, CoreRpcHandler},
    proxy::ProxyRpcHandler,
    stdio::stdio_transport,
    RpcMessage,
};
use once_cell::sync::Lazy;

#[cfg(debug_assertions)]
pub const APPLICATION_NAME: &str = "Lapce-debug";

#[cfg(not(debug_assertions))]
pub const APPLICATION_NAME: &str = "Lapce";

pub static VERSION: Lazy<&str> = Lazy::new(version);

fn version() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else if option_env!("RELEASE_TAG_NAME").is_some()
        && option_env!("RELEASE_TAG_NAME")
            .unwrap()
            .starts_with("nightly")
    {
        option_env!("RELEASE_TAG_NAME").unwrap()
    } else {
        env!("CARGO_PKG_VERSION")
    }
}

pub fn mainloop() {
    let core_rpc = CoreRpcHandler::new();
    let proxy_rpc = ProxyRpcHandler::new();
    let mut dispatcher = Dispatcher::new(core_rpc.clone(), proxy_rpc.clone());

    let (writer_tx, writer_rx) = crossbeam_channel::unbounded();
    let (reader_tx, reader_rx) = crossbeam_channel::unbounded();
    stdio_transport(stdout(), writer_rx, BufReader::new(stdin()), reader_tx);

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
