use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    sync::{Arc, mpsc::Sender},
};

use floem::{ext_event::create_signal_from_channel, reactive::ReadSignal};
use lapce_proxy::dispatch::Dispatcher;
use lapce_rpc::{
    core::{CoreHandler, CoreNotification, CoreRpcHandler},
    plugin::VoltID,
    proxy::{ProxyRpcHandler, ProxyStatus},
    terminal::TermId,
};
use tracing::error;

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

pub fn new_proxy(
    workspace: Arc<LapceWorkspace>,
    disabled_volts: Vec<VoltID>,
    extra_plugin_paths: Vec<PathBuf>,
    plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
    term_tx: Sender<(TermId, TermEvent)>,
) -> ProxyData {
    let proxy_rpc = ProxyRpcHandler::new();
    let core_rpc = CoreRpcHandler::new();

    {
        let core_rpc = core_rpc.clone();
        let proxy_rpc = proxy_rpc.clone();
        std::thread::Builder::new()
            .name("ProxyRpcHandler".to_owned())
            .spawn(move || {
                core_rpc.notification(CoreNotification::ProxyStatus {
                    status: ProxyStatus::Connecting,
                });
                proxy_rpc.initialize(
                    workspace.path.clone(),
                    disabled_volts,
                    extra_plugin_paths,
                    plugin_configurations,
                    1,
                    1,
                );

                match &workspace.kind {
                    LapceWorkspaceType::Local => {
                        let core_rpc = core_rpc.clone();
                        let proxy_rpc = proxy_rpc.clone();
                        let mut dispatcher = Dispatcher::new(core_rpc, proxy_rpc);
                        let proxy_rpc = dispatcher.proxy_rpc.clone();
                        proxy_rpc.mainloop(&mut dispatcher);
                    }
                    LapceWorkspaceType::RemoteSSH(remote) => {
                        if let Err(e) = start_remote(
                            SshRemote {
                                ssh: remote.clone(),
                            },
                            core_rpc.clone(),
                            proxy_rpc.clone(),
                        ) {
                            error!("Failed to start SSH remote: {e}");
                        }
                    }
                    #[cfg(windows)]
                    LapceWorkspaceType::RemoteWSL(remote) => {
                        if let Err(e) = start_remote(
                            wsl::WslRemote {
                                wsl: remote.clone(),
                            },
                            core_rpc.clone(),
                            proxy_rpc.clone(),
                        ) {
                            error!("Failed to start SSH remote: {e}");
                        }
                    }
                }
                core_rpc.notification(CoreNotification::ProxyStatus {
                    status: ProxyStatus::Disconnected,
                });
            })
            .unwrap();
    }

    let (tx, rx) = std::sync::mpsc::channel();
    {
        let core_rpc = core_rpc.clone();
        std::thread::Builder::new()
            .name("CoreRpcHandler".to_owned())
            .spawn(move || {
                let mut proxy = Proxy { tx, term_tx };
                core_rpc.mainloop(&mut proxy);
                core_rpc.notification(CoreNotification::ProxyStatus {
                    status: ProxyStatus::Disconnected,
                });
            })
            .unwrap()
    };

    let notification = create_signal_from_channel(rx);

    ProxyData {
        proxy_rpc,
        core_rpc,
        notification,
    }
}

impl CoreHandler for Proxy {
    fn handle_notification(&mut self, rpc: lapce_rpc::core::CoreNotification) {
        if let CoreNotification::UpdateTerminal { term_id, content } = &rpc {
            if let Err(err) = self
                .term_tx
                .send((*term_id, TermEvent::UpdateContent(content.to_vec())))
            {
                tracing::error!("{:?}", err);
            }
            return;
        }
        if let Err(err) = self.tx.send(rpc) {
            tracing::error!("{:?}", err);
        }
    }

    fn handle_request(
        &mut self,
        _id: lapce_rpc::RequestId,
        _rpc: lapce_rpc::core::CoreRequest,
    ) {
    }
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
