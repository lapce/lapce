use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use floem::{
    ext_event::create_ext_action,
    glazier::KeyEvent,
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{create_effect, Memo, RwSignal, Scope},
};
use itertools::Itertools;
use lapce_core::{
    buffer::rope_text::RopeText, cursor::Cursor, selection::Selection,
};
use lapce_rpc::{plugin::PluginId, proxy::ProxyResponse};
use lapce_xi_rope::Rope;
use lsp_types::{
    CodeAction, CodeActionOrCommand, DiagnosticSeverity, DocumentChangeOperation,
    DocumentChanges, OneOf, Position, TextEdit, Url, WorkspaceEdit,
};
use serde::{Deserialize, Serialize};

use crate::{
    doc::{DiagnosticData, DocContent, DocHistory, Document, EditorDiagnostic},
    editor::{
        diff::DiffEditorData,
        location::{EditorLocation, EditorPosition},
        EditorData,
    },
    editor_tab::{
        EditorTabChild, EditorTabChildSource, EditorTabData, EditorTabInfo,
    },
    id::{DiffEditorId, EditorId, EditorTabId, SettingsId, SplitId},
    keypress::KeyPressData,
    window_tab::{CommonData, Focus, WindowTabData},
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

    pub fn content_info(&self, data: &WindowTabData) -> SplitContentInfo {
        match &self {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data = data
                    .main_split
                    .editor_tabs
                    .get_untracked()
                    .get(editor_tab_id)
                    .cloned()
                    .unwrap();
                SplitContentInfo::EditorTab(
                    editor_tab_data.get_untracked().tab_info(data),
                )
            }
            SplitContent::Split(split_id) => {
                let split_data = data
                    .main_split
                    .splits
                    .get_untracked()
                    .get(split_id)
                    .cloned()
                    .unwrap();
                SplitContentInfo::Split(split_data.get_untracked().split_info(data))
            }
        }
    }
}

#[derive(Clone)]
pub struct SplitData {
    pub scope: Scope,
    pub parent_split: Option<SplitId>,
    pub split_id: SplitId,
    pub children: Vec<SplitContent>,
    pub direction: SplitDirection,
    pub window_origin: Point,
    pub layout_rect: Rect,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SplitInfo {
    pub children: Vec<SplitContentInfo>,
    pub direction: SplitDirection,
}

impl SplitInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        parent_split: Option<SplitId>,
        split_id: SplitId,
    ) -> RwSignal<SplitData> {
        let split_data = {
            let cx = data.scope.create_child();
            let split_data = SplitData {
                scope: cx,
                split_id,
                direction: self.direction,
                parent_split,
                children: self
                    .children
                    .iter()
                    .map(|child| child.to_data(data.clone(), split_id))
                    .collect(),
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
            };
            cx.create_rw_signal(split_data)
        };
        data.splits.update(|splits| {
            splits.insert(split_id, split_data);
        });
        split_data
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SplitContentInfo {
    EditorTab(EditorTabInfo),
    Split(SplitInfo),
}

impl SplitContentInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        parent_split: SplitId,
    ) -> SplitContent {
        match &self {
            SplitContentInfo::EditorTab(tab_info) => {
                let tab_data = tab_info.to_data(data, parent_split);
                SplitContent::EditorTab(
                    tab_data.with_untracked(|tab_data| tab_data.editor_tab_id),
                )
            }
            SplitContentInfo::Split(split_info) => {
                let split_id = SplitId::next();
                split_info.to_data(data, Some(parent_split), split_id);
                SplitContent::Split(split_id)
            }
        }
    }
}

impl SplitData {
    pub fn split_info(&self, data: &WindowTabData) -> SplitInfo {
        let info = SplitInfo {
            direction: self.direction,
            children: self
                .children
                .iter()
                .map(|child| child.content_info(data))
                .collect(),
        };
        info
    }
}

#[derive(Clone)]
pub struct MainSplitData {
    pub scope: Scope,
    pub root_split: SplitId,
    pub active_editor_tab: RwSignal<Option<EditorTabId>>,
    pub splits: RwSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    pub editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    pub editors: RwSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    pub diff_editors: RwSignal<im::HashMap<DiffEditorId, DiffEditorData>>,
    pub docs: RwSignal<im::HashMap<PathBuf, RwSignal<Document>>>,
    pub diagnostics: RwSignal<im::HashMap<PathBuf, DiagnosticData>>,
    pub active_editor: Memo<Option<RwSignal<EditorData>>>,
    pub find_editor: EditorData,
    pub replace_editor: EditorData,
    pub locations: RwSignal<im::Vector<EditorLocation>>,
    pub current_location: RwSignal<usize>,
    pub common: CommonData,
}

