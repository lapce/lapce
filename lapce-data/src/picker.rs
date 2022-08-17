use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use druid::{ExtEventSink, Target, WidgetId};
use lapce_rpc::{file::FileNodeItem, proxy::ProxyResponse};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    proxy::LapceProxy,
};

#[derive(Clone)]
pub struct FilePickerData {
    pub widget_id: WidgetId,
    pub editor_view_id: WidgetId,
    pub active: bool,
    pub root: FileNodeItem,
    pub home: PathBuf,
    pub pwd: PathBuf,
    pub index: usize,
}

impl FilePickerData {
    pub fn new() -> Self {
        let root = FileNodeItem {
            path_buf: PathBuf::from("/"),
            is_dir: true,
            read: false,
            open: false,
            children: HashMap::new(),
            children_open_count: 0,
        };
        let home = PathBuf::from("/");
        let pwd = PathBuf::from("/");
        Self {
            widget_id: WidgetId::next(),
            editor_view_id: WidgetId::next(),
            active: false,
            root,
            home,
            pwd,
            index: 0,
        }
    }

    pub fn init_home(&mut self, home: &Path) {
        self.home = home.to_path_buf();
        let mut current_file_node = FileNodeItem {
            path_buf: home.to_path_buf(),
            is_dir: true,
            read: false,
            open: false,
            children: HashMap::new(),
            children_open_count: 0,
        };
        let mut current_path = home.to_path_buf();

        let mut ancestors = home.ancestors();
        ancestors.next();

        for p in ancestors {
            let mut file_node = FileNodeItem {
                path_buf: PathBuf::from(p),
                is_dir: true,
                read: false,
                open: true,
                children: HashMap::new(),
                children_open_count: 0,
            };
            file_node
                .children
                .insert(current_path.clone(), current_file_node.clone());
            current_file_node = file_node;
            current_path = PathBuf::from(p);
        }
        self.root = current_file_node;
        self.pwd = home.to_path_buf();
    }

    pub fn read_dir(
        path: &Path,
        tab_id: WidgetId,
        proxy: &LapceProxy,
        event_sink: ExtEventSink,
    ) {
        let path = PathBuf::from(path);
        let local_path = path.clone();
        proxy.proxy_rpc.read_dir(local_path, move |result| {
            if let Ok(ProxyResponse::ReadDirResponse { items }) = result {
                let path = path.clone();
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdatePickerItems(path, items),
                    Target::Widget(tab_id),
                );
            }
        });
    }
}

impl Default for FilePickerData {
    fn default() -> Self {
        Self::new()
    }
}
