use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use druid::WidgetId;
use lapce_rpc::file::FileNodeItem;

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

    pub fn set_item_children(
        &mut self,
        path: &Path,
        children: HashMap<PathBuf, FileNodeItem>,
    ) {
        if let Some(node) = self.get_file_node_mut(path) {
            node.open = true;
            node.read = true;
            node.children = children;
        }

        for p in path.ancestors() {
            self.update_node_count(&PathBuf::from(p));
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

    pub fn get_file_node_mut(&mut self, path: &Path) -> Option<&mut FileNodeItem> {
        let mut node = Some(&mut self.root);

        let ancestors = path.ancestors().collect::<Vec<&Path>>();
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = Some(node?.children.get_mut(&PathBuf::from(p))?);
        }
        node
    }

    pub fn get_file_node(&self, path: &Path) -> Option<&FileNodeItem> {
        let mut node = Some(&self.root);

        let ancestors = path.ancestors().collect::<Vec<&Path>>();
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = Some(node?.children.get(&PathBuf::from(p))?);
        }
        node
    }

    pub fn update_node_count(&mut self, path: &Path) -> Option<()> {
        let node = self.get_file_node_mut(path)?;
        if node.is_dir {
            if node.open {
                node.children_open_count = node
                    .children
                    .iter()
                    .map(|(_, item)| item.children_open_count + 1)
                    .sum::<usize>();
            } else {
                node.children_open_count = 0;
            }
        }
        None
    }
}

impl Default for FilePickerData {
    fn default() -> Self {
        Self::new()
    }
}
