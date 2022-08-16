pub mod buffer;
pub mod directory;
pub mod dispatch;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use std::{
    io::{stdin, stdout, BufRead, BufReader, Stdin, Stdout},
    thread,
};

use anyhow::{anyhow, Result};
use dispatch::NewDispatcher;
use lapce_rpc::{
    core::{
        CoreHandler, CoreNotification, CoreRequest, CoreResponse, CoreRpcHandler,
    },
    proxy::{CoreProxyNotification, CoreProxyRequest, ProxyRpcHandler},
    RequestId, RpcMessage, RpcObject,
};
use serde_json::Value;

#[cfg(debug_assertions)]
pub const APPLICATION_NAME: &str = "Lapce-debug";

#[cfg(debug_assertions)]
pub const VERSION: &str = "nightly";

#[cfg(not(debug_assertions))]
pub const APPLICATION_NAME: &str = "Lapce";

#[cfg(not(debug_assertions))]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn mainloop() {
    // let (sender, receiver) = lapce_rpc::stdio();
    // let dispatcher = Dispatcher::new(sender);
    // let _ = dispatcher.mainloop(receiver);
}

struct CoreStdout {
    stdout: Stdout,
}

impl CoreStdout {
    fn new(stdout: Stdout) -> Self {
        Self { stdout }
    }
}

impl CoreHandler for CoreStdout {
    fn handle_notification(&mut self, rpc: CoreNotification) {
        todo!()
    }

    fn handle_request(&mut self, id: RequestId, rpc: CoreRequest) {
        todo!()
    }
}

pub fn new_mainloop() {
    let core_rpc = CoreRpcHandler::new();
    let proxy_rpc = ProxyRpcHandler::new();
    let mut dispatcher = NewDispatcher::new(core_rpc.clone(), proxy_rpc.clone());

    let mut core_stdout = CoreStdout::new(stdout());
    thread::spawn(move || {
        core_rpc.mainloop(&mut core_stdout);
    });

    let local_proxy_rpc = proxy_rpc.clone();
    thread::spawn(move || -> Result<()> {
        let mut reader = BufReader::new(stdin());
        loop {
            let msg = new_read_msg(&mut reader)?;
            match msg {
                RpcMessage::Request(id, request) => todo!(),
                RpcMessage::Notification(n) => {
                    local_proxy_rpc.notification(n);
                }
                RpcMessage::Response(_, _) => todo!(),
                RpcMessage::Error(_, _) => todo!(),
            }
        }
        // proxy_rpc.mainloop(&mut proxy_stdin);
    });

    proxy_rpc.mainloop(&mut dispatcher);
}

fn new_read_msg(
    reader: &mut BufReader<Stdin>,
) -> Result<RpcMessage<CoreProxyRequest, CoreProxyNotification, CoreResponse>> {
    let mut buf = String::new();
    let _s = reader.read_line(&mut buf)?;
    let value: Value = serde_json::from_str(&buf)?;
    let object = RpcObject(value);
    let is_response = object.is_response();
    let msg = if is_response {
        let id = object.get_id().ok_or_else(|| anyhow!("no id"))?;
        let resp = object.into_response().map_err(|e| anyhow!(e))?;
        match resp {
            Ok(value) => {
                todo!()
            }
            Err(value) => {
                todo!()
            }
        }
    } else {
        match object.get_id() {
            Some(id) => {
                let req: CoreProxyRequest = serde_json::from_value(object.0)?;
                RpcMessage::Request(id, req)
            }
            None => {
                let notif: CoreProxyNotification = serde_json::from_value(object.0)?;
                RpcMessage::Notification(notif)
            }
        }
    };
    Ok(msg)
}
