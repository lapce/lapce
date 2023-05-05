use floem::reactive::{create_rw_signal, Scope};
use indexmap::IndexMap;

use crate::window_tab::CommonData;

use super::node::FileNode;

#[derive(Clone)]
pub struct FileExplorerData {
    pub root: FileNode,
    pub common: CommonData,
}

impl FileExplorerData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        let path = common.workspace.path.clone().unwrap_or_default();
        let root = FileNode {
            scope: cx,
            path,
            is_dir: true,
            read: create_rw_signal(cx, false),
            expanded: create_rw_signal(cx, false),
            children: create_rw_signal(cx, IndexMap::new()),
        };
        if common.workspace.path.is_some() {
            root.toggle_expand(&common.proxy);
        }
        Self { root, common }
    }
}
