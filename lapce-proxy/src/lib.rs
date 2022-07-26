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
    core::CoreRpcMessage,
    proxy::{CoreProxyNotification, CoreProxyRequest, ProxyRpcMessage},
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

pub fn new_mainloop() {
    let (core_sender, proxy_sender, proxy_receiver) = stdio();
    let mut dispatcher = NewDispatcher::new(core_sender, proxy_sender);
    dispatcher.mainloop(proxy_receiver);
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
