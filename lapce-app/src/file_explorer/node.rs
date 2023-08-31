use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use floem::{reactive::Memo, views::VirtualListVector};

use crate::{command::InternalCommand, listener::Listener};

#[derive(Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub is_dir: bool,
    pub read: bool,
    pub expanded: bool,
    pub children: HashMap<PathBuf, FileNode>,
    pub children_open_count: usize,
    pub line_height: Memo<f64>,
    pub internal_command: Listener<InternalCommand>,
}

impl FileNode {
    pub fn get_node(&self, path: &Path) -> Option<&FileNode> {
        let mut node = self;
        if node.path == path {
            return Some(node);
        }
        let root = node.path.clone();
        let path = path.strip_prefix(&root).ok()?;
        for path in path.ancestors().collect::<Vec<&Path>>().iter().rev() {
            if path.to_str()?.is_empty() {
                continue;
            }
            node = node.children.get(&root.join(path))?;
        }
        Some(node)
    }

    pub fn get_node_mut(&mut self, path: &Path) -> Option<&mut FileNode> {
        let mut node = self;
        if node.path == path {
            return Some(node);
        }
        let root = node.path.clone();
        let path = path.strip_prefix(&root).ok()?;
        for path in path.ancestors().collect::<Vec<&Path>>().iter().rev() {
            if path.to_str()?.is_empty() {
                continue;
            }
            node = node.children.get_mut(&root.join(path))?;
        }
        Some(node)
    }

    pub fn update_node_count_recursive(&mut self, path: &Path) {
        for current_path in path.ancestors() {
            self.update_node_count(current_path);
        }
    }

    pub fn update_node_count(&mut self, path: &Path) {
        if let Some(node) = self.get_node_mut(path) {
            if node.is_dir {
                if node.expanded {
                    node.children_open_count = node
                        .children
                        .values()
                        .map(|item| item.children_open_count + 1)
                        .sum::<usize>();
                } else {
                    node.children_open_count = 0;
                }
            }
        }
    }
}

impl VirtualListVector<(PathBuf, FileNode)> for FileNode {
    type ItemIterator = Box<dyn Iterator<Item = (PathBuf, FileNode)>>;

    fn total_size(&self) -> Option<f64> {
        let line_height = self.line_height.get();
        let count = self.children_open_count + 1;
        Some(line_height * count as f64)
    }

    fn total_len(&self) -> usize {
        0
    }

    fn slice(&mut self, _range: std::ops::Range<usize>) -> Self::ItemIterator {
        let children = if !self.is_dir {
            HashMap::new()
        } else if self.expanded {
            self.children.clone()
        } else {
            HashMap::new()
        };
        Box::new(children.into_iter())
    }
}
