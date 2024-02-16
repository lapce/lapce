use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    rc::Rc,
};

use floem::{
    action::show_context_menu,
    ext_event::create_ext_action,
    keyboard::ModifiersState,
    menu::{Menu, MenuItem},
    reactive::{RwSignal, Scope},
    views::editor::{id::EditorId, text::SystemClipboard},
    EventPropagation,
};
use globset::Glob;
use lapce_core::{
    command::{EditCommand, FocusCommand},
    mode::Mode,
    register::Clipboard,
};
use lapce_rpc::{
    file::{FileNodeItem, RenameState},
    proxy::ProxyResponse,
};

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand, LapceCommand},
    editor::EditorData,
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::CommonData,
};

enum RenamedPath {
    NotRenaming,
    NameUnchanged,
    Renamed {
        current_path: PathBuf,
        new_path: PathBuf,
    },
}

#[derive(Clone)]
pub struct FileExplorerData {
    pub root: RwSignal<FileNodeItem>,
    pub rename_state: RwSignal<RenameState>,
    pub rename_editor_data: EditorData,
    pub common: Rc<CommonData>,
    left_diff_path: RwSignal<Option<PathBuf>>,
}

impl KeyPressFocus for FileExplorerData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        self.rename_state
            .with_untracked(RenameState::is_accepting_input)
            && condition == Condition::ModalFocus
    }

    fn run_command(
        &self,
        command: &LapceCommand,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> CommandExecuted {
        if self
            .rename_state
            .with_untracked(RenameState::is_accepting_input)
        {
            match command.kind {
                CommandKind::Focus(FocusCommand::ModalClose) => {
                    self.cancel_rename();
                    CommandExecuted::Yes
                }
                CommandKind::Edit(EditCommand::InsertNewLine) => {
                    self.finish_rename();
                    CommandExecuted::Yes
                }
                CommandKind::Edit(_) => {
                    let command_executed =
                        self.rename_editor_data.run_command(command, count, mods);

                    if let RenamedPath::Renamed { new_path, .. } =
                        self.renamed_path()
                    {
                        self.common
                            .internal_command
                            .send(InternalCommand::TestRenamePath { new_path });
                    }

                    command_executed
                }
                _ => self.rename_editor_data.run_command(command, count, mods),
            }
        } else {
            CommandExecuted::No
        }
    }

    fn receive_char(&self, c: &str) {
        if self
            .rename_state
            .with_untracked(RenameState::is_accepting_input)
        {
            self.rename_editor_data.receive_char(c);

            if let RenamedPath::Renamed { new_path, .. } = self.renamed_path() {
                self.common
                    .internal_command
                    .send(InternalCommand::TestRenamePath { new_path });
            }
        }
    }
}

impl FileExplorerData {
    pub fn new(
        cx: Scope,
        editors: RwSignal<im::HashMap<EditorId, Rc<EditorData>>>,
        common: Rc<CommonData>,
    ) -> Self {
        let path = common.workspace.path.clone().unwrap_or_default();
        let root = cx.create_rw_signal(FileNodeItem {
            path: path.clone(),
            is_dir: true,
            read: false,
            open: false,
            children: HashMap::new(),
            children_open_count: 0,
        });
        let rename_state = cx.create_rw_signal(RenameState::NotRenaming);
        let rename_editor_data = EditorData::new_local(cx, editors, common.clone());
        let data = Self {
            root,
            rename_state,
            rename_editor_data,
            common,
            left_diff_path: cx.create_rw_signal(None),
        };
        if data.common.workspace.path.is_some() {
            // only fill in the child files if there is open folder
            data.toggle_expand(&path);
        }
        data
    }

    /// Reload the file explorer data via reading the root directory.  
    /// Note that this will not update immediately.
    pub fn reload(&self) {
        let path = self.root.with_untracked(|root| root.path.clone());
        self.read_dir(&path);
    }

    /// Toggle whether the directory is expanded or not.  
    /// Does nothing if the path does not exist or is not a directory.
    pub fn toggle_expand(&self, path: &Path) {
        let Some(read) = self
            .root
            .try_update(|root| {
                let read = if let Some(node) = root.get_file_node_mut(path) {
                    if !node.is_dir {
                        return None;
                    }
                    node.open = !node.open;
                    Some(node.read)
                } else {
                    None
                };

                if Some(true) == read {
                    root.update_node_count_recursive(path);
                }
                read
            })
            .unwrap()
        else {
            return;
        };

        // Read the directory's files if they haven't been read yet
        if !read {
            self.read_dir(path);
        }
    }

