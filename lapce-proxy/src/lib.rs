pub mod buffer;
pub mod dispatch;
pub mod lsp;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use std::{
    io::{stdin, stdout, BufRead, BufReader, Stdin, Stdout, Write},
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use dispatch::{Dispatcher, NewDispatcher};
use lapce_rpc::{
    core::{
        CoreHandler, CoreNotification, CoreRequest, CoreRpcHandler, CoreRpcMessage,
    },
    proxy::{
        CoreProxyNotification, CoreProxyRequest, CoreProxyRpcMessage, ProxyHandler,
        ProxyRpcHandler, ProxyRpcMessage,
    },
    RpcMessage, RpcObject,
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
    let (sender, receiver) = lapce_rpc::stdio();
    let dispatcher = Dispatcher::new(sender);
    let _ = dispatcher.mainloop(receiver);
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

    fn handle_request(&mut self, rpc: CoreRequest) {
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
                RpcMessage::Request(_, _) => todo!(),
                RpcMessage::Notification(n) => {
                    local_proxy_rpc.send_notification(n);
                }
                RpcMessage::Response(_, _) => todo!(),
                RpcMessage::Error(_, _) => todo!(),
            }
        }
        // proxy_rpc.mainloop(&mut proxy_stdin);
    });

    proxy_rpc.mainloop(&mut dispatcher);
}

fn new_read_msg(reader: &mut BufReader<Stdin>) -> Result<CoreProxyRpcMessage> {
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

fn stdio() -> (
    Sender<CoreRpcMessage>,
    Sender<ProxyRpcMessage>,
    Receiver<ProxyRpcMessage>,
) {
    let mut stdout = stdout();
    let mut stdin = BufReader::new(stdin());

    let (core_sender, core_receiver) = crossbeam_channel::unbounded();
    let (proxy_sender, proxy_receiver) = crossbeam_channel::unbounded();

    thread::spawn(move || {
        for msg in core_receiver {
            write_msg(&mut stdout, msg);
        }
    });

    let local_proxy_sender = proxy_sender.clone();
    thread::spawn(move || -> Result<()> {
        loop {
            let msg = read_msg(&mut stdin)?;
            local_proxy_sender.send(msg)?;
        }
    });

    (core_sender, proxy_sender, proxy_receiver)
}

fn write_msg(out: &mut Stdout, msg: CoreRpcMessage) -> Result<()> {
    let value = match msg {
        CoreRpcMessage::Request(id, req) => {
            let mut msg = serde_json::to_value(&req)?;
            msg.as_object_mut()
                .ok_or_else(|| anyhow!(""))?
                .insert("id".into(), id.into());
            msg
        }
        CoreRpcMessage::Response(_, _) => todo!(),
        CoreRpcMessage::Notification(_) => todo!(),
        CoreRpcMessage::Error(_, _) => todo!(),
    };
    let msg = format!("{}\n", serde_json::to_string(&value)?);
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    Ok(())
}

fn read_msg(reader: &mut BufReader<Stdin>) -> Result<ProxyRpcMessage> {
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
                ProxyRpcMessage::Core(RpcMessage::Request(id, req))
            }
            None => {
                let notif: CoreProxyNotification = serde_json::from_value(object.0)?;
                ProxyRpcMessage::Core(RpcMessage::Notification(notif))
            }
        }
    };
    Ok(msg)
}
