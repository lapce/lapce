use std::path::PathBuf;

use floem::{
    ext_event::create_ext_action,
    reactive::{
        create_rw_signal, Memo, RwSignal, Scope, SignalGet, SignalGetUntracked,
        SignalSet, SignalUpdate, SignalWithUntracked,
    },
    views::VirtualListVector,
};
use indexmap::IndexMap;
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};

use crate::{command::InternalCommand, listener::Listener};

#[derive(Clone)]
pub struct FileNode {
    pub scope: Scope,
    pub path: PathBuf,
    pub is_dir: bool,
    pub read: RwSignal<bool>,
    pub expanded: RwSignal<bool>,
    pub children: RwSignal<IndexMap<PathBuf, FileNode>>,
    pub children_open_count: RwSignal<usize>,
    pub all_files: RwSignal<im::HashMap<PathBuf, FileNode>>,
    pub line_height: Memo<f64>,
    pub internal_command: Listener<InternalCommand>,
}

impl VirtualListVector<(PathBuf, FileNode)> for FileNode {
    type ItemIterator = Box<dyn Iterator<Item = (PathBuf, FileNode)>>;

    fn total_size(&self) -> Option<f64> {
        let line_height = self.line_height.get();
        let count = self.children_open_count.get() + 1;
        Some(line_height * count as f64)
    }

    fn total_len(&self) -> usize {
        0
    }

    fn slice(&mut self, _range: std::ops::Range<usize>) -> Self::ItemIterator {
        let children = if !self.is_dir {
            IndexMap::new()
        } else {
            let expanded = self.expanded.get();
            if expanded {
                self.children.get()
            } else {
                IndexMap::new()
            }
        };
        Box::new(children.into_iter())
    }
}

impl FileNode {
    pub fn click(&self, proxy: &ProxyRpcHandler) {
        if self.is_dir {
            self.toggle_expand(proxy)
        } else {
            self.internal_command.send(InternalCommand::OpenFile {
                path: self.path.clone(),
            })
        }
    }

    pub fn toggle_expand(&self, proxy: &ProxyRpcHandler) {
        if !self.is_dir {
            return;
        }
        let expanded = self.expanded.get_untracked();
        if expanded {
            self.expanded.set(false);
            self.update_open_count();
        } else {
            self.expanded.set(true);
            if self.read.get_untracked() {
                self.update_open_count();
            } else {
                self.read_dir(proxy);
            }
        }
    }

    fn update_open_count(&self) {
        self.all_files.with_untracked(|all_files| {
            for current_path in self.path.ancestors() {
                if let Some(node) = all_files.get(current_path) {
                    node.update_node_open_count();
                }
            }
        });
    }

    fn update_node_open_count(&self) {
        if self.is_dir {
            let expanded = self.expanded.get_untracked();
            if expanded {
                let count = self.children.with_untracked(|children| {
                    children
                        .values()
                        .map(|node| node.children_open_count.get_untracked() + 1)
                        .sum::<usize>()
                });
                self.children_open_count.set(count);
            } else {
                self.children_open_count.set(0);
            }
        }
    }

    fn read_dir(&self, proxy: &ProxyRpcHandler) {
        if self.read.get_untracked() {
            return;
        }
        self.read.set(true);
        let cx = self.scope;
        let file_node = self.clone();
        let send = create_ext_action(self.scope, move |result| {
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
                                children_open_count: create_rw_signal(cx, 0),
                                all_files: file_node.all_files,
                                line_height: file_node.line_height,
                                internal_command: file_node.internal_command,
                            },
                        )
                    })
                    .collect::<IndexMap<PathBuf, FileNode>>();
                file_node.all_files.update(|all_files| {
                    for (_, item) in items.iter() {
                        all_files.insert(item.path.clone(), item.clone());
                    }
                });
                file_node.children.set(items);
                file_node.update_open_count();
            }
        });
        proxy.read_dir(self.path.clone(), move |result| {
            send(result);
        })
    }
}
