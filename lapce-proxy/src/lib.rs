#![allow(clippy::manual_clamp)]

pub mod buffer;
pub mod cli;
pub mod dispatch;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use std::{
    io::{stdin, stdout, BufReader},
    path::PathBuf,
    process::exit,
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use clap::Parser;
use dispatch::Dispatcher;
use lapce_core::{directory::Directory, meta};
use lapce_rpc::{
    core::{CoreRpc, CoreRpcHandler},
    file::PathObject,
    proxy::{ProxyMessage, ProxyNotification, ProxyRpcHandler},
    stdio::stdio_transport,
    RpcMessage,
};

#[derive(Parser)]
#[clap(name = "Lapce-proxy")]
#[clap(version = meta::VERSION)]
struct Cli {
    #[clap(short, long, action, hide = true)]
    proxy: bool,

    /// Paths to file(s) and/or folder(s) to open.
    /// When path is a file (that exists or not),
    /// it accepts `path:line:column` syntax
    /// to specify line and column at which it should open the file
    #[clap(value_parser = cli::parse_file_line_column)]
    #[clap(value_hint = clap::ValueHint::AnyPath)]
    paths: Vec<PathObject>,
}

pub fn mainloop() {
    let cli = Cli::parse();
    if !cli.proxy {
        if let Err(e) = cli::try_open_in_existing_process(&cli.paths) {
            log::error!("failed to open path(s): {e}");
        };
        exit(1);
    }
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
        local_proxy_rpc.shutdown();
    });

    let local_proxy_rpc = proxy_rpc.clone();
    std::thread::spawn(move || {
        let _ = listen_local_socket(local_proxy_rpc);
    });
    let _ = register_lapce_path();

    proxy_rpc.mainloop(&mut dispatcher);
}

pub fn register_lapce_path() -> Result<()> {
    let path = std::env::current_exe()?;

    if let Some(path) = path.parent() {
        if let Some(path) = path.to_str() {
            if let Ok(current_path) = std::env::var("PATH") {
                let mut paths = vec![PathBuf::from(path)];
                paths.append(
                    &mut std::env::split_paths(&current_path).collect::<Vec<_>>(),
                );
                std::env::set_var("PATH", std::env::join_paths(paths)?);
            }
        }
    }

    Ok(())
}

fn listen_local_socket(proxy_rpc: ProxyRpcHandler) -> Result<()> {
    let local_socket = Directory::local_socket()
        .ok_or_else(|| anyhow!("can't get local socket folder"))?;
    let _ = std::fs::remove_file(&local_socket);
    let socket =
        interprocess::local_socket::LocalSocketListener::bind(local_socket)?;
    for stream in socket.incoming().flatten() {
        let mut reader = BufReader::new(stream);
        let proxy_rpc = proxy_rpc.clone();
        thread::spawn(move || -> Result<()> {
            loop {
                let msg: ProxyMessage = lapce_rpc::stdio::read_msg(&mut reader)?;
                if let RpcMessage::Notification(ProxyNotification::OpenPaths {
                    paths,
                }) = msg
                {
                    proxy_rpc.notification(ProxyNotification::OpenPaths { paths });
                }
            }
        });
    }
    Ok(())
}
