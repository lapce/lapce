use std::{
    cmp::{Ord, Ordering, PartialOrd},
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

impl PartialOrd for FileNodeItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self.is_dir, other.is_dir) {
            (true, false) => return Some(Ordering::Less),
            (false, true) => return Some(Ordering::Greater),
            _ => {}
        }

        let self_file_name = self.path_buf.file_name()?.to_str()?;
        let other_file_name = other.path_buf.file_name()?.to_str()?;

        // TODO(dbuga): it would be nicer if human_sort had a `eq_ignore_ascii_case` function.
        Some(human_sort::compare(
            &self_file_name.to_lowercase(),
            &other_file_name.to_lowercase(),
        ))
    }
}

impl Ord for FileNodeItem {
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

    /// Returns an iterator over the ancestors of `path`, starting with the first decendant of `prefix`.
    ///
    /// # Example:
    /// (ignored because the function is private but I promise this passes)
    /// ```rust,ignore
    /// # use lapce_rpc::file::FileNodeItem;
    /// # use std::path::Path;
    /// #
    /// let mut iter = FileNodeItem::ancestors_rev(Path::new("/pre/fix"), Path::new("/pre/fix/foo/bar"));
    /// assert_eq!(Some(Path::new("/pre/fix/foo")), iter.next());
    /// assert_eq!(Some(Path::new("/pre/fix/foo/bar")), iter.next());
    /// ```
    fn ancestors_rev<'a>(
        prefix: &Path,
        path: &'a Path,
    ) -> impl Iterator<Item = &'a Path> {
        let skip = prefix.components().count();
        let take = path.components().count() - skip;

        let ancestors = path.ancestors().take(take).collect::<Vec<&Path>>();
        ancestors.into_iter().rev()
    }

    pub fn get_file_node(&self, path: &Path) -> Option<&FileNodeItem> {
        let mut node = self;
        for p in Self::ancestors_rev(&self.path_buf, path) {
            node = node.children.get(p)?;
        }
        Some(node)
    }

    pub fn get_file_node_mut(&mut self, path: &Path) -> Option<&mut FileNodeItem> {
        let iterator = Self::ancestors_rev(&self.path_buf, path);

        let mut node = self;
        for p in iterator {
            node = node.children.get_mut(p)?;
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
            node.children_open_count = if node.open {
                node.children
                    .values()
                    .map(|item| item.children_open_count + 1)
                    .sum::<usize>()
            } else {
                0
            };
        }
        None
    }
}
