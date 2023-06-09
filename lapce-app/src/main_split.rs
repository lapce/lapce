use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use floem::{
    ext_event::create_ext_action,
    glazier::KeyEvent,
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{
        create_effect, create_memo, create_rw_signal, Memo, RwSignal, Scope,
        SignalGet, SignalGetUntracked, SignalSet, SignalUpdate, SignalWith,
        SignalWithUntracked,
    },
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
    doc::{DiagnosticData, Document, EditorDiagnostic},
    editor::{
        location::{EditorLocation, EditorPosition},
        EditorData,
    },
    editor_tab::{EditorTabChild, EditorTabData, EditorTabInfo},
    id::{EditorId, EditorTabId, SettingsId, SplitId},
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
            let (cx, _) = data.scope.run_child_scope(|cx| cx);
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
            create_rw_signal(cx, split_data)
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
        let splits = create_rw_signal(cx, im::HashMap::new());
        let active_editor_tab = create_rw_signal(cx, None);
        let editor_tabs: RwSignal<
            im::HashMap<EditorTabId, RwSignal<EditorTabData>>,
        > = create_rw_signal(cx, im::HashMap::new());
        let editors = create_rw_signal(cx, im::HashMap::new());
        let docs = create_rw_signal(cx, im::HashMap::new());
        let locations = create_rw_signal(cx, im::Vector::new());
        let current_location = create_rw_signal(cx, 0);
        let diagnostics = create_rw_signal(cx, im::HashMap::new());
        let find_editor =
            EditorData::new_local(cx, EditorId::next(), common.clone());
        let replace_editor =
            EditorData::new_local(cx, EditorId::next(), common.clone());

        let active_editor =
            create_memo(cx, move |_| -> Option<RwSignal<EditorData>> {
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
                    _ => return None,
                };

                Some(editor)
            });

        {
            let find_editor_doc = find_editor.doc;
            let find = common.find.clone();
            create_effect(cx, move |_| {
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
            EditorTabChild::Settings(_) => {
                return None;
            }
        }
        Some(())
    }

    fn save_current_jump_location(&self) -> bool {
        if let Some(editor) = self.active_editor.get_untracked() {
            let (doc, cursor, viewport) = editor.with_untracked(|editor| {
                (editor.doc, editor.cursor, editor.viewport)
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
            let doc = create_rw_signal(cx, doc);
            self.docs.update(|docs| {
                docs.insert(path.clone(), doc);
            });

            {
                let proxy = self.common.proxy.clone();
                create_effect(cx, move |last| {
                    let rev = doc.with(|doc| doc.buffer().rev());
                    if last == Some(rev) {
                        return rev;
                    }
                    let find_result =
                        doc.with_untracked(|doc| doc.find_result.clone());
                    find_result.reset();
                    Document::tigger_proxy_update(cx, doc, &proxy);
                    rev
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
        if self.common.focus.get_untracked() != Focus::Workbench {
            self.common.focus.set(Focus::Workbench);
        }
        let path = location.path.clone();
        let (doc, new_doc) = self.get_doc(path.clone());

        let editor = self.get_editor_or_new(
            doc,
            &path,
            location.ignore_unconfirmed,
            location.same_editor_tab,
        );
        let editor = editor.get_untracked();
        editor.go_to_location(location, new_doc, edits);
    }

    fn new_editor_tab(
        &self,
        editor_tab_id: EditorTabId,
        split_id: SplitId,
    ) -> RwSignal<EditorTabData> {
        let editor_tab = {
            let (cx, _) = self.scope.run_child_scope(|cx| cx);
            let editor_tab = EditorTabData {
                scope: cx,
                split: split_id,
                active: 0,
                editor_tab_id,
                children: vec![],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
                locations: create_rw_signal(cx, im::Vector::new()),
                current_location: create_rw_signal(cx, 0),
            };
            create_rw_signal(cx, editor_tab)
        };
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab);
        });
        editor_tab
    }

    fn get_editor_or_new(
        &self,
        doc: RwSignal<Document>,
        path: &Path,
        ignore_unconfirmed: bool,
        same_editor_tab: bool,
    ) -> RwSignal<EditorData> {
        let cx = self.scope;
        let config = self.common.config.get_untracked();

        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        let editors = self.editors.get_untracked();
        let splits = self.splits.get_untracked();

        let active_editor_tab = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .cloned();

        // first check if the file exists in active editor tab or there's unconfirmed editor
        if let Some(editor_tab) = active_editor_tab {
            let selected = if !config.editor.show_tab {
                editor_tab.with_untracked(|editor_tab| {
                    for (i, child) in editor_tab.children.iter().enumerate() {
                        if let (_, EditorTabChild::Editor(editor_id)) = child {
                            if let Some(editor) = editors.get(editor_id) {
                                let e = editor.get_untracked();
                                let can_be_selected = e.doc.with_untracked(|doc| {
                                    doc.content
                                        .path()
                                        .map(|p| p == path)
                                        .unwrap_or(false)
                                        || doc.buffer().is_pristine()
                                });
                                if can_be_selected {
                                    return Some((i, *editor));
                                }
                            }
                        }
                    }
                    None
                })
            } else {
                editor_tab.with_untracked(|editor_tab| {
                    editor_tab.get_editor(&editors, path).or_else(|| {
                        if ignore_unconfirmed {
                            None
                        } else {
                            editor_tab.get_unconfirmed_editor(&editors)
                        }
                    })
                })
            };

            if let Some((index, editor)) = selected {
                if editor.with_untracked(|editor| {
                    editor.doc.with_untracked(|doc| doc.buffer_id)
                        != doc.with_untracked(|doc| doc.buffer_id)
                }) {
                    editor.with_untracked(|editor| {
                        editor.save_doc_position(cx);
                    });
                    editor.update(|editor| {
                        editor.update_doc(doc);
                    });
                    editor.with_untracked(|editor| {
                        editor.cursor.set(Cursor::origin(
                            self.common.config.with_untracked(|c| c.core.modal),
                        ));
                    });
                }
                editor_tab.update(|editor_tab| {
                    editor_tab.active = index;
                });
                return editor;
            }
        }

        // check file exists in non active editor tabs
        if config.editor.show_tab && !ignore_unconfirmed && !same_editor_tab {
            for (editor_tab_id, editor_tab) in &editor_tabs {
                if Some(*editor_tab_id) != active_editor_tab_id {
                    if let Some((index, editor)) =
                        editor_tab.with_untracked(|editor_tab| {
                            editor_tab.get_editor(&editors, path)
                        })
                    {
                        self.active_editor_tab.set(Some(*editor_tab_id));
                        editor_tab.update(|editor_tab| {
                            editor_tab.active = index;
                        });
                        return editor;
                    }
                }
            }
        }

        let editor = if editor_tabs.is_empty() {
            // the main split doens't have anything
            let editor_tab_id = EditorTabId::next();

            // create the new editor
            let editor_id = EditorId::next();
            let editor = EditorData::new(
                self.scope,
                Some(editor_tab_id),
                editor_id,
                doc,
                self.common.clone(),
            );
            let editor = create_rw_signal(cx, editor);
            self.editors.update(|editors| {
                editors.insert(editor_id, editor);
            });

            let editor_tab = {
                let (cx, _) = self.scope.run_child_scope(|cx| cx);
                let editor_tab = EditorTabData {
                    scope: cx,
                    split: self.root_split,
                    active: 0,
                    editor_tab_id,
                    children: vec![(
                        create_rw_signal(cx, 0),
                        EditorTabChild::Editor(editor_id),
                    )],
                    window_origin: Point::ZERO,
                    layout_rect: Rect::ZERO,
                    locations: create_rw_signal(cx, im::Vector::new()),
                    current_location: create_rw_signal(cx, 0),
                };
                create_rw_signal(cx, editor_tab)
            };
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
                Some(editor_tab_id),
                editor_id,
                doc,
                self.common.clone(),
            );
            let editor = create_rw_signal(cx, editor);
            self.editors.update(|editors| {
                editors.insert(editor_id, editor);
            });

            editor_tab.update(|editor_tab| {
                let active = editor_tab
                    .active
                    .min(editor_tab.children.len().saturating_sub(1));
                editor_tab.children.insert(
                    active + 1,
                    (create_rw_signal(cx, 0), EditorTabChild::Editor(editor_id)),
                );
                editor_tab.active = active + 1;
            });
            editor
        };

        editor
    }

    pub fn jump_location_backward(&self, local: bool) {
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
                let (cx, _) = self.scope.run_child_scope(|cx| cx);
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
                create_rw_signal(cx, new_split)
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
                let editor = create_rw_signal(cx, editor);
                self.editors.update(|editors| {
                    editors.insert(new_editor_id, editor);
                });
                EditorTabChild::Editor(new_editor_id)
            }
            EditorTabChild::Settings(_) => {
                EditorTabChild::Settings(SettingsId::next())
            }
        };

        let editor_tab = {
            let (cx, _) = self.scope.run_child_scope(|cx| cx);
            let editor_tab = EditorTabData {
                scope: cx,
                split: split_id,
                editor_tab_id,
                active: 0,
                children: vec![(create_rw_signal(cx, 0), new_child)],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
                locations: create_rw_signal(
                    cx,
                    editor_tab.locations.get_untracked(),
                ),
                current_location: create_rw_signal(
                    cx,
                    editor_tab.current_location.get_untracked(),
                ),
            };
            create_rw_signal(cx, editor_tab)
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
                    editor.save_doc_position(cx);
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
                        .map(|editor| editor.with_untracked(|editor| editor.doc))
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
                editor.with_untracked(|editor| (editor.doc, editor.cursor))
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
                expanded: create_rw_signal(self.scope, true),
                diagnostics: create_rw_signal(self.scope, im::Vector::new()),
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
                .doc
                .update(|doc| doc.reload(Rope::from(pattern), true));
        }
        let pattern_len = self
            .find_editor
            .doc
            .with_untracked(|doc| doc.buffer().len());
        self.find_editor
            .cursor
            .update(|cursor| cursor.set_insert(Selection::region(0, pattern_len)));
    }

    pub fn open_settings(&self) {
        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        let active_editor_tab = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .cloned();

        let editor_tab = if let Some(editor_tab) = active_editor_tab {
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

        let position = editor_tab.with_untracked(|editor_tab| {
            editor_tab
                .children
                .iter()
                .position(|(_, child)| child.is_settings())
        });
        if let Some(position) = position {
            editor_tab.update(|editor_tab| {
                editor_tab.active = position;
            });
            return;
        }

        editor_tab.update(|editor_tab| {
            let new_active = (editor_tab.active + 1).min(editor_tab.children.len());
            editor_tab.children.insert(
                new_active,
                (
                    create_rw_signal(self.scope, 0),
                    EditorTabChild::Settings(SettingsId::next()),
                ),
            );
            editor_tab.active = new_active;
        });
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
