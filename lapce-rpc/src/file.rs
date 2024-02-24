use std::{
    cmp::{Ord, Ordering, PartialOrd},
    collections::HashMap,
    mem,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// UTF8 line and column-offset
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct LineCol {
    pub line: usize,
    pub column: usize,
}

#[derive(
    Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct PathObject {
    pub path: PathBuf,
    pub linecol: Option<LineCol>,
    pub is_dir: bool,
}

impl PathObject {
    pub fn new(
        path: PathBuf,
        is_dir: bool,
        line: usize,
        column: usize,
    ) -> PathObject {
        PathObject {
            path,
            is_dir,
            linecol: Some(LineCol { line, column }),
        }
    }

    pub fn from_path(path: PathBuf, is_dir: bool) -> PathObject {
        PathObject {
            path,
            is_dir,
            linecol: None,
        }
    }
}

/// Stores the state of any in progress rename of a path.
///
/// The `editor_needs_reset` field is `true` if the rename editor should have its contents reset
/// when the view function next runs.
#[derive(Clone)]
pub enum RenameState {
    NotRenaming,
    Renaming {
        path: PathBuf,
        editor_needs_reset: bool,
    },
    RenameRequestPending {
        path: PathBuf,
        editor_needs_reset: bool,
    },
    RenameErr {
        path: PathBuf,
        editor_needs_reset: bool,
        err: String,
    },
}

impl RenameState {
    pub fn is_accepting_input(&self) -> bool {
        match self {
            Self::NotRenaming | Self::RenameRequestPending { .. } => false,
            Self::Renaming { .. } | Self::RenameErr { .. } => true,
        }
    }

    pub fn is_err(&self) -> bool {
        match self {
            Self::NotRenaming
            | Self::Renaming { .. }
            | Self::RenameRequestPending { .. } => false,
            Self::RenameErr { .. } => true,
        }
    }

    pub fn set_ok(&mut self) {
        if let &mut Self::RenameErr {
            ref mut path,
            editor_needs_reset,
            ..
        } = self
        {
            let path = mem::take(path);

            *self = Self::Renaming {
                path,
                editor_needs_reset,
            };
        }
    }

    pub fn set_pending(&mut self) {
        if let &mut Self::Renaming {
            ref mut path,
            editor_needs_reset,
        }
        | &mut Self::RenameErr {
            ref mut path,
            editor_needs_reset,
            ..
        } = self
        {
            let path = mem::take(path);

            *self = Self::RenameRequestPending {
                path,
                editor_needs_reset,
            };
        }
    }

    pub fn set_err(&mut self, err: String) {
        if let &mut Self::Renaming {
            ref mut path,
            editor_needs_reset,
        }
        | &mut Self::RenameRequestPending {
            ref mut path,
            editor_needs_reset,
        }
        | &mut Self::RenameErr {
            ref mut path,
            editor_needs_reset,
            ..
        } = self
        {
            let path = mem::take(path);

            *self = Self::RenameErr {
                path,
                editor_needs_reset,
                err,
            };
        }
    }

    pub fn set_editor_needs_reset(&mut self, needs_reset: bool) {
        if let Self::Renaming {
            editor_needs_reset, ..
        }
        | Self::RenameRequestPending {
            editor_needs_reset, ..
        }
        | Self::RenameErr {
            editor_needs_reset, ..
        } = self
        {
            *editor_needs_reset = needs_reset;
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::NotRenaming => None,
            Self::Renaming { path, .. }
            | Self::RenameRequestPending { path, .. }
            | Self::RenameErr { path, .. } => Some(path),
        }
    }

    pub fn editor_needs_reset(&self) -> bool {
        match self {
            Self::NotRenaming => false,
            &Self::Renaming {
                editor_needs_reset, ..
            }
            | &Self::RenameRequestPending {
                editor_needs_reset, ..
            }
            | &Self::RenameErr {
                editor_needs_reset, ..
            } => editor_needs_reset,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IsRenaming {
    NotRenaming,
    Renaming { err: Option<String> },
}

impl IsRenaming {
    fn is_node_renaming(rename_state: &RenameState, node_path: &Path) -> Self {
        match rename_state {
            RenameState::NotRenaming => Self::NotRenaming,
            RenameState::Renaming { path, .. }
            | RenameState::RenameRequestPending { path, .. } => {
                if path == node_path {
                    Self::Renaming { err: None }
                } else {
                    Self::NotRenaming
                }
            }
            RenameState::RenameErr { path, err, .. } => {
                if path == node_path {
                    Self::Renaming {
                        err: Some(err.clone()),
                    }
                } else {
                    Self::NotRenaming
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct FileNodeViewData {
    pub path: PathBuf,
    pub is_dir: bool,
    pub open: bool,
    pub is_renaming: IsRenaming,
    pub level: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileNodeItem {
    pub path: PathBuf,
    pub is_dir: bool,
    pub read: bool,
    pub open: bool,
    pub children: HashMap<PathBuf, FileNodeItem>,
    pub children_open_count: usize,
}

impl PartialOrd for FileNodeItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileNodeItem {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.is_dir, other.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => {
                let [self_file_name, other_file_name] = [&self.path, &other.path]
                    .map(|path| {
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase()
                    });
                human_sort::compare(&self_file_name, &other_file_name)
            }
        }
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
    /// # use std::path::{Path, PathBuf};
    /// # use std::collections::HashMap;
    /// #
    /// let node_item = FileNodeItem {
    ///     path_buf: PathBuf::from("/pre/fix"),
    ///     // ...
    /// #    is_dir: true,
    /// #    read: false,
    /// #    open: false,
    /// #    children: HashMap::new(),
    /// #    children_open_count: 0,
    ///};
    /// let mut iter = node_item.ancestors_rev(Path::new("/pre/fix/foo/bar")).unwrap();
    /// assert_eq!(Some(Path::new("/pre/fix/foo")), iter.next());
    /// assert_eq!(Some(Path::new("/pre/fix/foo/bar")), iter.next());
    /// ```
    fn ancestors_rev<'a>(
        &self,
        path: &'a Path,
    ) -> Option<impl Iterator<Item = &'a Path>> {
        let take = if let Ok(suffix) = path.strip_prefix(&self.path) {
            suffix.components().count()
        } else {
            return None;
        };

        #[allow(clippy::needless_collect)] // Ancestors is not reversible
        let ancestors = path.ancestors().take(take).collect::<Vec<&Path>>();
        Some(ancestors.into_iter().rev())
    }

    pub fn get_file_node(&self, path: &Path) -> Option<&FileNodeItem> {
        self.ancestors_rev(path)?
            .try_fold(self, |node, path| node.children.get(path))
    }

    pub fn get_file_node_mut(&mut self, path: &Path) -> Option<&mut FileNodeItem> {
        self.ancestors_rev(path)?
            .try_fold(self, |node, path| node.children.get_mut(path))
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
                path: PathBuf::from(path),
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

    pub fn update_node_count_recursive(&mut self, path: &Path) {
        for current_path in path.ancestors() {
            self.update_node_count(current_path);
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

    pub fn append_view_slice(
        &self,
        view_items: &mut Vec<FileNodeViewData>,
        rename_state: &RenameState,
        min: usize,
        max: usize,
        current: usize,
        level: usize,
    ) -> usize {
        if current > max {
            return current;
        }
        if current + self.children_open_count < min {
            return current + self.children_open_count;
        }

        let mut i = current;
        if current >= min {
            view_items.push(FileNodeViewData {
                path: self.path.clone(),
                is_dir: self.is_dir,
                open: self.open,
                is_renaming: IsRenaming::is_node_renaming(rename_state, &self.path),
                level,
            });
        }

        if self.open {
            for item in self.sorted_children() {
                i = item.append_view_slice(
                    view_items,
                    rename_state,
                    min,
                    max,
                    i + 1,
                    level + 1,
                );
                if i > max {
                    return i;
                }
            }
        }
        i
    }
}