impl MainSplitData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        let splits = cx.create_rw_signal(im::HashMap::new());
        let active_editor_tab = cx.create_rw_signal(None);
        let editor_tabs: RwSignal<
            im::HashMap<EditorTabId, RwSignal<EditorTabData>>,
        > = cx.create_rw_signal(im::HashMap::new());
        let editors = cx.create_rw_signal(im::HashMap::new());
        let diff_editors: RwSignal<im::HashMap<DiffEditorId, DiffEditorData>> =
            cx.create_rw_signal(im::HashMap::new());
        let docs = cx.create_rw_signal(im::HashMap::new());
        let locations = cx.create_rw_signal(im::Vector::new());
        let current_location = cx.create_rw_signal(0);
        let diagnostics = cx.create_rw_signal(im::HashMap::new());
        let find_editor =
            EditorData::new_local(cx, EditorId::next(), common.clone());
        let replace_editor =
            EditorData::new_local(cx, EditorId::next(), common.clone());

        let active_editor =
            cx.create_memo(move |_| -> Option<RwSignal<EditorData>> {
                let active_editor_tab = active_editor_tab.get()?;
                let editor_tab = editor_tabs.with(|editor_tabs| {
                    editor_tabs.get(&active_editor_tab).copied()
                })?;
                let (_, child) = editor_tab.with(|editor_tab| {
                    editor_tab.children.get(editor_tab.active).cloned()
                })?;

                let editor = match child {
                    EditorTabChild::Editor(editor_id) => {
                        editors.with(|editors| editors.get(&editor_id).copied())?
                    }
                    EditorTabChild::DiffEditor(diff_editor_id) => {
                        let diff_editor = diff_editors.with(|diff_editors| {
                            diff_editors.get(&diff_editor_id).cloned()
                        })?;
                        if diff_editor.focus_right.get() {
                            diff_editor.right
                        } else {
                            diff_editor.left
                        }
                    }
                    _ => return None,
                };

                Some(editor)
            });

        {
            let find_editor_doc = find_editor.view.doc;
            let find = common.find.clone();
            create_effect(move |_| {
                let content = find_editor_doc.with(|doc| doc.buffer().to_string());
                find.set_find(&content);
            });
        }

        Self {
            scope: cx,
            root_split: SplitId::next(),
            splits,
            active_editor_tab,
            editor_tabs,
            editors,
            diff_editors,
            docs,
            active_editor,
            find_editor,
            replace_editor,
            diagnostics,
            locations,
            current_location,
            common,
        }
    }

    pub fn key_down(
        &self,
        key_event: &KeyEvent,
        keypress: &mut KeyPressData,
    ) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
            editor_tabs.get(&active_editor_tab).copied()
        })?;
        let (_, child) = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;
        match child {
            EditorTabChild::Editor(editor_id) => {
                let editor = self
                    .editors
                    .with_untracked(|editors| editors.get(&editor_id).copied())?;
                let editor = editor.get_untracked();
                keypress.key_down(key_event, &editor);
                editor.get_code_actions();
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let diff_editor =
                    self.diff_editors.with_untracked(|diff_editors| {
                        diff_editors.get(&diff_editor_id).cloned()
                    })?;
                let editor = if diff_editor.focus_right.get_untracked() {
                    diff_editor.right.get_untracked()
                } else {
                    diff_editor.left.get_untracked()
                };
                keypress.key_down(key_event, &editor);
                editor.get_code_actions();
            }
            EditorTabChild::Settings(_) => {
                return None;
            }
        }
        Some(())
    }

    fn save_current_jump_location(&self) -> bool {
        if let Some(editor) = self.active_editor.get_untracked() {
            let (doc, cursor, viewport) = editor.with_untracked(|editor| {
                (editor.view.doc, editor.cursor, editor.viewport)
            });
            let path = doc.with_untracked(|doc| doc.content.path().cloned());
            if let Some(path) = path {
                let offset = cursor.with_untracked(|c| c.offset());
                let scroll_offset = viewport.get_untracked().origin().to_vec2();
                return self.save_jump_location(path, offset, scroll_offset);
            }
        }
        false
    }

    pub fn save_jump_location(
        &self,
        path: PathBuf,
        offset: usize,
        scroll_offset: Vec2,
    ) -> bool {
        let mut locations = self.locations.get_untracked();
        if let Some(last_location) = locations.last() {
            if last_location.path == path
                && last_location.position == Some(EditorPosition::Offset(offset))
                && last_location.scroll_offset == Some(scroll_offset)
            {
                return false;
            }
        }
        let location = EditorLocation {
            path,
            position: Some(EditorPosition::Offset(offset)),
            scroll_offset: Some(scroll_offset),
            ignore_unconfirmed: false,
            same_editor_tab: false,
        };
        locations.push_back(location.clone());
        let current_location = locations.len();
        self.locations.set(locations);
        self.current_location.set(current_location);

        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        if let Some((locations, current_location)) = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .map(|editor_tab| {
                editor_tab.with_untracked(|editor_tab| {
                    (editor_tab.locations, editor_tab.current_location)
                })
            })
        {
            let mut l = locations.get_untracked();
            l.push_back(location);
            let new_current_location = l.len();
            locations.set(l);
            current_location.set(new_current_location);
        }

        true
    }

    pub fn jump_to_location(
        &self,
        location: EditorLocation,
        edits: Option<Vec<TextEdit>>,
    ) {
        self.save_current_jump_location();
        self.go_to_location(location, edits);
    }

    pub fn get_doc(&self, path: PathBuf) -> (RwSignal<Document>, bool) {
        let cx = self.scope;
        let doc = self.docs.with_untracked(|docs| docs.get(&path).cloned());
        if let Some(doc) = doc {
            (doc, false)
        } else {
            let diagnostic_data = self.get_diagnostic_data(&path);

            let doc = Document::new(
                cx,
                path.clone(),
                diagnostic_data,
                self.common.find.clone(),
                self.common.proxy.clone(),
                self.common.config,
            );
            let doc = cx.create_rw_signal(doc);
            self.docs.update(|docs| {
                docs.insert(path.clone(), doc);
            });

            {
                let proxy = self.common.proxy.clone();
                create_effect(move |last| {
                    let rev = doc.with(|doc| doc.buffer().rev());
                    if last == Some(rev) {
                        return rev;
                    }
                    let find_result =
                        doc.with_untracked(|doc| doc.find_result.clone());
                    find_result.reset();
                    Document::tigger_proxy_update(doc, &proxy);
                    rev
                });
            }

            {
                let buffer_id = doc.with_untracked(|doc| doc.buffer_id);
                let set_doc = doc.write_only();
                let send = create_ext_action(cx, move |content| {
                    set_doc.update(move |doc| {
                        doc.init_content(content);
                    });
                });

                self.common
                    .proxy
                    .new_buffer(buffer_id, path, move |result| {
                        if let Ok(ProxyResponse::NewBufferResponse { content }) =
                            result
                        {
                            send(Rope::from(content))
                        }
                    });
            }

            (doc, true)
        }
    }

    pub fn go_to_location(
        &self,
        location: EditorLocation,
        edits: Option<Vec<TextEdit>>,
    ) {
        println!("go to location");
        if self.common.focus.get_untracked() != Focus::Workbench {
            self.common.focus.set(Focus::Workbench);
        }
        let path = location.path.clone();
        let (doc, new_doc) = self.get_doc(path.clone());

        let child = self.get_editor_tab_child(
            EditorTabChildSource::Editor { path, doc },
            location.ignore_unconfirmed,
            location.same_editor_tab,
        );
        if let EditorTabChild::Editor(editor_id) = child {
            if let Some(editor) = self
                .editors
                .with_untracked(|editors| editors.get(&editor_id).cloned())
            {
                let editor = editor.get_untracked();
                editor.go_to_location(location, new_doc, edits);
            }
        }
    }

    pub fn open_file_changes(&self, path: PathBuf) {
        let (right, _) = self.get_doc(path.clone());
        let left = Document::new_hisotry(
            self.scope,
            DocContent::History(DocHistory {
                path: path.clone(),
                version: "head".to_string(),
            }),
            self.common.find.clone(),
            self.common.proxy.clone(),
            self.common.config,
        );
        let left = left.scope.create_rw_signal(left);

        let send = create_ext_action(self.scope, move |result| {
            if let Ok(ProxyResponse::BufferHeadResponse { content, .. }) = result {
                left.update(|doc| {
                    doc.init_content(Rope::from(content));
                });
            }
        });
        self.common.proxy.get_buffer_head(path, move |result| {
            send(result);
        });

        self.get_editor_tab_child(
            EditorTabChildSource::DiffEditor { left, right },
            false,
            false,
        );
    }

    fn new_editor_tab(
        &self,
        editor_tab_id: EditorTabId,
        split_id: SplitId,
    ) -> RwSignal<EditorTabData> {
        let editor_tab = {
            let cx = self.scope.create_child();
            let editor_tab = EditorTabData {
                scope: cx,
                split: split_id,
                active: 0,
                editor_tab_id,
                children: vec![],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
                locations: cx.create_rw_signal(im::Vector::new()),
                current_location: cx.create_rw_signal(0),
            };
            cx.create_rw_signal(editor_tab)
        };
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab);
        });
        editor_tab
    }

    fn get_editor_tab_child(
        &self,
        source: EditorTabChildSource,
        ignore_unconfirmed: bool,
        same_editor_tab: bool,
    ) -> EditorTabChild {
        let config = self.common.config.get_untracked();

        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        let active_editor_tab = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .cloned();

        let editors = self.editors.get_untracked();
        let diff_editors = self.diff_editors.get_untracked();

        let active_editor_tab = if let Some(editor_tab) = active_editor_tab {
            editor_tab
        } else if editor_tabs.is_empty() {
            let editor_tab_id = EditorTabId::next();
            let editor_tab = self.new_editor_tab(editor_tab_id, self.root_split);
            let root_split = self.splits.with_untracked(|splits| {
                splits.get(&self.root_split).cloned().unwrap()
            });
            root_split.update(|root_split| {
                root_split.children = vec![SplitContent::EditorTab(editor_tab_id)];
            });
            self.active_editor_tab.set(Some(editor_tab_id));
            editor_tab
        } else {
            let (editor_tab_id, editor_tab) = editor_tabs.iter().next().unwrap();
            self.active_editor_tab.set(Some(*editor_tab_id));
            *editor_tab
        };

        let is_same_diff_editor =
            |diff_editor_id: &DiffEditorId,
             left: &RwSignal<Document>,
             right: &RwSignal<Document>| {
                diff_editors
                    .get(diff_editor_id)
                    .map(|diff_editor| {
                        left.with_untracked(|doc| doc.content.clone())
                            == diff_editor
                                .left
                                .with_untracked(|editor| editor.view.doc)
                                .with_untracked(|doc| doc.content.clone())
                            && right.with_untracked(|doc| doc.content.clone())
                                == diff_editor
                                    .right
                                    .with_untracked(|editor| editor.view.doc)
                                    .with_untracked(|doc| doc.content.clone())
                    })
                    .unwrap_or(false)
            };

        let selected = if !config.editor.show_tab {
            active_editor_tab.with_untracked(|editor_tab| {
                for (i, (_, child)) in editor_tab.children.iter().enumerate() {
                    let can_be_selected = match child {
                        EditorTabChild::Editor(editor_id) => {
                            if let Some(editor) = editors.get(editor_id) {
                                let doc =
                                    editor.with_untracked(|editor| editor.view.doc);
                                doc.with_untracked(|doc| {
                                    let same_path =
                                        if let EditorTabChildSource::Editor {
                                            path,
                                            ..
                                        } = &source
                                        {
                                            doc.content
                                                .path()
                                                .map(|p| p == path)
                                                .unwrap_or(false)
                                        } else {
                                            false
                                        };

                                    same_path || doc.buffer().is_pristine()
                                })
                            } else {
                                false
                            }
                        }
                        EditorTabChild::DiffEditor(diff_editor_id) => {
                            if let EditorTabChildSource::DiffEditor { left, right } =
                                &source
                            {
                                is_same_diff_editor(diff_editor_id, left, right)
                                    || diff_editors
                                        .get(diff_editor_id)
                                        .map(|diff_editor| {
                                            diff_editor
                                                .left
                                                .with_untracked(|editor| {
                                                    editor.view.doc
                                                })
                                                .with_untracked(|doc| {
                                                    doc.buffer().is_pristine()
                                                })
                                                && diff_editor
                                                    .right
                                                    .with_untracked(|editor| {
                                                        editor.view.doc
                                                    })
                                                    .with_untracked(|doc| {
                                                        doc.buffer().is_pristine()
                                                    })
                                        })
                                        .unwrap_or(false)
                            } else {
                                false
                            }
                        }
                        EditorTabChild::Settings(_) => true,
                    };

                    if can_be_selected {
                        return Some(i);
                    }
                }
                None
            })
        } else {
            match &source {
                EditorTabChildSource::Editor { path, .. } => active_editor_tab
                    .with_untracked(|editor_tab| {
                        editor_tab
                            .get_editor(&editors, path)
                            .map(|(i, _)| i)
                            .or_else(|| {
                                if ignore_unconfirmed {
                                    None
                                } else {
                                    editor_tab
                                        .get_unconfirmed_editor_tab_child(
                                            &editors,
                                            &diff_editors,
                                        )
                                        .map(|(i, _)| i)
                                }
                            })
                    }),
                EditorTabChildSource::DiffEditor { left, right } => {
                    if let Some(index) =
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children.iter().position(|(_, child)| {
                                if let EditorTabChild::DiffEditor(diff_editor_id) =
                                    child
                                {
                                    is_same_diff_editor(diff_editor_id, left, right)
                                } else {
                                    false
                                }
                            })
                        })
                    {
                        Some(index)
                    } else if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    &editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
                EditorTabChildSource::Settings => {
                    if let Some(index) =
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children.iter().position(|(_, child)| {
                                matches!(child, EditorTabChild::Settings(_))
                            })
                        })
                    {
                        Some(index)
                    } else if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    &editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
            }
        };

        let new_child_from_source =
            |editor_tab_id: EditorTabId, source: &EditorTabChildSource| match source
            {
                EditorTabChildSource::Editor { doc, .. } => {
                    let editor_id = EditorId::next();
                    let editor = EditorData::new(
                        self.scope,
                        Some(editor_tab_id),
                        editor_id,
                        *doc,
                        self.common.clone(),
                    );
                    let editor = editor.scope.create_rw_signal(editor);
                    self.editors.update(|editors| {
                        editors.insert(editor_id, editor);
                    });
                    EditorTabChild::Editor(editor_id)
                }
                EditorTabChildSource::Settings => {
                    EditorTabChild::Settings(SettingsId::next())
                }
                EditorTabChildSource::DiffEditor { left, right } => {
                    let diff_editor_id = DiffEditorId::next();
                    let diff_editor = DiffEditorData::new(
                        self.scope,
                        diff_editor_id,
                        editor_tab_id,
                        *left,
                        *right,
                        self.common.clone(),
                    );
                    self.diff_editors.update(|diff_editors| {
                        diff_editors.insert(diff_editor_id, diff_editor);
                    });
                    EditorTabChild::DiffEditor(diff_editor_id)
                }
            };

        if let Some(selected) = selected {
            let (editor_tab_id, current_child) =
                active_editor_tab.with_untracked(|editor_tab| {
                    let editor_tab_id = editor_tab.editor_tab_id;
                    let (_, current_child) = &editor_tab.children[selected];
                    match current_child {
                        EditorTabChild::Editor(editor_id) => {
                            if let Some(editor) = editors.get(editor_id) {
                                editor.with_untracked(|editor| {
                                    editor.save_doc_position();
                                });
                            }
                        }
                        EditorTabChild::DiffEditor(_) => {}
                        EditorTabChild::Settings(_) => {}
                    }
                    (editor_tab_id, current_child.clone())
                });

            // firstly, if they are the same type of child, load the new doc to the old editor
            let is_same = match (&current_child, &source) {
                (
                    EditorTabChild::Editor(editor_id),
                    EditorTabChildSource::Editor { path, doc },
                ) => {
                    if let Some(editor) = editors.get(editor_id) {
                        let same_path = editor.with_untracked(|editor| {
                            editor.view.doc.with_untracked(|doc| {
                                doc.content.path() == Some(path)
                            })
                        });
                        if !same_path {
                            editor.update(|editor| {
                                editor.update_doc(*doc);
                            });
                            editor.with_untracked(|editor| {
                                println!("get editor tab child");
                                editor.cursor.set(Cursor::origin(
                                    self.common
                                        .config
                                        .with_untracked(|c| c.core.modal),
                                ));
                            });
                        }
                    }

                    true
                }
                (
                    EditorTabChild::DiffEditor(diff_editor_id),
                    EditorTabChildSource::DiffEditor { left, right },
                ) => {
                    if !is_same_diff_editor(diff_editor_id, left, right) {
                        if let Some(diff_editor) = diff_editors.get(diff_editor_id) {
                            diff_editor.left.update(|editor| {
                                editor.update_doc(*left);
                            });
                            diff_editor.right.update(|editor| {
                                editor.update_doc(*right);
                            });
                        }
                    }
                    true
                }
                (EditorTabChild::Settings(_), EditorTabChildSource::Settings) => {
                    true
                }
                _ => false,
            };
            if is_same {
                let (_, child) = active_editor_tab.with_untracked(|editor_tab| {
                    editor_tab.children[selected].clone()
                });
                active_editor_tab.update(|editor_tab| {
                    editor_tab.active = selected;
                });
                return child;
            }

            // We're loading a different kind of child, clean up the old resources
            match &current_child {
                EditorTabChild::Editor(editor_id) => {
                    self.editors.update(|editors| {
                        editors.remove(editor_id);
                    });
                }
                EditorTabChild::DiffEditor(diff_editor_id) => {
                    self.diff_editors.update(|diff_editors| {
                        diff_editors.remove(diff_editor_id);
                    });
                }
                EditorTabChild::Settings(_) => {}
            }

            // Now loading the new child
            let child = new_child_from_source(editor_tab_id, &source);
            active_editor_tab.update(|editor_tab| {
                editor_tab.children[selected] =
                    (editor_tab.scope.create_rw_signal(0), child.clone());
                editor_tab.active = selected;
            });
            return child;
        }

        // check file exists in non active editor tabs
        if config.editor.show_tab && !ignore_unconfirmed && !same_editor_tab {
            for (editor_tab_id, editor_tab) in &editor_tabs {
                if Some(*editor_tab_id) != active_editor_tab_id {
                    if let Some(index) =
                        editor_tab.with_untracked(|editor_tab| match &source {
                            EditorTabChildSource::Editor { path, .. } => editor_tab
                                .get_editor(&editors, path)
                                .map(|(index, _)| index),
                            EditorTabChildSource::DiffEditor { left, right } => {
                                editor_tab.children.iter().position(|(_, child)| {
                                    if let EditorTabChild::DiffEditor(
                                        diff_editor_id,
                                    ) = child
                                    {
                                        is_same_diff_editor(
                                            diff_editor_id,
                                            left,
                                            right,
                                        )
                                    } else {
                                        false
                                    }
                                })
                            }
                            EditorTabChildSource::Settings => {
                                editor_tab.children.iter().position(|(_, child)| {
                                    matches!(child, EditorTabChild::Settings(_))
                                })
                            }
                        })
                    {
                        self.active_editor_tab.set(Some(*editor_tab_id));
                        editor_tab.update(|editor_tab| {
                            editor_tab.active = index;
                        });
                        let (_, child) = editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children[index].clone()
                        });
                        return child;
                    }
                }
            }
        }

        let editor_tab_id =
            active_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
        let child = new_child_from_source(editor_tab_id, &source);

        active_editor_tab.update(|editor_tab| {
            let active = editor_tab
                .active
                .min(editor_tab.children.len().saturating_sub(1));
            let new_active = if editor_tab.children.is_empty() {
                0
            } else {
                active + 1
            };
            editor_tab
                .children
                .insert(new_active, (self.scope.create_rw_signal(0), child.clone()));
            editor_tab.active = new_active;
        });

        child
    }

    pub fn jump_location_backward(&self, local: bool) {
        println!("jump localtion backward");
        let (locations, current_location) = if local {
            let active_editor_tab_id = self.active_editor_tab.get_untracked();
            let editor_tabs = self.editor_tabs.get_untracked();
            if let Some((locations, current_location)) = active_editor_tab_id
                .and_then(|id| editor_tabs.get(&id))
                .map(|editor_tab| {
                    editor_tab.with_untracked(|editor_tab| {
                        (editor_tab.locations, editor_tab.current_location)
                    })
                })
            {
                (locations, current_location)
            } else {
                return;
            }
        } else {
            (self.locations, self.current_location)
        };

        let locations_value = locations.get_untracked();
        let current_location_value = current_location.get_untracked();
        if current_location_value < 1 {
            return;
        }

        if current_location_value >= locations_value.len() {
            // if we are at the head of the locations, save the current location
            // before jump back
            if self.save_current_jump_location() {
                current_location.update(|l| {
                    *l -= 1;
                });
            }
        }

        current_location.update(|l| {
            *l -= 1;
        });
        let current_location_value = current_location.get_untracked();
        current_location.set(current_location_value);
        let mut location = locations_value[current_location_value].clone();
        // for local jumps, we keep on the same editor tab
        // because we only jump on the same split
        location.same_editor_tab = local;

        self.go_to_location(location, None);
    }

    pub fn jump_location_forward(&self, local: bool) {
        let (locations, current_location) = if local {
            let active_editor_tab_id = self.active_editor_tab.get_untracked();
            let editor_tabs = self.editor_tabs.get_untracked();
            if let Some((locations, current_location)) = active_editor_tab_id
                .and_then(|id| editor_tabs.get(&id))
                .map(|editor_tab| {
                    editor_tab.with_untracked(|editor_tab| {
                        (editor_tab.locations, editor_tab.current_location)
                    })
                })
            {
                (locations, current_location)
            } else {
                return;
            }
        } else {
            (self.locations, self.current_location)
        };

        let locations_value = locations.get_untracked();
        let current_location_value = current_location.get_untracked();
        if locations_value.is_empty() {
            return;
        }
        if current_location_value >= locations_value.len() - 1 {
            return;
        }
        current_location.set(current_location_value + 1);
        let mut location = locations_value[current_location_value + 1].clone();
        // for local jumps, we keep on the same editor tab
        // because we only jump on the same split
        location.same_editor_tab = local;
        self.go_to_location(location, None);
    }

    pub fn split(
        &self,
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
                self.split_editor_tab(self.scope, split_id, editor_tab)
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
                self.split_editor_tab(self.scope, split_id, editor_tab)
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
                self.split_editor_tab(self.scope, new_split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

            let new_split = {
                let cx = self.scope.create_child();
                let new_split = SplitData {
                    scope: cx,
                    parent_split: Some(split_id),
                    split_id: new_split_id,
                    children: vec![
                        SplitContent::EditorTab(editor_tab_id),
                        SplitContent::EditorTab(new_editor_tab_id),
                    ],
                    direction,
                    window_origin: Point::ZERO,
                    layout_rect: Rect::ZERO,
                };
                cx.create_rw_signal(new_split)
            };
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
        cx: Scope,
        split_id: SplitId,
        editor_tab: &EditorTabData,
    ) -> Option<RwSignal<EditorTabData>> {
        let (_, child) = editor_tab.children.get(editor_tab.active)?;

        let editor_tab_id = EditorTabId::next();

        let new_child = match child {
            EditorTabChild::Editor(editor_id) => {
                let new_editor_id = EditorId::next();
                let editor =
                    self.editors.get_untracked().get(editor_id)?.with_untracked(
                        |editor| editor.copy(cx, Some(editor_tab_id), new_editor_id),
                    );
                let editor = editor.scope.create_rw_signal(editor);
                self.editors.update(|editors| {
                    editors.insert(new_editor_id, editor);
                });
                EditorTabChild::Editor(new_editor_id)
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let new_diff_editor_id = DiffEditorId::next();
                let diff_editor = self
                    .diff_editors
                    .get_untracked()
                    .get(diff_editor_id)?
                    .copy(cx, editor_tab_id, new_diff_editor_id);
                self.diff_editors.update(|diff_editors| {
                    diff_editors.insert(new_diff_editor_id, diff_editor);
                });
                EditorTabChild::DiffEditor(new_diff_editor_id)
            }
            EditorTabChild::Settings(_) => {
                EditorTabChild::Settings(SettingsId::next())
            }
        };

        let editor_tab = {
            let cx = self.scope.create_child();
            let editor_tab = EditorTabData {
                scope: cx,
                split: split_id,
                editor_tab_id,
                active: 0,
                children: vec![(cx.create_rw_signal(0), new_child)],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
                locations: cx.create_rw_signal(editor_tab.locations.get_untracked()),
                current_location: cx
                    .create_rw_signal(editor_tab.current_location.get_untracked()),
            };
            cx.create_rw_signal(editor_tab)
        };
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab);
        });
        Some(editor_tab)
    }

    pub fn split_move(
        &self,
        _cx: Scope,
        direction: SplitMoveDirection,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let rect = editor_tab.with_untracked(|editor_tab| {
            editor_tab.layout_rect.with_origin(editor_tab.window_origin)
        });

        match direction {
            SplitMoveDirection::Up => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
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
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
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
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
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
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
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
        cx: Scope,
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

    fn split_content_focus(&self, cx: Scope, content: &SplitContent) {
        match content {
            SplitContent::EditorTab(editor_tab_id) => {
                self.active_editor_tab.set(Some(*editor_tab_id));
            }
            SplitContent::Split(split_id) => {
                self.split_focus(cx, *split_id);
            }
        }
    }

    fn split_focus(&self, cx: Scope, split_id: SplitId) -> Option<()> {
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;

        let split_chilren = split.with_untracked(|split| split.children.clone());
        let content = split_chilren.get(0)?;
        self.split_content_focus(cx, content);

        Some(())
    }

    fn split_remove(&self, split_id: SplitId) -> Option<()> {
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
            .try_update(|split| {
                split
                    .children
                    .retain(|c| c != &SplitContent::Split(split_id));
                split.children.len()
            })
            .unwrap();
        if split_len == 0 {
            self.split_remove(split_id);
        }

        Some(())
    }

    fn editor_tab_remove(
        &self,
        cx: Scope,
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
            self.split_remove(split_id);
        }

        Some(())
    }

    pub fn editor_tab_close(
        &self,
        cx: Scope,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
        let editor_tab = editor_tab.get_untracked();
        for (_, child) in editor_tab.children {
            self.editor_tab_child_close(cx, editor_tab_id, child);
        }

        Some(())
    }

    pub fn editor_tab_child_close(
        &self,
        cx: Scope,
        editor_tab_id: EditorTabId,
        child: EditorTabChild,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let index = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.iter().position(|(_, c)| c == &child)
        })?;

        let editor_tab_children_len = editor_tab
            .try_update(|editor_tab| {
                editor_tab.children.remove(index);
                editor_tab.active =
                    index.min(editor_tab.children.len().saturating_sub(1));
                editor_tab.children.len()
            })
            .unwrap();

        match child {
            EditorTabChild::Editor(editor_id) => {
                let removed_editor = self
                    .editors
                    .try_update(|editors| editors.remove(&editor_id))
                    .unwrap();
                if let Some(editor) = removed_editor {
                    let editor = editor.get_untracked();
                    editor.save_doc_position();
                }
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let removed_diff_editor = self
                    .diff_editors
                    .try_update(|diff_editors| diff_editors.remove(&diff_editor_id))
                    .unwrap();
                if let Some(diff_editor) = removed_diff_editor {
                    let editor = diff_editor.right.get_untracked();
                    editor.save_doc_position();
                }
            }
            EditorTabChild::Settings(_) => {}
        }

        if editor_tab_children_len == 0 {
            self.editor_tab_remove(cx, editor_tab_id);
        }

        Some(())
    }

    pub fn editor_tab_update_layout(
        &self,
        editor_tab_id: &EditorTabId,
        window_origin: Point,
        rect: Rect,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(editor_tab_id).copied()?;
        editor_tab.update(|editor_tab| {
            editor_tab.window_origin = window_origin;
            editor_tab.layout_rect = rect;
        });
        Some(())
    }

    pub fn run_code_action(&self, plugin_id: PluginId, action: CodeActionOrCommand) {
        match action {
            CodeActionOrCommand::Command(_) => {}
            CodeActionOrCommand::CodeAction(action) => {
                if let Some(edit) = action.edit.as_ref() {
                    self.apply_workspace_edit(edit);
                } else {
                    self.resolve_code_action(plugin_id, action);
                }
            }
        }
    }

    /// Resolve a code action and apply its held workspace edit
    fn resolve_code_action(&self, plugin_id: PluginId, action: CodeAction) {
        let main_split = self.clone();
        let send = create_ext_action(self.scope, move |edit| {
            main_split.apply_workspace_edit(&edit);
        });
        self.common
            .proxy
            .code_action_resolve(action, plugin_id, move |result| {
                if let Ok(ProxyResponse::CodeActionResolveResponse { item }) = result
                {
                    if let Some(edit) = item.edit {
                        send(edit);
                    }
                }
            });
    }

    /// Perform a workspace edit, which are from the LSP (such as code actions, or symbol renaming)
    pub fn apply_workspace_edit(&self, edit: &WorkspaceEdit) {
        if let Some(DocumentChanges::Operations(_op)) =
            edit.document_changes.as_ref()
        {
            // TODO
        }

        if let Some(edits) = workspace_edits(edit) {
            for (url, edits) in edits {
                if let Ok(path) = url.to_file_path() {
                    let active_path = self
                        .active_editor
                        .get_untracked()
                        .map(|editor| {
                            editor.with_untracked(|editor| editor.view.doc)
                        })
                        .map(|doc| doc.with_untracked(|doc| doc.content.clone()))
                        .and_then(|content| content.path().cloned());
                    let position = if active_path.as_ref() == Some(&path) {
                        None
                    } else {
                        edits
                            .get(0)
                            .map(|edit| EditorPosition::Position(edit.range.start))
                    };
                    let location = EditorLocation {
                        path,
                        position,
                        scroll_offset: None,
                        ignore_unconfirmed: true,
                        same_editor_tab: false,
                    };
                    self.jump_to_location(location, Some(edits));
                }
            }
        }
    }

    pub fn next_error(&self) {
        let file_diagnostics =
            self.diagnostics_items(DiagnosticSeverity::ERROR, false);
        if file_diagnostics.is_empty() {
            return;
        }
        let active_editor = self.active_editor.get_untracked();
        let active_path = active_editor
            .map(|editor| {
                editor.with_untracked(|editor| (editor.view.doc, editor.cursor))
            })
            .and_then(|(doc, cursor)| {
                let offset = cursor.with_untracked(|c| c.offset());
                let (path, position) = doc.with_untracked(|doc| {
                    (
                        doc.content.path().cloned(),
                        doc.buffer().offset_to_position(offset),
                    )
                });
                path.map(|path| (path, position))
            });
        let (path, position) =
            next_in_file_errors_offset(active_path, &file_diagnostics);
        let location = EditorLocation {
            path,
            position: Some(EditorPosition::Position(position)),
            scroll_offset: None,
            ignore_unconfirmed: false,
            same_editor_tab: false,
        };
        self.jump_to_location(location, None);
    }

    pub fn diagnostics_items(
        &self,
        severity: DiagnosticSeverity,
        tracked: bool,
    ) -> Vec<(PathBuf, RwSignal<bool>, Vec<EditorDiagnostic>)> {
        let diagnostics = if tracked {
            self.diagnostics.get()
        } else {
            self.diagnostics.get_untracked()
        };
        diagnostics
            .into_iter()
            .filter_map(|(path, diagnostic)| {
                let diagnostics = if tracked {
                    diagnostic.diagnostics.get()
                } else {
                    diagnostic.diagnostics.get_untracked()
                };
                let diagnostics: Vec<EditorDiagnostic> = diagnostics
                    .into_iter()
                    .filter(|d| d.diagnostic.severity == Some(severity))
                    .collect();
                if !diagnostics.is_empty() {
                    Some((path, diagnostic.expanded, diagnostics))
                } else {
                    None
                }
            })
            .sorted_by_key(|(path, _, _)| path.clone())
            .collect()
    }

    pub fn get_diagnostic_data(&self, path: &Path) -> DiagnosticData {
        if let Some(d) = self.diagnostics.with_untracked(|d| d.get(path).cloned()) {
            d
        } else {
            let diagnostic_data = DiagnosticData {
                expanded: self.scope.create_rw_signal(true),
                diagnostics: self.scope.create_rw_signal(im::Vector::new()),
            };
            self.diagnostics.update(|d| {
                d.insert(path.to_path_buf(), diagnostic_data.clone());
            });
            diagnostic_data
        }
    }

    pub fn open_file_changed(&self, path: &Path, content: &str) {
        let doc = self.docs.with_untracked(|docs| docs.get(path).copied());
        let doc = match doc {
            Some(doc) => doc,
            None => return,
        };

        doc.update(|doc| {
            doc.handle_file_changed(Rope::from(content));
        });
    }

    pub fn set_find_pattern(&self, pattern: Option<String>) {
        if let Some(pattern) = pattern {
            self.find_editor
                .view
                .doc
                .update(|doc| doc.reload(Rope::from(pattern), true));
        }
        let pattern_len = self
            .find_editor
            .view
            .doc
            .with_untracked(|doc| doc.buffer().len());
        self.find_editor
            .cursor
            .update(|cursor| cursor.set_insert(Selection::region(0, pattern_len)));
    }

    pub fn open_settings(&self) {
        self.get_editor_tab_child(EditorTabChildSource::Settings, false, false);
    }

    pub fn can_jump_location_backward(&self, tracked: bool) -> bool {
        if tracked {
            self.current_location.get() >= 1
        } else {
            self.current_location.get_untracked() >= 1
        }
    }

    pub fn can_jump_location_forward(&self, tracked: bool) -> bool {
        if tracked {
            !(self.locations.with(|l| l.is_empty())
                || self.current_location.get()
                    >= self.locations.with(|l| l.len()) - 1)
        } else {
            !(self.locations.with_untracked(|l| l.is_empty())
                || self.current_location.get_untracked()
                    >= self.locations.with_untracked(|l| l.len()) - 1)
        }
    }
}

