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

#[derive(Debug, Clone)]
pub enum Naming {
    /// Not naming anything
    None,
    Renaming(RenameState),
    NewNode(NewNodeState),
    Duplicating(DuplicateState),
}
impl Naming {
    pub fn as_renaming(&self) -> Option<&RenameState> {
        match self {
            Naming::Renaming(state) => Some(state),
            _ => None,
        }
    }

    /// Change to error state, if doing any naming
    pub fn set_err(&mut self, message: String) {
        match self {
            Naming::None => {}
            Naming::Renaming(state) => state.set_err(message),
            Naming::NewNode(state) => state.set_err(message),
            Naming::Duplicating(state) => state.set_err(message),
        }
    }

    pub fn set_ok(&mut self) {
        match self {
            Naming::None => {}
            Naming::Renaming(state) => state.set_ok(),
            Naming::NewNode(state) => state.set_ok(),
            Naming::Duplicating(state) => state.set_ok(),
        }
    }

    pub fn set_pending(&mut self) {
        match self {
            Naming::None => {}
            Naming::Renaming(state) => state.set_pending(),
            Naming::NewNode(state) => state.set_pending(),
            Naming::Duplicating(state) => state.set_pending(),
        }
    }

    pub fn is_accepting_input(&self) -> bool {
        match self {
            Naming::None => false,
            Naming::Renaming(state) => state.is_accepting_input(),
            Naming::NewNode(state) => state.is_accepting_input(),
            Naming::Duplicating(state) => state.is_accepting_input(),
        }
    }

    pub fn set_editor_needs_reset(&mut self, needs_reset: bool) {
        match self {
            Naming::None => {}
            Naming::Renaming(state) => state.set_editor_needs_reset(needs_reset),
            Naming::NewNode(state) => state.set_editor_needs_reset(needs_reset),
            Naming::Duplicating(state) => state.set_editor_needs_reset(needs_reset),
        }
    }

    pub fn editor_needs_reset(&self) -> bool {
        match self {
            Naming::None => false,
            Naming::Renaming(rename) => rename.editor_needs_reset(),
            Naming::NewNode(state) => state.editor_needs_reset(),
            Naming::Duplicating(state) => state.editor_needs_reset(),
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
            Naming::NewNode(state) if state.base_path() == path => {
                Some(FileNodeViewData {
                    kind: FileNodeViewKind::Naming {
                        err: state.err().map(ToString::to_string),
                    },
                    is_dir: state.is_dir(),
                    open: false,
                    level: level + 1,
                })
            }
            Naming::Duplicating(state) if state.path() == path => {
                Some(FileNodeViewData {
                    kind: FileNodeViewKind::Duplicating {
                        source: state.path().to_path_buf(),
                        err: state.err().map(ToString::to_string),
                    },
                    is_dir,
                    open: false,
                    level: level + 1,
                })
            }
            _ => None,
        }
    }
}

/// Stores the state of any in progress rename of a path.
///
/// The `editor_needs_reset` field is `true` if the rename editor should have its contents reset
/// when the view function next runs.
#[derive(Debug, Clone)]
pub enum RenameState {
    Renaming {
        /// Original path
        path: PathBuf,
        editor_needs_reset: bool,
    },
    RenameRequestPending {
        /// Original path
        path: PathBuf,
        editor_needs_reset: bool,
    },
    RenameErr {
        /// Original path
        path: PathBuf,
        editor_needs_reset: bool,
        err: String,
    },
}

impl RenameState {
    pub fn is_accepting_input(&self) -> bool {
        match self {
            Self::RenameRequestPending { .. } => false,
            Self::Renaming { .. } | Self::RenameErr { .. } => true,
        }
    }

    pub fn is_err(&self) -> bool {
        match self {
            Self::Renaming { .. } | Self::RenameRequestPending { .. } => false,
            Self::RenameErr { .. } => true,
        }
    }

