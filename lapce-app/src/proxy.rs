use std::{collections::HashMap, path::PathBuf, process::Command, sync::Arc};

use crossbeam_channel::Sender;
use floem::{
    ext_event::create_signal_from_channel,
    reactive::{ReadSignal, Scope},
};
use lapce_proxy::dispatch::Dispatcher;
use lapce_rpc::{
    core::{CoreHandler, CoreNotification, CoreRpcHandler},
    plugin::VoltID,
    proxy::ProxyRpcHandler,
    terminal::TermId,
};
use lsp_types::Url;

use self::{remote::start_remote, ssh::SshRemote};
use crate::{
    terminal::event::TermEvent,
    workspace::{LapceWorkspace, LapceWorkspaceType},
};

mod remote;
mod ssh;
#[cfg(windows)]
mod wsl;

pub struct Proxy {
    pub tx: Sender<CoreNotification>,
    pub term_tx: Sender<(TermId, TermEvent)>,
}

#[derive(Clone)]
pub struct ProxyData {
    pub proxy_rpc: ProxyRpcHandler,
    pub core_rpc: CoreRpcHandler,
    pub notification: ReadSignal<Option<CoreNotification>>,
}

impl ProxyData {
    pub fn shutdown(&self) {
        self.proxy_rpc.shutdown();
        self.core_rpc.shutdown();
    }
}

pub fn start_proxy(
    cx: Scope,
    workspace: Arc<LapceWorkspace>,
    disabled_volts: Vec<VoltID>,
    plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
    term_tx: Sender<(TermId, TermEvent)>,
) -> ProxyData {
    let proxy_rpc = ProxyRpcHandler::new();
    let core_rpc = CoreRpcHandler::new();

    {
        let core_rpc = core_rpc.clone();
        let proxy_rpc = proxy_rpc.clone();
        std::thread::spawn(move || {
            proxy_rpc.initialize(
                workspace.path.clone(),
                disabled_volts,
                plugin_configurations,
                1,
                1,
            );

            match &workspace.kind {
                LapceWorkspaceType::Local => {
                    let core_rpc = core_rpc.clone();
                    let proxy_rpc = proxy_rpc.clone();
                    std::thread::spawn(move || {
                        let mut dispatcher = Dispatcher::new(core_rpc, proxy_rpc);
                        let proxy_rpc = dispatcher.proxy_rpc.clone();
                        proxy_rpc.mainloop(&mut dispatcher);
                    });
                }
                LapceWorkspaceType::RemoteSSH(ssh) => {
                    let _ = start_remote(
                        SshRemote { ssh: ssh.clone() },
                        core_rpc.clone(),
                        proxy_rpc.clone(),
                    );
                }
                #[cfg(windows)]
                LapceWorkspaceType::RemoteWSL => {
                    use wsl::{WslDistro, WslRemote};
                    let distro = WslDistro::all()
                        .ok()
                        .and_then(|d| d.into_iter().find(|distro| distro.default))
                        .map(|d| d.name);
                    if let Some(distro) = distro {
                        let _ = start_remote(
                            WslRemote { distro },
                            core_rpc.clone(),
                            proxy_rpc.clone(),
                        );
                    }
                }
            }
        });
    }

    let (tx, rx) = crossbeam_channel::unbounded();
    {
        let core_rpc = core_rpc.clone();
        std::thread::spawn(move || {
            let mut proxy = Proxy { tx, term_tx };
            core_rpc.mainloop(&mut proxy);
        })
    };

    let notification = create_signal_from_channel(cx, rx);

    ProxyData {
        proxy_rpc,
        core_rpc,
        notification,
    }
}

impl CoreHandler for Proxy {
    fn handle_notification(&mut self, rpc: lapce_rpc::core::CoreNotification) {
        if let CoreNotification::UpdateTerminal { term_id, content } = &rpc {
            let _ = self
                .term_tx
                .send((*term_id, TermEvent::UpdateContent(content.to_vec())));
            return;
        }
        let _ = self.tx.send(rpc);
    }

    fn handle_request(
        &mut self,
        _id: lapce_rpc::RequestId,
        _rpc: lapce_rpc::core::CoreRequest,
    ) {
    }
}

// Rust-analyzer returns paths in the form of "file:///<drive>:/...", which gets parsed into URL
// as "/<drive>://" which is then interpreted by PathBuf::new() as a UNIX-like path from root.
// This function strips the additional / from the beginning, if the first segment is a drive letter.
#[cfg(windows)]
pub fn path_from_url(url: &Url) -> PathBuf {
    let path = url.path();
    if let Some(path) = path.strip_prefix('/') {
        if let Some((maybe_drive_letter, _)) = path.split_once(['/', '\\']) {
            let b = maybe_drive_letter.as_bytes();
            if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from(path)
}

#[cfg(not(windows))]
pub fn path_from_url(url: &Url) -> PathBuf {
    url.to_file_path()
        .unwrap_or_else(|_| PathBuf::from(url.path()))
}

pub fn new_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd
}
