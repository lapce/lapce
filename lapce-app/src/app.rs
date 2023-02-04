use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

use crossbeam_channel::Sender;
use floem::{
    app::AppContext,
    button::button,
    ext_event::{create_signal_from_channel, ExtEvent},
    label,
    reactive::{
        create_isomorphic_effect, create_resource, create_signal,
        create_signal_from_stream, ReadSignal,
    },
    stack::stack,
    style::{Dimension, FlexDirection, Style},
    view::View,
    Decorators,
};
use lapce_proxy::dispatch::Dispatcher;
use lapce_rpc::{
    core::{CoreHandler, CoreNotification, CoreRpcHandler},
    proxy::ProxyRpcHandler,
    source_control::DiffInfo,
};

fn title(cx: AppContext, proxy_data: &ProxyData) -> impl View {
    let connected = proxy_data.connected;
    let diff_info = proxy_data.diff_info;
    let head = move || diff_info.get().map(|info| info.head);
    stack(cx, move |cx| {
        (
            label(cx, move || head().unwrap_or_default()).style(cx, || Style {
                width: Dimension::Points(30.0),
                padding: 10.0,
                border: 1.0,
                ..Default::default()
            }),
            label(cx, move || {
                println!("connected got new value");
                if connected.get() {
                    "connected".to_string()
                } else {
                    "disconnected".to_string()
                }
            }),
        )
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        padding: 10.0,
        border_bottom: 20.0,
        ..Default::default()
    })
}

struct Proxy {
    tx: Sender<CoreNotification>,
}

struct ProxyData {
    connected: ReadSignal<bool>,
    diff_info: ReadSignal<Option<DiffInfo>>,
}

fn start_proxy(cx: AppContext) -> ProxyData {
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
        connected: proxy_connected,
        diff_info,
    };

    create_isomorphic_effect(cx.scope, move |_| {
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

fn workbench(cx: AppContext) -> impl View {
    let (couter, set_couter) = create_signal(cx.scope, 0);
    stack(cx, move |cx| {
        (
            label(cx, move || "main".to_string()),
            button(cx, || "".to_string(), || {}).style(cx, || Style {
                width: Dimension::Percent(1.0),
                height: Dimension::Auto,
                flex_grow: 1.0,
                border: 2.0,
                border_radius: 24.0,
                ..Default::default()
            }),
        )
    })
    .style(cx, || Style {
        width: floem::style::Dimension::Percent(1.0),
        height: floem::style::Dimension::Auto,
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}

fn status(cx: AppContext) -> impl View {
    label(cx, move || "status".to_string())
}

fn app_logic(cx: AppContext) -> impl View {
    let proxy_data = start_proxy(cx);
    stack(cx, move |cx| {
        (title(cx, &proxy_data), workbench(cx), status(cx))
    })
    .style(cx, || Style {
        width: Dimension::Percent(1.0),
        height: Dimension::Percent(1.0),
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}

pub fn launch() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                floem::launch(app_logic);
            })
            .await;
    });
}
