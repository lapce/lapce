use floem::{
    app::AppContext,
    ext_event::{create_ext_action, create_signal_from_channel_oneshot},
    reactive::{
        create_effect, create_rw_signal, create_signal, RwSignal, WriteSignal,
    },
};
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};

use self::{item::PaletteItem, kind::PaletteKind};

mod item;
mod kind;

#[derive(Clone, PartialEq, Eq)]
pub enum PaletteStatus {
    Inactive,
    Started,
    Done,
}

#[derive(Clone)]
pub struct PaletteData {
    pub status: WriteSignal<PaletteStatus>,
    pub kind: WriteSignal<PaletteKind>,
    pub items: RwSignal<Vec<PaletteItem>>,
    pub proxy_rpc: ProxyRpcHandler,
}

impl PaletteData {
    pub fn new(cx: AppContext, proxy_rpc: ProxyRpcHandler) -> Self {
        let (_status, status) = create_signal(cx.scope, PaletteStatus::Inactive);
        let (_kind, kind) = create_signal(cx.scope, PaletteKind::File);
        let items = create_rw_signal(cx.scope, Vec::new());
        Self {
            status,
            kind,
            items,
            proxy_rpc,
        }
    }

    pub fn run(&self, cx: AppContext, kind: PaletteKind) {
        match kind {
            PaletteKind::File => {
                self.get_files(cx);
            }
        }
        self.kind.set(kind);
    }

    fn get_files(&self, cx: AppContext) {
        self.items.write_only();
        let (items, send) = create_ext_action(cx);
        self.proxy_rpc.get_files(move |result| {
            if let Ok(ProxyResponse::GetFilesResponse { items }) = result {
                send(items);
            }
        });
    }
}
