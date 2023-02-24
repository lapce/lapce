use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use floem::{
    app::AppContext,
    ext_event::create_ext_action,
    glazier::KeyEvent,
    peniko::kurbo::Rect,
    reactive::{
        create_rw_signal, ReadSignal, RwSignal, UntrackedGettableSignal, WriteSignal,
    },
};
use lapce_core::register::Register;
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};
use lapce_xi_rope::Rope;
use serde::{Deserialize, Serialize};

use crate::{
    command::InternalCommand,
    config::LapceConfig,
    doc::{DocContent, Document},
    editor::EditorData,
    editor_tab::{EditorTabChild, EditorTabData},
    id::{EditorId, EditorTabId, SplitId},
    keypress::{KeyPressData, KeyPressFocus},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitContent {
    EditorTab(EditorTabId),
    Split(SplitId),
}

impl SplitContent {
    pub fn id(&self) -> u64 {
        match self {
            SplitContent::EditorTab(id) => id.to_raw(),
            SplitContent::Split(id) => id.to_raw(),
        }
    }
}

#[derive(Clone)]
pub struct SplitData {
    pub parent_split: Option<SplitId>,
    pub split_id: SplitId,
    pub children: Vec<SplitContent>,
    pub direction: SplitDirection,
}

#[derive(Clone)]
pub struct MainSplitData {
    pub root_split: SplitId,
    pub active_editor_tab: RwSignal<Option<EditorTabId>>,
    pub splits: RwSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    pub editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    pub editors: RwSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    pub docs: RwSignal<im::HashMap<PathBuf, RwSignal<Document>>>,
    pub proxy_rpc: ProxyRpcHandler,
    register: RwSignal<Register>,
    internal_command: WriteSignal<Option<InternalCommand>>,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl MainSplitData {
    pub fn new(
        cx: AppContext,
        proxy_rpc: ProxyRpcHandler,
        register: RwSignal<Register>,
        internal_command: WriteSignal<Option<InternalCommand>>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let root_split = SplitId::next();
        let root_split_data = SplitData {
            parent_split: None,
            split_id: root_split,
            children: Vec::new(),
            direction: SplitDirection::Horizontal,
        };

        let mut splits = im::HashMap::new();
        splits.insert(root_split, create_rw_signal(cx.scope, root_split_data));
        let splits = create_rw_signal(cx.scope, splits);

        let active_editor_tab = create_rw_signal(cx.scope, None);
        let editor_tabs = create_rw_signal(cx.scope, im::HashMap::new());
        let editors = create_rw_signal(cx.scope, im::HashMap::new());
        let docs = create_rw_signal(cx.scope, im::HashMap::new());

        Self {
            root_split,
            splits,
            active_editor_tab,
            editor_tabs,
            editors,
            docs,
            proxy_rpc,
            register,
            internal_command,
            config,
        }
    }

    pub fn key_down(
        &self,
        cx: AppContext,
        key_event: &KeyEvent,
        keypress: &mut KeyPressData,
    ) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
            editor_tabs.get(&active_editor_tab).copied()
        })?;
        let child = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;
        match child {
            EditorTabChild::Editor(editor_id) => {
                let editor = self
                    .editors
                    .with_untracked(|editors| editors.get(&editor_id).copied())?;
                editor.with_untracked(|editor| {
                    keypress.key_down(cx, key_event, editor);
                });
            }
        }
        Some(())
    }

    pub fn open_file(&self, cx: AppContext, path: PathBuf) {
        let doc = self.docs.with_untracked(|docs| docs.get(&path).cloned());
        let doc = if let Some(doc) = doc {
            doc
        } else {
            let doc = Document::new(path.clone(), self.config);
            let buffer_id = doc.buffer_id;
            let doc = create_rw_signal(cx.scope, doc);
            self.docs.update(|docs| {
                docs.insert(path.clone(), doc);
            });

            let set_doc = doc.write_only();
            let send = create_ext_action(cx, move |content| {
                set_doc.update(|doc| {
                    doc.init_content(content);
                });
            });

            self.proxy_rpc
                .new_buffer(buffer_id, path.clone(), move |result| {
                    if let Ok(ProxyResponse::NewBufferResponse { content }) = result
                    {
                        send(Rope::from(content))
                    }
                });

            doc
        };
        self.get_editor_or_new(cx, doc, &path);
    }

    fn get_editor_or_new(
        &self,
        cx: AppContext,
        doc: RwSignal<Document>,
        path: &Path,
    ) -> RwSignal<EditorData> {
        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        let editors = self.editors.get_untracked();
        let splits = self.splits.get_untracked();

        let active_editor_tab = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .cloned();

        // first check if the file exists in active editor tab
        if let Some(editor_tab) = active_editor_tab {
            if let Some(editor) = editor_tab
                .with_untracked(|editor_tab| editor_tab.get_editor(&editors, path))
            {
                return editor;
            }
        }

        // check file exists in non active editor tabs
        for (editor_tab_id, editor_tab) in &editor_tabs {
            if Some(*editor_tab_id) != active_editor_tab_id {
                if let Some(editor) = editor_tab.with_untracked(|editor_tab| {
                    editor_tab.get_editor(&editors, path)
                }) {
                    self.active_editor_tab.set(Some(*editor_tab_id));
                    return editor;
                }
            }
        }

        let editor = if editor_tabs.is_empty() {
            // the main split doens't have anything
            let editor_tab_id = EditorTabId::next();

            // create the new editor
            let editor_id = EditorId::next();
            let editor = EditorData::new(
                cx,
                editor_tab_id,
                editor_id,
                doc,
                self.register,
                self.internal_command,
                self.config,
            );
            let editor = create_rw_signal(cx.scope, editor);
            self.editors.update(|editors| {
                editors.insert(editor_id, editor);
            });

            let editor_tab = EditorTabData {
                split: self.root_split,
                active: 0,
                editor_tab_id,
                children: vec![EditorTabChild::Editor(editor_id)],
                layout_rect: Rect::ZERO,
            };
            let editor_tab = create_rw_signal(cx.scope, editor_tab);
            self.editor_tabs.update(|editor_tabs| {
                editor_tabs.insert(editor_tab_id, editor_tab);
            });
            let root_split = splits.get(&self.root_split).unwrap();
            root_split.update(|root_split| {
                root_split.children = vec![SplitContent::EditorTab(editor_tab_id)];
            });
            self.active_editor_tab.set(Some(editor_tab_id));
            editor
        } else {
            let editor_tab = if let Some(editor_tab) = active_editor_tab {
                editor_tab
            } else {
                let (editor_tab_id, editor_tab) = editor_tabs.iter().next().unwrap();
                self.active_editor_tab.set(Some(*editor_tab_id));
                *editor_tab
            };

            let editor_tab_id =
                editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

            // create the new editor
            let editor_id = EditorId::next();
            let editor = EditorData::new(
                cx,
                editor_tab_id,
                editor_id,
                doc,
                self.register,
                self.internal_command,
                self.config,
            );
            let editor = create_rw_signal(cx.scope, editor);
            self.editors.update(|editors| {
                editors.insert(editor_id, editor);
            });

            println!("add editor tab child");

            editor_tab.update(|editor_tab| {
                let active = editor_tab
                    .active
                    .min(editor_tab.children.len().saturating_sub(1));
                editor_tab
                    .children
                    .insert(active + 1, EditorTabChild::Editor(editor_id));
                editor_tab.active = active + 1;
            });
            editor
        };

        editor
    }

    pub fn split(
        &self,
        cx: AppContext,
        direction: SplitDirection,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        let split_direction = split.with_untracked(|split| split.direction);

        let (index, children_len) = split.with_untracked(|split| {
            split
                .children
                .iter()
                .position(|c| c == &SplitContent::EditorTab(editor_tab_id))
                .map(|index| (index, split.children.len()))
        })?;

        if split_direction == direction {
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(cx, split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
            split.update(|split| {
                split
                    .children
                    .insert(index + 1, SplitContent::EditorTab(new_editor_tab_id));
            });
        } else if children_len == 1 {
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(cx, split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
            split.update(|split| {
                split.direction = direction;
                split
                    .children
                    .push(SplitContent::EditorTab(new_editor_tab_id));
            });
        } else {
            let new_split_id = SplitId::next();

            editor_tab.update(|editor_tab| {
                editor_tab.split = new_split_id;
            });
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(cx, new_split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

            let new_split = SplitData {
                parent_split: Some(split_id),
                split_id: new_split_id,
                children: vec![
                    SplitContent::EditorTab(editor_tab_id),
                    SplitContent::EditorTab(new_editor_tab_id),
                ],
                direction,
            };
            let new_split = create_rw_signal(cx.scope, new_split);
            self.splits.update(|splits| {
                splits.insert(new_split_id, new_split);
            });
            split.update(|split| {
                split.children[index] = SplitContent::Split(new_split_id);
            });
        }

        Some(())
    }

    fn split_editor_tab(
        &self,
        cx: AppContext,
        split_id: SplitId,
        editor_tab: &EditorTabData,
    ) -> Option<RwSignal<EditorTabData>> {
        let child = editor_tab.children.get(editor_tab.active)?;

        let editor_tab_id = EditorTabId::next();

        let new_child = match child {
            EditorTabChild::Editor(editor_id) => {
                let new_editor_id = EditorId::next();
                let mut editor =
                    self.editors.get_untracked().get(editor_id)?.get_untracked();
                editor.cursor =
                    create_rw_signal(cx.scope, editor.cursor.get_untracked());
                editor.editor_tab_id = Some(editor_tab_id);
                editor.editor_id = new_editor_id;
                let editor = create_rw_signal(cx.scope, editor);
                self.editors.update(|editors| {
                    editors.insert(new_editor_id, editor);
                });
                EditorTabChild::Editor(new_editor_id)
            }
        };

        let editor_tab = EditorTabData {
            split: split_id,
            editor_tab_id,
            active: 0,
            children: vec![new_child],
            layout_rect: Rect::ZERO,
        };
        let editor_tab = create_rw_signal(cx.scope, editor_tab);
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab);
        });
        Some(editor_tab)
    }

    pub fn split_move(
        &self,
        cx: AppContext,
        direction: SplitMoveDirection,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let rect = editor_tab.with_untracked(|editor_tab| editor_tab.layout_rect);

        match direction {
            SplitMoveDirection::Up => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| e.layout_rect);
                    if (current_rect.y1 - rect.y0).abs() < 3.0
                        && current_rect.x0 <= rect.x0
                        && rect.x0 < current_rect.x1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Down => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| e.layout_rect);
                    if (current_rect.y0 - rect.y1).abs() < 3.0
                        && current_rect.x0 <= rect.x0
                        && rect.x0 < current_rect.x1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Right => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| e.layout_rect);
                    if (rect.x1 - current_rect.x0).abs() < 3.0
                        && current_rect.y0 <= rect.y0
                        && rect.y0 < current_rect.y1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Left => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| e.layout_rect);
                    if (current_rect.x1 - rect.x0).abs() < 3.0
                        && current_rect.y0 <= rect.y0
                        && rect.y0 < current_rect.y1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
        }

        Some(())
    }

    pub fn split_exchange(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;

        split.update(|split| {
            let index = split
                .children
                .iter()
                .position(|c| c == &SplitContent::EditorTab(editor_tab_id));
            if let Some(index) = index {
                if index < split.children.len() - 1 {
                    split.children.swap(index, index + 1);
                }
                self.split_content_focus(cx, &split.children[index]);
            }
        });

        Some(())
    }

    fn split_content_focus(&self, cx: AppContext, content: &SplitContent) {
        match content {
            SplitContent::EditorTab(editor_tab_id) => {
                self.active_editor_tab.set(Some(*editor_tab_id));
            }
            SplitContent::Split(split_id) => {
                self.split_focus(cx, *split_id);
            }
        }
    }

    fn split_focus(&self, cx: AppContext, split_id: SplitId) -> Option<()> {
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;

        let split_chilren = split.with_untracked(|split| split.children.clone());
        let content = split_chilren.get(0)?;
        self.split_content_focus(cx, content);

        Some(())
    }

    fn split_remove(&self, cx: AppContext, split_id: SplitId) -> Option<()> {
        if split_id == self.root_split {
            return Some(());
        }

        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        self.splits.update(|splits| {
            splits.remove(&split_id);
        });
        let parent_split_id = split.with_untracked(|split| split.parent_split)?;
        let parent_split = splits.get(&parent_split_id).copied()?;

        let split_len = parent_split
            .update_returning(|split| {
                split
                    .children
                    .retain(|c| c != &SplitContent::Split(split_id));
                split.children.len()
            })
            .unwrap();
        if split_len == 0 {
            self.split_remove(cx, split_id);
        }

        Some(())
    }

    fn editor_tab_remove(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.remove(&editor_tab_id);
        });

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        let parent_split_id = split.with_untracked(|split| split.parent_split);

        let is_active = self
            .active_editor_tab
            .with_untracked(|active| active == &Some(editor_tab_id));

        let index = split.with_untracked(|split| {
            split
                .children
                .iter()
                .position(|c| c == &SplitContent::EditorTab(editor_tab_id))
        })?;
        split.update(|split| {
            split.children.remove(index);
        });
        let split_children = split.with_untracked(|split| split.children.clone());

        if is_active {
            if split_children.is_empty() {
                let new_focus = parent_split_id
                    .and_then(|split_id| splits.get(&split_id))
                    .and_then(|split| {
                        let index = split.with_untracked(|split| {
                            split
                                .children
                                .iter()
                                .position(|c| c == &SplitContent::Split(split_id))
                        })?;
                        Some((index, split))
                    })
                    .and_then(|(index, split)| {
                        split.with_untracked(|split| {
                            if index == split.children.len() - 1 {
                                if index > 0 {
                                    Some(split.children[index - 1])
                                } else {
                                    None
                                }
                            } else {
                                Some(split.children[index + 1])
                            }
                        })
                    });
                if let Some(content) = new_focus {
                    self.split_content_focus(cx, &content);
                }
            } else {
                let content = split_children[index.min(split_children.len() - 1)];
                self.split_content_focus(cx, &content);
            }
        }

        if split_children.is_empty() {
            self.split_remove(cx, split_id);
        }

        Some(())
    }

    pub fn editor_tab_close(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
        let editor_tab = editor_tab.get_untracked();
        for child in editor_tab.children {
            self.editor_tab_child_close(cx, editor_tab_id, child);
        }

        Some(())
    }

    pub fn editor_tab_child_close(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
        child: EditorTabChild,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let index = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.iter().position(|c| c == &child)
        })?;

        let editor_tab_children_len = editor_tab
            .update_returning(|editor_tab| {
                editor_tab.children.remove(index);
                editor_tab.active =
                    index.min(editor_tab.children.len().saturating_sub(1));
                editor_tab.children.len()
            })
            .unwrap();

        match child {
            EditorTabChild::Editor(editor_id) => {
                self.editors.update(|editors| {
                    editors.remove(&editor_id);
                });
            }
        }

        if editor_tab_children_len == 0 {
            self.editor_tab_remove(cx, editor_tab_id);
        }

        Some(())
    }
}
