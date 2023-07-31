use std::path::{Path, PathBuf};

use floem::{
    peniko::kurbo::{Point, Rect},
    reactive::{RwSignal, Scope},
};
use serde::{Deserialize, Serialize};

use crate::{
    doc::{DocContent, Document},
    editor::{
        diff::{DiffEditorData, DiffEditorInfo},
        location::EditorLocation,
        EditorData, EditorInfo,
    },
    id::{DiffEditorId, EditorId, EditorTabId, SettingsId, SplitId},
    main_split::MainSplitData,
    window_tab::WindowTabData,
};

#[derive(Clone, Serialize, Deserialize)]
pub enum EditorTabChildInfo {
    Editor(EditorInfo),
    DiffEditor(DiffEditorInfo),
    Settings,
}

impl EditorTabChildInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> EditorTabChild {
        match &self {
            EditorTabChildInfo::Editor(editor_info) => {
                let editor_data = editor_info.to_data(data, editor_tab_id);
                EditorTabChild::Editor(
                    editor_data.with_untracked(|editor_data| editor_data.editor_id),
                )
            }
            EditorTabChildInfo::DiffEditor(diff_editor_info) => {
                let diff_editor_data = diff_editor_info.to_data(data, editor_tab_id);
                EditorTabChild::DiffEditor(diff_editor_data.id)
            }
            EditorTabChildInfo::Settings => {
                EditorTabChild::Settings(SettingsId::next())
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorTabInfo {
    pub active: usize,
    pub is_focus: bool,
    pub children: Vec<EditorTabChildInfo>,
}

impl EditorTabInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        split: SplitId,
    ) -> RwSignal<EditorTabData> {
        let editor_tab_id = EditorTabId::next();
        let editor_tab_data = {
            let cx = data.scope.create_child();
            let editor_tab_data = EditorTabData {
                scope: cx,
                editor_tab_id,
                split,
                active: self.active,
                children: self
                    .children
                    .iter()
                    .map(|child| {
                        (
                            cx.create_rw_signal(0),
                            child.to_data(data.clone(), editor_tab_id),
                        )
                    })
                    .collect(),
                layout_rect: Rect::ZERO,
                window_origin: Point::ZERO,
                locations: cx.create_rw_signal(im::Vector::new()),
                current_location: cx.create_rw_signal(0),
            };
            cx.create_rw_signal(editor_tab_data)
        };
        if self.is_focus {
            data.active_editor_tab.set(Some(editor_tab_id));
        }
        data.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab_data);
        });
        editor_tab_data
    }
}

pub enum EditorTabChildSource {
    Editor {
        path: PathBuf,
        doc: RwSignal<Document>,
    },
    DiffEditor {
        left: RwSignal<Document>,
        right: RwSignal<Document>,
    },
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorTabChild {
    Editor(EditorId),
    DiffEditor(DiffEditorId),
    Settings(SettingsId),
}

impl EditorTabChild {
    pub fn id(&self) -> u64 {
        match self {
            EditorTabChild::Editor(id) => id.to_raw(),
            EditorTabChild::DiffEditor(id) => id.to_raw(),
            EditorTabChild::Settings(id) => id.to_raw(),
        }
    }

    pub fn is_settings(&self) -> bool {
        matches!(self, EditorTabChild::Settings(_))
    }

    pub fn child_info(&self, data: &WindowTabData) -> EditorTabChildInfo {
        match &self {
            EditorTabChild::Editor(editor_id) => {
                let editor_data = data
                    .main_split
                    .editors
                    .get_untracked()
                    .get(editor_id)
                    .cloned()
                    .unwrap();
                EditorTabChildInfo::Editor(
                    editor_data.get_untracked().editor_info(data),
                )
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let diff_editor_data = data
                    .main_split
                    .diff_editors
                    .get_untracked()
                    .get(diff_editor_id)
                    .cloned()
                    .unwrap();
                EditorTabChildInfo::DiffEditor(diff_editor_data.diff_editor_info())
            }
            EditorTabChild::Settings(_) => EditorTabChildInfo::Settings,
        }
    }
}

#[derive(Clone)]
pub struct EditorTabData {
    pub scope: Scope,
    pub split: SplitId,
    pub editor_tab_id: EditorTabId,
    pub active: usize,
    pub children: Vec<(RwSignal<usize>, EditorTabChild)>,
    pub window_origin: Point,
    pub layout_rect: Rect,
    pub locations: RwSignal<im::Vector<EditorLocation>>,
    pub current_location: RwSignal<usize>,
}

impl EditorTabData {
    pub fn get_editor(
        &self,
        editors: &im::HashMap<EditorId, RwSignal<EditorData>>,
        path: &Path,
    ) -> Option<(usize, RwSignal<EditorData>)> {
        for (i, child) in self.children.iter().enumerate() {
            if let (_, EditorTabChild::Editor(editor_id)) = child {
                if let Some(editor) = editors.get(editor_id) {
                    let e = editor.get_untracked();
                    let is_path = e.view.doc.with_untracked(|doc| {
                        if let DocContent::File(p) = &doc.content {
                            p == path
                        } else {
                            false
                        }
                    });
                    if is_path {
                        return Some((i, *editor));
                    }
                }
            }
        }
        None
    }

    pub fn get_unconfirmed_editor_tab_child(
        &self,
        editors: &im::HashMap<EditorId, RwSignal<EditorData>>,
        diff_editors: &im::HashMap<EditorId, DiffEditorData>,
    ) -> Option<(usize, EditorTabChild)> {
        for (i, (_, child)) in self.children.iter().enumerate() {
            match child {
                EditorTabChild::Editor(editor_id) => {
                    if let Some(editor) = editors.get(editor_id) {
                        let e = editor.get_untracked();
                        let confirmed = e.confirmed.get_untracked();
                        if !confirmed {
                            return Some((i, child.clone()));
                        }
                    }
                }
                EditorTabChild::DiffEditor(diff_editor_id) => {
                    if let Some(diff_editor) = diff_editors.get(diff_editor_id) {
                        let left_confirmed =
                            diff_editor.left.with_untracked(|editor| {
                                editor.confirmed.get_untracked()
                            });
                        let right_confirmed =
                            diff_editor.right.with_untracked(|editor| {
                                editor.confirmed.get_untracked()
                            });
                        if !left_confirmed && !right_confirmed {
                            return Some((i, child.clone()));
                        }
                    }
                }
                _ => (),
            }
        }
        None
    }

    pub fn get_unconfirmed_editor(
        &self,
        editors: &im::HashMap<EditorId, RwSignal<EditorData>>,
    ) -> Option<(usize, RwSignal<EditorData>)> {
        for (i, child) in self.children.iter().enumerate() {
            if let (_, EditorTabChild::Editor(editor_id)) = child {
                if let Some(editor) = editors.get(editor_id) {
                    let e = editor.get_untracked();
                    let confirmed = e.confirmed.get_untracked();
                    if !confirmed {
                        return Some((i, *editor));
                    }
                }
            }
        }
        None
    }

    pub fn tab_info(&self, data: &WindowTabData) -> EditorTabInfo {
        let info = EditorTabInfo {
            active: self.active,
            is_focus: data.main_split.active_editor_tab.get_untracked()
                == Some(self.editor_tab_id),
            children: self
                .children
                .iter()
                .map(|(_, child)| child.child_info(data))
                .collect(),
        };
        info
    }
}
