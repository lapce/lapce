use std::{
    cmp::{Ord, Ordering, PartialOrd},
    collections::HashMap,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileNodeViewKind {
    /// An actual file/directory
    Path(PathBuf),
    /// We are renaming the file at this path
    Renaming { path: PathBuf, err: Option<String> },
    /// We are naming a new file/directory
    Naming { err: Option<String> },
    Duplicating {
        /// The path that is being duplicated
        source: PathBuf,
        err: Option<String>,
    },
}
impl FileNodeViewKind {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Renaming { path, .. } => Some(path),
            Self::Naming { .. } => None,
            Self::Duplicating { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamingState {
    /// Actively naming
    Naming,
    /// Application of the naming is pending
    Pending,
    /// There's an active error with the typed name
    Err { err: String },
}
impl NamingState {
    pub fn is_accepting_input(&self) -> bool {
        match self {
            Self::Naming | Self::Err { .. } => true,
            Self::Pending => false,
        }
    }

    pub fn is_err(&self) -> bool {
        match self {
            Self::Naming | Self::Pending => false,
            Self::Err { .. } => true,
        }
    }

    pub fn err(&self) -> Option<&str> {
        match self {
            Self::Err { err } => Some(err.as_str()),
            _ => None,
        }
    }

    pub fn set_ok(&mut self) {
        *self = Self::Naming;
    }

    pub fn set_pending(&mut self) {
        *self = Self::Pending;
    }

    pub fn set_err(&mut self, err: String) {
        *self = Self::Err { err };
    }
}

/// Stores the state of any in progress rename of a path.
///
/// The `editor_needs_reset` field is `true` if the rename editor should have its contents reset
/// when the view function next runs.
#[derive(Debug, Clone)]
pub struct Renaming {
    pub state: NamingState,
    /// Original file's path
    pub path: PathBuf,
    pub editor_needs_reset: bool,
}

#[derive(Debug, Clone)]
pub struct NewNode {
    pub state: NamingState,
    /// If true, then we are creating a directory
    pub is_dir: bool,
    /// The folder that the file/directory is being created within
    pub base_path: PathBuf,
    pub editor_needs_reset: bool,
}

#[derive(Debug, Clone)]
pub struct Duplicating {
    pub state: NamingState,
    /// Path to the item being duplicated
    pub path: PathBuf,
    pub editor_needs_reset: bool,
}

#[derive(Debug, Clone)]
pub enum Naming {
    None,
    Renaming(Renaming),
    NewNode(NewNode),
    Duplicating(Duplicating),
}
impl Naming {
    pub fn state(&self) -> Option<&NamingState> {
        match self {
            Self::None => None,
            Self::Renaming(rename) => Some(&rename.state),
            Self::NewNode(state) => Some(&state.state),
            Self::Duplicating(state) => Some(&state.state),
        }
    }

    pub fn state_mut(&mut self) -> Option<&mut NamingState> {
        match self {
            Self::None => None,
            Self::Renaming(rename) => Some(&mut rename.state),
            Self::NewNode(state) => Some(&mut state.state),
            Self::Duplicating(state) => Some(&mut state.state),
        }
    }

    pub fn is_accepting_input(&self) -> bool {
        self.state().is_some_and(NamingState::is_accepting_input)
    }

    pub fn editor_needs_reset(&self) -> bool {
        match self {
            Naming::None => false,
            Naming::Renaming(rename) => rename.editor_needs_reset,
            Naming::NewNode(state) => state.editor_needs_reset,
            Naming::Duplicating(state) => state.editor_needs_reset,
        }
    }

    pub fn set_editor_needs_reset(&mut self, needs_reset: bool) {
        match self {
            Naming::None => {}
            Naming::Renaming(rename) => rename.editor_needs_reset = needs_reset,
            Naming::NewNode(state) => state.editor_needs_reset = needs_reset,
            Naming::Duplicating(state) => state.editor_needs_reset = needs_reset,
        }
    }

    pub fn set_ok(&mut self) {
        if let Some(state) = self.state_mut() {
            state.set_ok();
        }
    }

    pub fn set_pending(&mut self) {
        if let Some(state) = self.state_mut() {
            state.set_pending();
        }
    }

    pub fn set_err(&mut self, err: String) {
        if let Some(state) = self.state_mut() {
            state.set_err(err);
        }
    }

    pub fn as_renaming(&self) -> Option<&Renaming> {
        match self {
            Naming::Renaming(rename) => Some(rename),
            _ => None,
        }
    }

    /// The extra node that should be added after the node at `path`
    pub fn extra_node(
        &self,
        is_dir: bool,
        level: usize,
        path: &Path,
    ) -> Option<FileNodeViewData> {
        match self {
            Naming::NewNode(n) if n.base_path == path => Some(FileNodeViewData {
                kind: FileNodeViewKind::Naming {
                    err: n.state.err().map(ToString::to_string),
                },
                is_dir: n.is_dir,
                is_root: false,
                open: false,
                level: level + 1,
            }),
            Naming::Duplicating(d) if d.path == path => Some(FileNodeViewData {
                kind: FileNodeViewKind::Duplicating {
                    source: d.path.to_path_buf(),
                    err: d.state.err().map(ToString::to_string),
                },
                is_dir,
                is_root: false,
                open: false,
                level: level + 1,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileNodeViewData {
    pub kind: FileNodeViewKind,
    pub is_dir: bool,
    pub is_root: bool,
    pub open: bool,
    pub level: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileNodeItem {
    pub path: PathBuf,
    pub is_dir: bool,
    /// Whether the directory's children have been read.
    /// Does nothing if not a directory.
    pub read: bool,
    /// Whether the directory is open in the explorer view.
    pub open: bool,
    pub children: HashMap<PathBuf, FileNodeItem>,
    /// The number of child (directories) that are open themselves
    /// Used for sizing of the explorer list
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
    /// Collect the children, sorted by name.
    /// Note: this will be empty if the directory has not been read.
    pub fn sorted_children(&self) -> Vec<&FileNodeItem> {
        let mut children = self.children.values().collect::<Vec<&FileNodeItem>>();
        children.sort();
        children
    }

    /// Collect the children, sorted by name.
    /// Note: this will be empty if the directory has not been read.
    pub fn sorted_children_mut(&mut self) -> Vec<&mut FileNodeItem> {
        let mut children = self
            .children
            .values_mut()
            .collect::<Vec<&mut FileNodeItem>>();
        children.sort();
        children
    }

    /// Returns an iterator over the ancestors of `path`, starting with the first descendant of `prefix`.
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
    ) -> Option<impl Iterator<Item = &'a Path> + use<'a>> {
        let take = if let Ok(suffix) = path.strip_prefix(&self.path) {
            suffix.components().count()
        } else {
            return None;
        };

        #[allow(clippy::needless_collect)] // Ancestors is not reversible
        let ancestors = path.ancestors().take(take).collect::<Vec<&Path>>();
        Some(ancestors.into_iter().rev())
    }

    /// Recursively get the node at `path`.
    pub fn get_file_node(&self, path: &Path) -> Option<&FileNodeItem> {
        self.ancestors_rev(path)?
            .try_fold(self, |node, path| node.children.get(path))
    }

    /// Recursively get the (mutable) node at `path`.
    pub fn get_file_node_mut(&mut self, path: &Path) -> Option<&mut FileNodeItem> {
        self.ancestors_rev(path)?
            .try_fold(self, |node, path| node.children.get_mut(path))
    }

    /// Remove a specific child from the node.
    /// The path is recursive and will remove the child from parent indicated by the path.
    pub fn remove_child(&mut self, path: &Path) -> Option<FileNodeItem> {
        let parent = path.parent()?;
        let node = self.get_file_node_mut(parent)?;
        let node = node.children.remove(path)?;
        for p in path.ancestors() {
            self.update_node_count(p);
        }

        Some(node)
    }

    /// Add a new (unread & unopened) child to the node.
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

    /// Set the children of the node.
    /// Note: this opens the node.
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
        naming: &Naming,
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

        if current >= min {
            let kind = if let Naming::Renaming(r) = &naming {
                if r.path == self.path {
                    FileNodeViewKind::Renaming {
                        path: self.path.clone(),
                        err: r.state.err().map(ToString::to_string),
                    }
                } else {
                    FileNodeViewKind::Path(self.path.clone())
                }
            } else {
                FileNodeViewKind::Path(self.path.clone())
            };
            view_items.push(FileNodeViewData {
                kind,
                is_dir: self.is_dir,
                is_root: level == 1,
                open: self.open,
                level,
            });
        }

        self.append_children_view_slice(view_items, naming, min, max, current, level)
    }

    /// Calculate the row where the file resides
    pub fn find_file_at_line(&self, file_path: &Path) -> (bool, f64) {
        let mut line = 0.0;
        if !self.open {
            return (false, line);
        }
        for item in self.sorted_children() {
            line += 1.0;
            match (item.is_dir, item.open, item.path == file_path) {
                (_, _, true) => {
                    return (true, line);
                }
                (true, true, _) => {
                    let (found, item_position) = item.find_file_at_line(file_path);
                    line += item_position;
                    if found {
                        return (true, line);
                    }
                }
                _ => {}
            }
        }
        (false, line)
    }

    /// Append the children of this item with the given level
    pub fn append_children_view_slice(
        &self,
        view_items: &mut Vec<FileNodeViewData>,
        naming: &Naming,
        min: usize,
        max: usize,
        mut i: usize,
        level: usize,
    ) -> usize {
        let mut naming_extra = naming.extra_node(self.is_dir, level, &self.path);

        if !self.open {
            // If the folder isn't open, then we just put it right at the top
            if i >= min {
                if let Some(naming_extra) = naming_extra {
                    view_items.push(naming_extra);
                    i += 1;
                }
            }
            return i;
        }

        let naming_is_dir = naming_extra.as_ref().map(|n| n.is_dir).unwrap_or(false);
        // Immediately put the naming entry first if it's a directory
        if naming_is_dir {
            if let Some(node) = naming_extra.take() {
                // Actually add the node if it's within the range
                if i >= min {
                    view_items.push(node);
                    i += 1;
                }
            }
        }

        let mut after_dirs = false;

        for item in self.sorted_children() {
            // If we're naming a file at the root, then wait until we've added the directories
            // before adding the input node
            if naming_extra.is_some()
                && !naming_is_dir
                && !item.is_dir
                && !after_dirs
            {
                after_dirs = true;

                // If we're creating a new file node, then we show it after the directories
                // TODO(minor): should this be i >= min or i + 1 >= min?
                if i >= min {
                    if let Some(node) = naming_extra.take() {
                        view_items.push(node);
                        i += 1;
                    }
                }
            }
            i = item.append_view_slice(
                view_items,
                naming,
                min,
                max,
                i + 1,
                level + 1,
            );
            if i > max {
                return i;
            }
        }

        // If it has not been added yet, add it now.
        if i >= min {
            if let Some(node) = naming_extra {
                view_items.push(node);
                i += 1;
            }
        }

        i
    }
}
