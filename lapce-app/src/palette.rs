use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    ext_event::{create_ext_action, create_signal_from_channel_oneshot},
    reactive::{
        create_effect, create_rw_signal, create_signal, RwSignal, WriteSignal,
    },
};
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};

use crate::workspace::LapceWorkspace;

use self::{
    item::{PaletteItem, PaletteItemContent},
    kind::PaletteKind,
};

pub mod item;
pub mod kind;

#[derive(Clone, PartialEq, Eq)]
pub enum PaletteStatus {
    Inactive,
    Started,
    Done,
}

#[derive(Clone)]
pub struct PaletteData {
    pub workspace: Arc<LapceWorkspace>,
    pub status: RwSignal<PaletteStatus>,
    pub kind: RwSignal<PaletteKind>,
    pub items: RwSignal<Vec<PaletteItem>>,
    pub proxy_rpc: ProxyRpcHandler,
}

impl PaletteData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        proxy_rpc: ProxyRpcHandler,
    ) -> Self {
        let status = create_rw_signal(cx.scope, PaletteStatus::Inactive);
        let kind = create_rw_signal(cx.scope, PaletteKind::File);
        let items = create_rw_signal(cx.scope, Vec::new());
        Self {
            workspace,
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
        let workspace = self.workspace.clone();
        let set_items = self.items.write_only();
        let send = create_ext_action(cx, move |items: Vec<PathBuf>| {
            let items = items
                .into_iter()
                .map(|path| {
                    let full_path = path.clone();
                    let mut path = path;
                    if let Some(workspace_path) = workspace.path.as_ref() {
                        path = path
                            .strip_prefix(workspace_path)
                            .unwrap_or(&full_path)
                            .to_path_buf();
                    }
                    let filter_text = path.to_str().unwrap_or("").to_string();
                    PaletteItem {
                        content: PaletteItemContent::File { path, full_path },
                        filter_text,
                        score: 0,
                        indices: Vec::new(),
                    }
                })
                .collect::<Vec<_>>();
            println!("get files set items");
            set_items.set(items);
        });
        self.proxy_rpc.get_files(move |result| {
            if let Ok(ProxyResponse::GetFilesResponse { items }) = result {
                send(items);
            }
        });
    }
}
