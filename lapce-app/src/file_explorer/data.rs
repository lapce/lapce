use std::path::PathBuf;

use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout},
    reactive::{
        create_memo, create_rw_signal, RwSignal, Scope, SignalGet, SignalUpdate,
    },
};
use indexmap::IndexMap;

use crate::window_tab::CommonData;

use super::node::FileNode;

#[derive(Clone)]
pub struct FileExplorerData {
    pub root: FileNode,
    pub common: CommonData,
    pub all_files: RwSignal<im::HashMap<PathBuf, FileNode>>,
}

impl FileExplorerData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        let path = common.workspace.path.clone().unwrap_or_default();
        let config = common.config;
        let all_files = create_rw_signal(cx, im::HashMap::new());
        let line_height = create_memo(cx, move |_| {
            let config = config.get();
            let mut text_layout = TextLayout::new();

            let family: Vec<FamilyOwned> =
                FamilyOwned::parse_list(&config.ui.font_family).collect();
            let attrs = Attrs::new()
                .family(&family)
                .font_size(config.ui.font_size() as f32)
                .line_height(LineHeightValue::Normal(1.6));
            let attrs_list = AttrsList::new(attrs);
            text_layout.set_text("W", attrs_list);
            text_layout.size().height
        });
        let root = FileNode {
            scope: cx,
            path: path.clone(),
            is_dir: true,
            read: create_rw_signal(cx, false),
            expanded: create_rw_signal(cx, false),
            children: create_rw_signal(cx, IndexMap::new()),
            children_open_count: create_rw_signal(cx, 0),
            all_files,
            line_height,
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
