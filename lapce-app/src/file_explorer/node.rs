use std::path::PathBuf;

use floem::{
    ext_event::create_ext_action,
    reactive::{create_rw_signal, RwSignal, Scope, SignalGetUntracked, SignalSet},
};
use indexmap::IndexMap;
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};

#[derive(Clone)]
pub struct FileNode {
    pub scope: Scope,
    pub path: PathBuf,
    pub is_dir: bool,
    pub read: RwSignal<bool>,
    pub expanded: RwSignal<bool>,
    pub children: RwSignal<IndexMap<PathBuf, FileNode>>,
}

impl FileNode {
    pub fn toggle_expand(&self, proxy: &ProxyRpcHandler) {
        let expanded = self.expanded.get_untracked();
        if expanded {
            self.expanded.set(false);
        } else {
            self.expanded.set(true);
            self.read_dir(proxy);
        }
    }

    fn read_dir(&self, proxy: &ProxyRpcHandler) {
        if self.read.get_untracked() {
            return;
        }
        self.read.set(true);
        let cx = self.scope;
        let children = self.children;
        let send = create_ext_action(cx, move |result| {
            if let Ok(ProxyResponse::ReadDirResponse { items }) = result {
                let items = items
                    .into_iter()
                    .map(|item| {
                        (
                            item.path_buf.clone(),
                            FileNode {
                                scope: cx,
                                path: item.path_buf,
                                is_dir: item.is_dir,
                                read: create_rw_signal(cx, false),
                                expanded: create_rw_signal(cx, false),
                                children: create_rw_signal(cx, IndexMap::new()),
                            },
                        )
                    })
                    .collect();
                children.set(items);
            }
        });
        proxy.read_dir(self.path.clone(), move |result| {
            send(result);
        })
    }
}
