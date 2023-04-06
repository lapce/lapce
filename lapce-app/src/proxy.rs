use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};

use crossbeam_channel::Sender;
use floem::{
    app::AppContext,
    ext_event::create_signal_from_channel,
    reactive::{
        create_effect, create_signal, ReadSignal, SignalSet, SignalUpdate,
        SignalWith, WriteSignal,
    },
};
use lapce_proxy::dispatch::Dispatcher;
use lapce_rpc::plugin::VoltID;
use lapce_rpc::terminal::TermId;
use lapce_rpc::{
    core::{CoreHandler, CoreNotification, CoreRpcHandler},
    proxy::ProxyRpcHandler,
    source_control::DiffInfo,
};
use lsp_types::Url;

use crate::terminal::event::TermEvent;
use crate::{completion::CompletionData, workspace::LapceWorkspace};

pub struct Proxy {
    pub tx: Sender<CoreNotification>,
    pub term_tx: Sender<(TermId, TermEvent)>,
}

#[derive(Clone)]
pub struct ProxyData {
    pub rpc: ProxyRpcHandler,
    pub notification: ReadSignal<Option<CoreNotification>>,
}

pub fn start_proxy(
    cx: AppContext,
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
            let mut dispatcher = Dispatcher::new(core_rpc, proxy_rpc);
            let proxy_rpc = dispatcher.proxy_rpc.clone();
            proxy_rpc.mainloop(&mut dispatcher);
        });
    }

    proxy_rpc.initialize(
        workspace.path.clone(),
        disabled_volts,
        plugin_configurations,
        1,
        1,
    );

    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || {
        let mut proxy = Proxy { tx, term_tx };
        core_rpc.mainloop(&mut proxy);
    });

    let notification = create_signal_from_channel(cx, rx);

    ProxyData {
        rpc: proxy_rpc,
        notification,
    }
}

impl CoreHandler for Proxy {
    fn handle_notification(&mut self, rpc: lapce_rpc::core::CoreNotification) {
        match &rpc {
            CoreNotification::UpdateTerminal { term_id, content } => {
                let _ = self
                    .term_tx
                    .send((*term_id, TermEvent::UpdateContent(content.to_vec())));
                return;
            }
            CoreNotification::TerminalProcessStopped { term_id } => {
                let _ = self.term_tx.send((*term_id, TermEvent::CloseTerminal));
                return;
            }
            _ => {}
        }
        let result = self.tx.send(rpc);
    }

    fn handle_request(
        &mut self,
        id: lapce_rpc::RequestId,
        rpc: lapce_rpc::core::CoreRequest,
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
