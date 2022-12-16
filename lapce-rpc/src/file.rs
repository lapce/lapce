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

impl std::cmp::PartialOrd for FileNodeItem {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        let self_dir = self.is_dir;
        let other_dir = other.is_dir;
        if self_dir && !other_dir {
            return Some(cmp::Ordering::Less);
        }
        if !self_dir && other_dir {
            return Some(cmp::Ordering::Greater);
        }

        let self_file_name = self.path_buf.file_name()?.to_str()?.to_lowercase();
        let other_file_name = other.path_buf.file_name()?.to_str()?.to_lowercase();
        if self_file_name.starts_with('.') && !other_file_name.starts_with('.') {
            return Some(cmp::Ordering::Less);
        }
        if !self_file_name.starts_with('.') && other_file_name.starts_with('.') {
            return Some(cmp::Ordering::Greater);
        }
        self_file_name.partial_cmp(&other_file_name)
    }
}

impl FileNodeItem {
    pub fn sorted_children(&self) -> Vec<&FileNodeItem> {
        let mut children = self.children.values().collect::<Vec<&FileNodeItem>>();
        children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, true) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
        });
        children
    }

    pub fn sorted_children_mut(&mut self) -> Vec<&mut FileNodeItem> {
        let mut children = self
            .children
            .iter_mut()
            .map(|(_, item)| item)
            .collect::<Vec<&mut FileNodeItem>>();
        children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, true) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => a
                .path_buf
                .to_str()
                .unwrap()
                .cmp(b.path_buf.to_str().unwrap()),
        });
        children
    }

    pub fn get_file_node(&self, path: &Path) -> Option<&FileNodeItem> {
        let path_buf = self.path_buf.clone();
        let path = path.strip_prefix(&self.path_buf).ok()?;
        let ancestors = path.ancestors().collect::<Vec<&Path>>();

        let mut node = Some(self);
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = Some(node?.children.get(&path_buf.join(p))?);
        }
        node
    }

    pub fn get_file_node_mut(&mut self, path: &Path) -> Option<&mut FileNodeItem> {
        let path_buf = self.path_buf.clone();
        let path = path.strip_prefix(&self.path_buf).ok()?;
        let ancestors = path.ancestors().collect::<Vec<&Path>>();

        let mut node = Some(self);
        for p in ancestors[..ancestors.len() - 1].iter().rev() {
            node = Some(node?.children.get_mut(&path_buf.join(p))?);
        }
        node
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