    pub fn err(&self) -> Option<&str> {
        match self {
            Self::RenameErr { err, .. } => Some(err.as_str()),
            _ => None,
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
        match self {
            Self::Renaming {
                path,
                editor_needs_reset,
            }
            | Self::RenameRequestPending {
                path,
                editor_needs_reset,
            }
            | Self::RenameErr {
                path,
                editor_needs_reset,
                ..
            } => {
                let path = mem::take(path);

                *self = Self::RenameErr {
                    path,
                    editor_needs_reset: *editor_needs_reset,
                    err,
                };
            }
        }
    }

    pub fn set_editor_needs_reset(&mut self, needs_reset: bool) {
        match self {
            Self::Renaming {
                editor_needs_reset, ..
            }
            | Self::RenameRequestPending {
                editor_needs_reset, ..
            }
            | Self::RenameErr {
                editor_needs_reset, ..
            } => {
                *editor_needs_reset = needs_reset;
            }
        }
    }

    /// Get the path for the target file we're renaming
    pub fn path(&self) -> &Path {
        match self {
            Self::Renaming { path, .. }
            | Self::RenameRequestPending { path, .. }
            | Self::RenameErr { path, .. } => path,
        }
    }

    pub fn editor_needs_reset(&self) -> bool {
        match self {
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

#[derive(Debug, Clone)]
pub enum NewNodeState {
    /// We are naming the uncreated node
    Naming {
        /// If true, then we are creating a directory
        is_dir: bool,
        /// The folder that the file/directory is being created within
        base_path: PathBuf,
        editor_needs_reset: bool,
    },
    /// We are waiting for the node to be created
    CreationPending {
        base_path: PathBuf,
        is_dir: bool,
        editor_needs_reset: bool,
    },
    /// There is or would be an error in creating the node there
    Err {
        base_path: PathBuf,
        is_dir: bool,
        editor_needs_reset: bool,
        err: String,
    },
}
impl NewNodeState {
    pub fn is_dir(&self) -> bool {
        match self {
            Self::Naming { is_dir, .. }
            | Self::CreationPending { is_dir, .. }
            | Self::Err { is_dir, .. } => *is_dir,
        }
    }

    pub fn is_accepting_input(&self) -> bool {
        match self {
            Self::Naming { .. } | Self::Err { .. } => true,
            Self::CreationPending { .. } => false,
        }
    }

    pub fn is_err(&self) -> bool {
        match self {
            Self::Naming { .. } | Self::CreationPending { .. } => false,
            Self::Err { .. } => true,
        }
    }

    pub fn err(&self) -> Option<&str> {
        match self {
            Self::Err { err, .. } => Some(err.as_str()),
            _ => None,
        }
    }

    pub fn set_ok(&mut self) {
        if let &mut Self::Err {
            ref mut base_path,
            is_dir,
            editor_needs_reset,
            ..
        } = self
        {
            let base_path = mem::take(base_path);

            *self = Self::Naming {
                is_dir,
                base_path,
                editor_needs_reset,
            };
        }
    }

    pub fn set_pending(&mut self) {
        if let &mut Self::Naming {
            ref mut base_path,
            is_dir,
            editor_needs_reset,
        } = self
        {
            let base_path = mem::take(base_path);

            *self = Self::CreationPending {
                base_path,
                is_dir,
                editor_needs_reset,
            };
        }
    }

    pub fn set_err(&mut self, err: String) {
        match self {
            Self::Naming {
                base_path,
                is_dir,
                editor_needs_reset,
            }
            | Self::CreationPending {
                base_path,
                is_dir,
                editor_needs_reset,
            }
            | Self::Err {
                base_path,
                is_dir,
                editor_needs_reset,
                ..
            } => {
                let base_path = mem::take(base_path);

                *self = Self::Err {
                    base_path,
                    is_dir: *is_dir,
                    editor_needs_reset: *editor_needs_reset,
                    err,
                };
            }
        }
    }

    pub fn base_path(&self) -> &Path {
        match self {
            Self::Naming { base_path, .. }
            | Self::CreationPending { base_path, .. }
            | Self::Err { base_path, .. } => base_path,
        }
    }

    pub fn set_editor_needs_reset(&mut self, needs_reset: bool) {
        match self {
            Self::Naming {
                editor_needs_reset, ..
            }
            | Self::CreationPending {
                editor_needs_reset, ..
            }
            | Self::Err {
                editor_needs_reset, ..
            } => {
                *editor_needs_reset = needs_reset;
            }
        }
    }

    pub fn editor_needs_reset(&self) -> bool {
        match self {
            &Self::Naming {
                editor_needs_reset, ..
            }
            | &Self::CreationPending {
                editor_needs_reset, ..
            }
            | &Self::Err {
                editor_needs_reset, ..
            } => editor_needs_reset,
        }
    }
}

/// State for duplicating a file/directory in the same folder
#[derive(Debug, Clone)]
pub enum DuplicateState {
    /// We are naming the uncreated node
    Naming {
        /// Path to the item being duplicated
        path: PathBuf,
        editor_needs_reset: bool,
    },
    /// We are waiting for the node to be created
    CreationPending {
        /// Path to the item being duplicated
        path: PathBuf,
        editor_needs_reset: bool,
    },
    /// There is or would be an error in creating the node there
    Err {
        /// Path to the item being duplicated
        path: PathBuf,
        editor_needs_reset: bool,
        err: String,
    },
}
impl DuplicateState {
    pub fn is_accepting_input(&self) -> bool {
        match self {
            Self::Naming { .. } | Self::Err { .. } => true,
            Self::CreationPending { .. } => false,
        }
    }

    pub fn is_err(&self) -> bool {
        match self {
            Self::Naming { .. } | Self::CreationPending { .. } => false,
            Self::Err { .. } => true,
        }
    }

    pub fn err(&self) -> Option<&str> {
        match self {
            Self::Err { err, .. } => Some(err.as_str()),
            _ => None,
        }
    }

    pub fn set_ok(&mut self) {
        if let &mut Self::Err {
            ref mut path,
            editor_needs_reset,
            ..
        } = self
        {
            let path = mem::take(path);

            *self = Self::Naming {
                path,
                editor_needs_reset,
            };
        }
    }

    pub fn set_pending(&mut self) {
        if let &mut Self::Naming {
            ref mut path,
            editor_needs_reset,
        } = self
        {
            let path = mem::take(path);

            *self = Self::CreationPending {
                path,
                editor_needs_reset,
            };
        }
    }

    pub fn set_err(&mut self, err: String) {
        match self {
            Self::Naming {
                path,
                editor_needs_reset,
            }
            | Self::CreationPending {
                path,
                editor_needs_reset,
            }
            | Self::Err {
                path,
                editor_needs_reset,
                ..
            } => {
                let path = mem::take(path);

                *self = Self::Err {
                    path,
                    editor_needs_reset: *editor_needs_reset,
                    err,
                };
            }
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Naming { path, .. }
            | Self::CreationPending { path, .. }
            | Self::Err { path, .. } => path,
        }
    }

    pub fn set_editor_needs_reset(&mut self, needs_reset: bool) {
        match self {
            Self::Naming {
                editor_needs_reset, ..
            }
            | Self::CreationPending {
                editor_needs_reset, ..
            }
            | Self::Err {
                editor_needs_reset, ..
            } => {
                *editor_needs_reset = needs_reset;
            }
        }
    }

    pub fn editor_needs_reset(&self) -> bool {
        match self {
            &Self::Naming {
                editor_needs_reset, ..
            }
            | &Self::CreationPending {
                editor_needs_reset, ..
            }
            | &Self::Err {
                editor_needs_reset, ..
            } => editor_needs_reset,
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
            Self::Path(path)
            | Self::Renaming { path, .. }
            | Self::Duplicating { source: path, .. } => Some(path),
            Self::Naming { .. } => None,
        }
    }
}

#[derive(Debug)]
pub struct FileNodeViewData {
    pub kind: FileNodeViewKind,
    pub is_dir: bool,
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

        let mut i = current;
        if current >= min {
            let kind = if let Naming::Renaming(state) = &naming {
                if state.path() == self.path {
                    FileNodeViewKind::Renaming {
                        path: self.path.clone(),
                        err: state.err().map(ToString::to_string),
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
                open: self.open,
                level,
            });
        }

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

        if i >= min {
            if let Some(node) = naming_extra {
                view_items.push(node);
                i += 1;
            }
        }

        i
    }
}
