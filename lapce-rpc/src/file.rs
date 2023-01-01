use std::{
    cmp::{self, Ordering},
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileNodeItem {
    pub path_buf: PathBuf,
    pub is_dir: bool,
    pub read: bool,
    pub open: bool,
    pub children: HashMap<PathBuf, FileNodeItem>,
    pub children_open_count: usize,
}

impl cmp::PartialOrd for FileNodeItem {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        match (self.is_dir, other.is_dir) {
            (true, false) => return Some(Ordering::Less),
            (false, true) => return Some(Ordering::Greater),
            _ => {}
        }

        let self_file_name = self.path_buf.file_name()?.to_str()?;
        let other_file_name = other.path_buf.file_name()?.to_str()?;

        match (
            self_file_name.starts_with('.'),
            other_file_name.starts_with('.'),
        ) {
            (true, false) => return Some(Ordering::Less),
            (false, true) => return Some(Ordering::Greater),
            _ => {}
        }

        Some(human_sort::compare(
            &self_file_name.to_lowercase(),
            &other_file_name.to_lowercase(),
        ))
    }
}

impl cmp::Ord for FileNodeItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl FileNodeItem {
    pub fn sorted_children(&self) -> Vec<&FileNodeItem> {
        let mut children = self.children.values().collect::<Vec<&FileNodeItem>>();
        children.sort();
        children
    }

    pub fn sorted_children_mut(&mut self) -> Vec<&mut FileNodeItem> {
        let mut children = self
            .children
            .values_mut()
            .collect::<Vec<&mut FileNodeItem>>();
        children.sort();
        children
    }

    pub fn get_file_node(&self, path: &Path) -> Option<&FileNodeItem> {
        let path_buf = self.path_buf.clone();
        let path = path.strip_prefix(&self.path_buf).ok()?;
        let ancestors = path.ancestors().collect::<Vec<&Path>>();

        let mut node = self;
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = node.children.get(&path_buf.join(p))?;
        }
        Some(node)
    }

    pub fn get_file_node_mut(&mut self, path: &Path) -> Option<&mut FileNodeItem> {
        let path_buf = self.path_buf.clone();
        let path = path.strip_prefix(&self.path_buf).ok()?;
        let ancestors = path.ancestors().collect::<Vec<&Path>>();

        let mut node = self;
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = node.children.get_mut(&path_buf.join(p))?;
        }
        Some(node)
    }

    pub fn remove_child(&mut self, path: &Path) -> Option<FileNodeItem> {
        let parent = path.parent()?;
        let node = self.get_file_node_mut(parent)?;
        let node = node.children.remove(path)?;
        for p in path.ancestors() {
            self.update_node_count(p);
        }

        Some(node)
    }

    pub fn add_child(&mut self, path: &Path, is_dir: bool) -> Option<()> {
        let parent = path.parent()?;
        let node = self.get_file_node_mut(parent)?;
        node.children.insert(
            PathBuf::from(path),
            FileNodeItem {
                path_buf: PathBuf::from(path),
                is_dir,
                read: false,
                open: false,
                children: HashMap::new(),
                children_open_count: 0,
            },
        );
        for p in path.ancestors() {
            self.update_node_count(p);
        }

        Some(())
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
            self.update_node_count(p);
        }
    }

    pub fn update_node_count(&mut self, path: &Path) -> Option<()> {
        let node = self.get_file_node_mut(path)?;
        if node.is_dir {
            if node.open {
                node.children_open_count = node
                    .children
                    .values()
                    .map(|item| item.children_open_count + 1)
                    .sum::<usize>();
            } else {
                node.children_open_count = 0;
            }
        }
        None
    }
}
