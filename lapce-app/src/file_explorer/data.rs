use std::path::PathBuf;

use floem::reactive::{create_rw_signal, RwSignal, Scope, SignalUpdate};
use indexmap::IndexMap;

use super::node::FileNode;
use crate::window_tab::CommonData;

#[derive(Clone)]
pub struct FileExplorerData {
    pub root: FileNode,
    pub common: CommonData,
    pub all_files: RwSignal<im::HashMap<PathBuf, FileNode>>,
}

impl FileExplorerData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        let path = common.workspace.path.clone().unwrap_or_default();
        let all_files = create_rw_signal(cx, im::HashMap::new());
        let root = FileNode {
            scope: cx,
            path: path.clone(),
            is_dir: true,
            read: create_rw_signal(cx, false),
            expanded: create_rw_signal(cx, false),
            children: create_rw_signal(cx, IndexMap::new()),
            children_open_count: create_rw_signal(cx, 0),
            all_files,
            line_height: common.ui_line_height,
            internal_command: common.internal_command,
        };
        all_files.update(|all_files| {
            all_files.insert(path, root.clone());
        });
        if common.workspace.path.is_some() {
            root.toggle_expand(&common.proxy);
        }
        Self {
            root,
            common,
            all_files,
        }
    }
}
