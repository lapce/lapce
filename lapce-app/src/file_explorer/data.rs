use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use floem::{
    ext_event::create_ext_action,
    reactive::{RwSignal, Scope},
};
use lapce_rpc::proxy::ProxyResponse;

use super::node::FileNode;
use crate::{command::InternalCommand, window_tab::CommonData};

#[derive(Clone)]
pub struct FileExplorerData {
    pub id: RwSignal<usize>,
    pub root: RwSignal<FileNode>,
    pub common: Rc<CommonData>,
}

impl FileExplorerData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        let path = common.workspace.path.clone().unwrap_or_default();
        let new_root = cx.create_rw_signal(FileNode {
            path: path.clone(),
            is_dir: true,
            read: false,
            expanded: false,
            children: HashMap::new(),
            children_open_count: 0,
            line_height: common.ui_line_height,
            internal_command: common.internal_command,
        });
        let data = Self {
            id: cx.create_rw_signal(0),
            root: new_root,
            common,
        };
        data.toggle_expand(&path);
        data
    }

    pub fn reload(&self) {
        let path = self.root.with_untracked(|root| root.path.clone());
        self.read_dir(&path);
    }

    pub fn toggle_expand(&self, path: &Path) {
        self.id.update(|id| {
            *id += 1;
        });
        if let Some(read) = self
            .root
            .try_update(|root| {
                let read = if let Some(node) = root.get_node_mut(path) {
                    if !node.is_dir {
                        return None;
                    }
                    node.expanded = !node.expanded;
                    Some(node.read)
                } else {
                    None
                };
                if Some(true) == read {
                    root.update_node_count_recursive(path);
                }
                read
            })
            .unwrap()
        {
            if !read {
                self.read_dir(path);
            }
        }
    }

    pub fn read_dir(&self, path: &Path) {
        let root = self.root;
        let id = self.id;
        let data = self.clone();
        let send = {
            let path = path.to_path_buf();
            create_ext_action(self.common.scope, move |result| {
                if let Ok(ProxyResponse::ReadDirResponse { items }) = result {
                    id.update(|id| {
                        *id += 1;
                    });
                    root.update(|root| {
                        if let Some(node) = root.get_node_mut(&path) {
                            node.read = true;
                            let removed_paths: Vec<PathBuf> = node
                                .children
                                .keys()
                                .filter(|p| !items.iter().any(|i| &&i.path == p))
                                .map(PathBuf::from)
                                .collect();
                            for path in removed_paths {
                                node.children.remove(&path);
                            }

                            for item in items {
                                if let Some(existing) = node.children.get(&item.path)
                                {
                                    if existing.read {
                                        data.read_dir(&existing.path);
                                    }
                                } else {
                                    node.children.insert(
                                        item.path.clone(),
                                        FileNode {
                                            path: item.path,
                                            is_dir: item.is_dir,
                                            read: false,
                                            expanded: false,
                                            children: HashMap::new(),
                                            children_open_count: 0,
                                            internal_command: node.internal_command,
                                            line_height: node.line_height,
                                        },
                                    );
                                }
                            }
                        }
                        root.update_node_count_recursive(&path);
                    });
                }
            })
        };
        self.common
            .proxy
            .read_dir(path.to_path_buf(), move |result| {
                send(result);
            });
    }

    pub fn click(&self, path: &Path) {
        let is_dir = self
            .root
            .with_untracked(|root| root.get_node(path).map(|n| n.is_dir))
            .unwrap_or(false);
        if is_dir {
            self.toggle_expand(path);
        } else {
            self.common
                .internal_command
                .send(InternalCommand::OpenFile {
                    path: path.to_path_buf(),
                })
        }
    }

    pub fn double_click(&self, path: &Path) -> bool {
        let is_dir = self
            .root
            .with_untracked(|root| root.get_node(path).map(|n| n.is_dir))
            .unwrap_or(false);
        if is_dir {
            false
        } else {
            self.common
                .internal_command
                .send(InternalCommand::MakeConfirmed);
            true
        }
    }

    pub fn middle_click(&self, path: &Path) -> bool {
        let is_dir = self
            .root
            .with_untracked(|root| root.get_node(path).map(|n| n.is_dir))
            .unwrap_or(false);
        if is_dir {
            false
        } else {
            self.common
                .internal_command
                .send(InternalCommand::OpenFileInNewTab {
                    path: path.to_path_buf(),
                });
            true
        }
    }
}