    pub fn read_dir(&self, path: &Path) {
        self.read_dir_cb(path, |_| {});
    }

    /// Read the directory's information and update the file explorer tree.  
    /// `done : FnOnce(was_read: bool)` is called when the operation is completed, whether success,
    /// failure, or ignored.
    pub fn read_dir_cb(&self, path: &Path, done: impl FnOnce(bool) + 'static) {
        let root = self.root;
        let data = self.clone();
        let config = self.common.config;
        let send = {
            let path = path.to_path_buf();
            create_ext_action(self.common.scope, move |result| {
                let Ok(ProxyResponse::ReadDirResponse { mut items }) = result else {
                    done(false);
                    return;
                };

                root.update(|root| {
                    // Get the node for this path, which should already exist if we're calling
                    // read_dir on it.
                    if let Some(node) = root.get_file_node_mut(&path) {
                        // TODO: do not recreate glob every time we read a directory
                        // Retain only items that are not excluded from view by the configuration
                        match Glob::new(&config.get().editor.files_exclude) {
                            Ok(glob) => {
                                let matcher = glob.compile_matcher();
                                items.retain(|i| !matcher.is_match(&i.path));
                            }
                            Err(e) => tracing::error!(
                                target:"files_exclude",
                                "Failed to compile glob: {}",
                                e
                            ),
                        }

                        node.read = true;

                        // Remove paths that no longer exist
                        let removed_paths: Vec<PathBuf> = node
                            .children
                            .keys()
                            .filter(|p| !items.iter().any(|i| &&i.path == p))
                            .map(PathBuf::from)
                            .collect();
                        for path in removed_paths {
                            node.children.remove(&path);
                        }

                        // Reread dirs that were already read and add new paths
                        for item in items {
                            if let Some(existing) = node.children.get(&item.path) {
                                if existing.read {
                                    data.read_dir(&existing.path);
                                }
                            } else {
                                node.children.insert(item.path.clone(), item);
                            }
                        }
                    }
                    root.update_node_count_recursive(&path);
                });

                done(true);
            })
        };

        // Ask the proxy for the directory information
        self.common
            .proxy
            .read_dir(path.to_path_buf(), move |result| {
                send(result);
            });
    }

    /// Returns `true` if `path` exists in the file explorer tree and is a directory, `false`
    /// otherwise.
    fn is_dir(&self, path: &Path) -> bool {
        self.root.with_untracked(|root| {
            root.get_file_node(path).is_some_and(|node| node.is_dir)
        })
    }

    /// If there is an in progress rename and the user has entered a path that differs from the
    /// current path, gets the current and new paths of the renamed node.
    fn renamed_path(&self) -> RenamedPath {
        self.rename_state.with_untracked(|rename_state| {
            if let Some(current_path) = rename_state.path() {
                let current_file_name = current_path.file_name().unwrap_or_default();
                // `new_relative_path` is the new path relative to the parent directory, unless the
                // user has entered an absolute path.
                let new_relative_path = self.rename_editor_data.text();

                let new_relative_path: Cow<OsStr> =
                    match new_relative_path.slice_to_cow(..) {
                        Cow::Borrowed(path) => Cow::Borrowed(path.as_ref()),
                        Cow::Owned(path) => Cow::Owned(path.into()),
                    };

                if new_relative_path == current_file_name {
                    RenamedPath::NameUnchanged
                } else {
                    let new_path = current_path
                        .parent()
                        .unwrap_or("".as_ref())
                        .join(new_relative_path);

                    RenamedPath::Renamed {
                        current_path: current_path.to_owned(),
                        new_path,
                    }
                }
            } else {
                RenamedPath::NotRenaming
            }
        })
    }

    /// If a rename is in progress and the user has entered a path that differs from the current
    /// path, sends a request to perform the rename.
    pub fn finish_rename(&self) {
        match self.renamed_path() {
            RenamedPath::NotRenaming => (),
            RenamedPath::NameUnchanged => self.cancel_rename(),
            RenamedPath::Renamed {
                current_path,
                new_path,
            } => self.common.internal_command.send(
                InternalCommand::FinishRenamePath {
                    current_path,
                    new_path,
                },
            ),
        }
    }

    /// Closes the rename text box without renaming the item.
    pub fn cancel_rename(&self) {
        self.rename_state.set(RenameState::NotRenaming);
    }

    pub fn click(&self, path: &Path) {
        if self.is_dir(path) {
            self.toggle_expand(path);
        } else {
            self.common
                .internal_command
                .send(InternalCommand::OpenFile {
                    path: path.to_path_buf(),
                })
        }
    }

