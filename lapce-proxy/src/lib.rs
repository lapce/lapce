pub mod buffer;
pub mod directory;
pub mod dispatch;
pub mod plugin;
pub mod terminal;
pub mod watcher;

use std::{
    io::{stdin, stdout, BufReader},
    path::PathBuf,
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use clap::Parser;
use directory::Directory;
use dispatch::Dispatcher;
use lapce_rpc::{
    core::{CoreRpc, CoreRpcHandler},
    proxy::{ProxyNotification, ProxyRequest, ProxyResponse, ProxyRpcHandler},
    stdio::stdio_transport,
    RpcMessage,
};
use once_cell::sync::Lazy;

pub static APPLICATION_NAME: Lazy<&str> = Lazy::new(application_name);

fn application_name() -> &'static str {
    if cfg!(debug_assertions) {
        "Lapce-Debug"
    } else if option_env!("RELEASE_TAG_NAME")
        .unwrap_or("")
        .starts_with("nightly")
    {
        "Lapce-Nightly"
    } else {
        "Lapce-Stable"
    }
}

pub static VERSION: Lazy<&str> = Lazy::new(version);

fn version() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else if option_env!("RELEASE_TAG_NAME")
        .unwrap_or("")
        .starts_with("nightly")
    {
        option_env!("RELEASE_TAG_NAME").unwrap()
    } else {
        env!("CARGO_PKG_VERSION")
    }
}

#[derive(Parser)]
#[clap(name = "Lapce")]
#[clap(version=*VERSION)]
struct Cli {
    #[clap(short, long, action)]
    proxy: bool,
    paths: Vec<PathBuf>,
}

pub fn mainloop() {
    let cli = Cli::parse();
    if !cli.proxy {
        let pwd = std::env::current_dir().unwrap_or_default();
        let paths: Vec<PathBuf> = cli.paths.iter().map(|p| pwd.join(p)).collect();
        let _ = check_local_socket(paths);
        return;
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
    });

    let local_proxy_rpc = proxy_rpc.clone();
    std::thread::spawn(move || {
        let _ = listen_local_socket(local_proxy_rpc);
    });
    if let Some(path) = process_path::get_executable_path() {
        if let Some(path) = path.parent() {
            if let Some(path) = path.to_str() {
                if let Ok(current_path) = std::env::var("PATH") {
                    std::env::set_var("PATH", &format!("{path}:{current_path}"));
                }
            }
        }
    }

    proxy_rpc.mainloop(&mut dispatcher);
}

fn check_local_socket(paths: Vec<PathBuf>) -> Result<()> {
    let local_socket = Directory::local_socket()
        .ok_or_else(|| anyhow!("can't get local socket folder"))?;
    let mut socket =
        interprocess::local_socket::LocalSocketStream::connect(local_socket)?;
    let folders: Vec<PathBuf> =
        paths.clone().into_iter().filter(|p| p.is_dir()).collect();
    let files: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
    let msg: RpcMessage<ProxyRequest, ProxyNotification, ProxyResponse> =
        RpcMessage::Notification(ProxyNotification::OpenPaths { folders, files });
    lapce_rpc::stdio::write_msg(&mut socket, msg)?;
    Ok(())
}

pub(crate) fn listen_local_socket(proxy_rpc: ProxyRpcHandler) -> Result<()> {
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
                let msg: RpcMessage<ProxyRequest, ProxyNotification, ProxyResponse> =
                    lapce_rpc::stdio::read_msg(&mut reader)?;
                if let RpcMessage::Notification(ProxyNotification::OpenPaths {
                    folders,
                    files,
                }) = msg
                {
                    proxy_rpc.notification(ProxyNotification::OpenPaths {
                        folders,
                        files,
                    });
                }
            }
        });
    }
    Ok(())
}
