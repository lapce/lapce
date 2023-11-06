use std::{
    collections::HashMap,
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
use lapce_rpc::{file::FileNodeItem, proxy::ProxyResponse};

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand, LapceCommand},
    editor::EditorData,
    id::EditorId,
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct FileExplorerData {
    pub id: RwSignal<usize>,
    pub root: RwSignal<FileNodeItem>,
    pub rename_path: RwSignal<Option<PathBuf>>,
    pub rename_editor_data: EditorData,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for FileExplorerData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        self.rename_path
            .with_untracked(|rename_path| rename_path.is_some())
            && condition == Condition::ModalFocus
    }

    fn run_command(
        &self,
        command: &LapceCommand,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> CommandExecuted {
        if self
            .rename_path
            .with_untracked(|rename_path| rename_path.is_some())
        {
            match command.kind {
                CommandKind::Focus(FocusCommand::ModalClose) => {
                    self.cancel_rename();
                    CommandExecuted::Yes
                }
                CommandKind::Edit(EditCommand::InsertNewLine) => {
                    let new_relative_path: String =
                        self.rename_editor_data.view.text().into();
                    self.finish_rename(new_relative_path.as_ref());
                    CommandExecuted::Yes
                }
                _ => self.rename_editor_data.run_command(command, count, mods),
            }
        } else {
            CommandExecuted::No
        }
    }

    fn receive_char(&self, c: &str) {
        if self
            .rename_path
            .with_untracked(|rename_path| rename_path.is_some())
        {
            self.rename_editor_data.receive_char(c);
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
        let rename_path = cx.create_rw_signal(None);
        let rename_editor_data =
            EditorData::new_local(cx, EditorId::next(), common.clone());
        let data = Self {
            id: cx.create_rw_signal(0),
            root,
            rename_path,
            rename_editor_data,
            common,
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

    /// Closes the rename text box and sends a request to perform the rename.
    ///
    /// `new_relative_path` is the path the item is to be moved to relative to the item's current
    /// parent directory.
    ///
    /// Currently the text box is closed unconditionally, and if the rename fails no error will be
    /// reported. The name of the item in the file explorer is only updated if the rename succeeds.
    pub fn finish_rename(&self, new_relative_path: &Path) {
        if let Some(current_path) = self.rename_path.get() {
            let parent = current_path.parent().unwrap_or("".as_ref());
            let new_path = parent.join(new_relative_path);

            self.common
                .internal_command
                .send(InternalCommand::FinishRenameFile {
                    current_path,
                    new_path,
                })
        }
    }

    /// Closes the rename text box without renaming the item.
    pub fn cancel_rename(&self) {
        self.rename_path.set(None);
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
        let path = path.to_owned();
        let common = self.common.clone();

        let menu =
            Menu::new("").entry(MenuItem::new("Rename...").action(move || {
                common
                    .internal_command
                    .send(InternalCommand::StartRenameFile { path: path.clone() });
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