fn workspace_edits(edit: &WorkspaceEdit) -> Option<HashMap<Url, Vec<TextEdit>>> {
    if let Some(changes) = edit.changes.as_ref() {
        return Some(changes.clone());
    }

    let changes = edit.document_changes.as_ref()?;
    let edits = match changes {
        DocumentChanges::Edits(edits) => edits
            .iter()
            .map(|e| {
                (
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
        DocumentChanges::Operations(ops) => ops
            .iter()
            .filter_map(|o| match o {
                DocumentChangeOperation::Op(_op) => None,
                DocumentChangeOperation::Edit(e) => Some((
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )),
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
    };
    Some(edits)
}

fn next_in_file_errors_offset(
    active_path: Option<(PathBuf, Position)>,
    file_diagnostics: &[(PathBuf, RwSignal<bool>, Vec<EditorDiagnostic>)],
) -> (PathBuf, Position) {
    if let Some((active_path, position)) = active_path {
        for (current_path, _, diagnostics) in file_diagnostics {
            if &active_path == current_path {
                for diagnostic in diagnostics {
                    if diagnostic.diagnostic.range.start.line > position.line
                        || (diagnostic.diagnostic.range.start.line == position.line
                            && diagnostic.diagnostic.range.start.character
                                > position.character)
                    {
                        return (
                            (*current_path).clone(),
                            diagnostic.diagnostic.range.start,
                        );
                    }
                }
            }
            if current_path > &active_path {
                return (
                    (*current_path).clone(),
                    diagnostics[0].diagnostic.range.start,
                );
            }
        }
    }

    (
        file_diagnostics[0].0.clone(),
        file_diagnostics[0].2[0].diagnostic.range.start,
    )
}