    pub fn double_click(&self, path: &Path) -> EventPropagation {
        if self.is_dir(path) {
            EventPropagation::Continue
        } else {
            self.common
                .internal_command
                .send(InternalCommand::MakeConfirmed);
            EventPropagation::Stop
        }
    }

    pub fn secondary_click(&self, path: &Path) {
        let common = self.common.clone();
        let path_a = path.to_owned();
        let left_diff_path = self.left_diff_path;
        // TODO: should we just pass is_dir into secondary click?
        let is_dir = self.is_dir(path);
        let is_workspace = self
            .common
            .workspace
            .path
            .as_ref()
            .map_or(false, |p| path == p);

        let mut menu = Menu::new("");

        let path = path_a.clone();
        let data = self.clone();
        menu = menu.entry(MenuItem::new("New File").action(move || {
            let path_b = path.clone();
            data.read_dir_cb(&path, move |was_read| {
                if !was_read {
                    tracing::warn!(
                        "Failed to read directory, avoiding creating file: {:?}",
                        path_b
                    );
                    return;
                }
            });
        }));

        let path = path_a.clone();
        menu = menu.entry(MenuItem::new("New Directory").action(move || {
            println!("New Folder");
        }));

        menu = menu.separator();

        // TODO: there are situations where we can open the file explorer to remote files
        if !common.workspace.kind.is_remote() {
            let path = path_a.clone();
            menu = menu.entry(MenuItem::new("Reveal in file explorer").action(
                move || {
                    let path = path.parent().unwrap_or(&path);
                    if !path.exists() {
                        return;
                    }

                    if let Err(err) = open::that(path) {
                        tracing::error!(
                            "Failed to reveal file in system file explorer: {}",
                            err
                        );
                    }
                },
            ));
        }

        if !is_workspace {
            let path = path_a.clone();
            let internal_command = common.internal_command;
            menu = menu.entry(MenuItem::new("Rename").action(move || {
                internal_command
                    .send(InternalCommand::StartRenamePath { path: path.clone() });
            }));

            let path = path_a.clone();
            menu = menu.entry(MenuItem::new("Duplicate").action(move || {
                println!("Duplicate");
            }));

            // TODO: it is common for shift+right click to make 'Move file to trash' an actual
            // Delete, which can be useful for large files.
            let path = path_a.clone();
            let proxy = common.proxy.clone();
            let trash_text = if is_dir {
                "Move Directory to Trash"
            } else {
                "Move File to Trash"
            };
            menu = menu.entry(MenuItem::new(trash_text).action(move || {
                proxy.trash_path(path.clone(), |res| {
                    if let Err(err) = res {
                        tracing::warn!("Failed to trash path: {:?}", err);
                    }
                })
            }));
        }

        menu = menu.separator();

        let path = path_a.clone();
        menu = menu.entry(MenuItem::new("Copy Path").action(move || {
            let mut clipboard = SystemClipboard::new();
            clipboard.put_string(path.to_string_lossy());
        }));

        let path = path_a.clone();
        let workspace = common.workspace.clone();
        menu = menu.entry(MenuItem::new("Copy Relative Path").action(move || {
            let relative_path = if let Some(workspace_path) = &workspace.path {
                path.strip_prefix(workspace_path).unwrap_or(&path)
            } else {
                path.as_ref()
            };

            let mut clipboard = SystemClipboard::new();
            clipboard.put_string(relative_path.to_string_lossy());
        }));

        menu = menu.separator();

        let path = path_a.clone();
        menu = menu.entry(
            MenuItem::new("Select for Compare")
                .action(move || left_diff_path.set(Some(path.clone()))),
        );

        if let Some(left_path) = self.left_diff_path.get_untracked() {
            let common = self.common.clone();
            let right_path = path_a.to_owned();

            menu = menu.entry(MenuItem::new("Compare with Selected").action(
                move || {
                    common
                        .internal_command
                        .send(InternalCommand::OpenDiffFiles {
                            left_path: left_path.clone(),
                            right_path: right_path.clone(),
                        })
                },
            ))
        }

        menu = menu.separator();

        let internal_command = common.internal_command;
        menu = menu.entry(MenuItem::new("Refresh").action(move || {
            internal_command.send(InternalCommand::ReloadFileExplorer);
        }));

        show_context_menu(menu, None);
    }

    pub fn middle_click(&self, path: &Path) -> EventPropagation {
        if self.is_dir(path) {
            EventPropagation::Continue
        } else {
            self.common
                .internal_command
                .send(InternalCommand::OpenFileInNewTab {
                    path: path.to_path_buf(),
                });
            EventPropagation::Stop
        }
    }
}
