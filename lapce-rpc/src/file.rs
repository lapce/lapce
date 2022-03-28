use std::{
    cmp::{self, Ordering},
    collections::HashMap,
    path::PathBuf,
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
        let mut children = self
            .children
            .iter()
            .map(|(_, item)| item)
            .collect::<Vec<&FileNodeItem>>();
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
}
