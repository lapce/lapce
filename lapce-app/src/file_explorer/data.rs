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
    EventPropagation,
};
use lapce_core::{
    command::{EditCommand, FocusCommand},
    mode::Mode,
};
use lapce_rpc::{
    file::{FileNodeItem, RenameState},
    proxy::ProxyResponse,
};

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand, LapceCommand},
    editor::EditorData,
    id::EditorId,
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
    pub id: RwSignal<usize>,
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
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
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
        let rename_editor_data =
            EditorData::new_local(cx, EditorId::next(), common.clone());
        let data = Self {
            id: cx.create_rw_signal(0),
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

    pub fn reload(&self) {
        let path = self.root.with_untracked(|root| root.path.clone());
        self.read_dir(&path);
    }

    pub fn toggle_expand(&self, path: &Path) {
        self.id.update(|id| {
            *id += 1;
        });
        if let Some(read) = self
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
        {
            if !read {
                self.read_dir(path);
            }
        }
    }

    pub fn read_dir(&self, path: &Path) {
        let root = self.root;
        let id = self.id;
        let data = self.clone();
        let send = {
            let path = path.to_path_buf();
            create_ext_action(self.common.scope, move |result| {
                if let Ok(ProxyResponse::ReadDirResponse { items }) = result {
                    id.update(|id| {
                        *id += 1;
                    });
                    root.update(|root| {
                        if let Some(node) = root.get_file_node_mut(&path) {
                            node.read = true;
                            let removed_paths: Vec<PathBuf> = node
                                .children
                                .keys()
                                .filter(|p| !items.iter().any(|i| &&i.path == p))
                                .map(PathBuf::from)
                                .collect();
                            for path in removed_paths {
                                node.children.remove(&path);
                            }

                            for item in items {
                                if let Some(existing) = node.children.get(&item.path)
                                {
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
                }
            })
        };
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
                let new_relative_path = self.rename_editor_data.view.text();

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
        let menu = {
            let common = self.common.clone();
            let path = path.to_owned();
            let left_path = path.clone();
            let left_diff_path = self.left_diff_path;

            Menu::new("")
                .entry(MenuItem::new("Rename...").action(move || {
                    common
                        .internal_command
                        .send(InternalCommand::StartRenamePath {
                            path: path.clone(),
                        });
                }))
                .entry(
                    MenuItem::new("Select for Compare")
                        .action(move || left_diff_path.set(Some(left_path.clone()))),
                )
        };

        let menu = if let Some(left_path) = self.left_diff_path.get_untracked() {
            let common = self.common.clone();
            let right_path = path.to_owned();

            menu.entry(MenuItem::new("Compare with Selected").action(move || {
                common
                    .internal_command
                    .send(InternalCommand::OpenDiffFiles {
                        left_path: left_path.clone(),
                        right_path: right_path.clone(),
                    })
            }))
        } else {
            menu
        };

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
