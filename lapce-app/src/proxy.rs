use std::{collections::HashMap, path::PathBuf};

use crossbeam_channel::Sender;
use floem::{
    app::AppContext,
    ext_event::create_signal_from_channel,
    reactive::{create_effect, create_signal, ReadSignal},
};
use lapce_proxy::dispatch::Dispatcher;
use lapce_rpc::{
    core::{CoreHandler, CoreNotification, CoreRpcHandler},
    proxy::ProxyRpcHandler,
    source_control::DiffInfo,
};

pub struct Proxy {
    pub tx: Sender<CoreNotification>,
}

#[derive(Clone)]
pub struct ProxyData {
    pub rpc: ProxyRpcHandler,
    pub connected: ReadSignal<bool>,
    pub diff_info: ReadSignal<Option<DiffInfo>>,
}

pub fn start_proxy(cx: AppContext) -> ProxyData {
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
        Some(PathBuf::from("/Users/dz/lapce")),
        Vec::new(),
        HashMap::new(),
        1,
        1,
    );

    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || {
        let mut proxy = Proxy { tx };
        core_rpc.mainloop(&mut proxy);
    });

    let notification = create_signal_from_channel(cx, rx);

    let (proxy_connected, set_proxy_connected) = create_signal(cx.scope, false);
    let (diff_info, set_diff_info) = create_signal(cx.scope, None);

    let proxy_data = ProxyData {
        rpc: proxy_rpc,
        connected: proxy_connected,
        diff_info,
    };

    create_effect(cx.scope, move |_| {
        notification.with(|event| {
            if let Some(rpc) = event.as_ref() {
                match rpc {
                    CoreNotification::ProxyConnected {} => {
                        set_proxy_connected.update(|v| *v = true);
                    }
                    CoreNotification::DiffInfo { diff } => {
                        set_diff_info.set(Some(diff.clone()));
                    }
                    _ => {}
                }
            }
        });
    });

    proxy_data
}

impl CoreHandler for Proxy {
    fn handle_notification(&mut self, rpc: lapce_rpc::core::CoreNotification) {
        let result = self.tx.send(rpc);
    }

    fn handle_request(
        &mut self,
        id: lapce_rpc::RequestId,
        rpc: lapce_rpc::core::CoreRequest,
    ) {
        todo!()
    }
}
