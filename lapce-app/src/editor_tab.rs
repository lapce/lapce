use std::path::Path;

use floem::{
    app::AppContext,
    peniko::kurbo::{Point, Rect},
    reactive::{
        create_rw_signal, RwSignal, SignalGetUntracked, SignalSet, SignalUpdate,
        SignalWithUntracked,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    doc::DocContent,
    editor::{EditorData, EditorInfo},
    id::{EditorId, EditorTabId, SplitId},
    main_split::MainSplitData,
    window_tab::WindowTabData,
};

#[derive(Clone, Serialize, Deserialize)]
pub enum EditorTabChildInfo {
    Editor(EditorInfo),
}

impl EditorTabChildInfo {
    pub fn to_data(
        &self,
        cx: AppContext,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> EditorTabChild {
        match &self {
            EditorTabChildInfo::Editor(editor_info) => {
                let editor_data = editor_info.to_data(cx, data, editor_tab_id);
                EditorTabChild::Editor(
                    editor_data.with_untracked(|editor_data| editor_data.editor_id),
                )
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
        cx: AppContext,
        data: MainSplitData,
        split: SplitId,
    ) -> RwSignal<EditorTabData> {
        let editor_tab_id = EditorTabId::next();
        let editor_tab_data = EditorTabData {
            editor_tab_id,
            split,
            active: self.active,
            children: self
                .children
                .iter()
                .map(|child| {
                    (
                        create_rw_signal(cx.scope, 0),
                        child.to_data(cx, data.clone(), editor_tab_id),
                    )
                })
                .collect(),
            layout_rect: Rect::ZERO,
            window_origin: Point::ZERO,
        };
        let editor_tab_data = create_rw_signal(cx.scope, editor_tab_data);
        if self.is_focus {
            data.active_editor_tab.set(Some(editor_tab_id));
        }
        data.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab_data.clone());
        });
        editor_tab_data
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorTabChild {
    Editor(EditorId),
}

impl EditorTabChild {
    pub fn id(&self) -> u64 {
        match self {
            EditorTabChild::Editor(id) => id.to_raw(),
        }
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
        }
    }
}

#[derive(Clone)]
pub struct EditorTabData {
    pub split: SplitId,
    pub editor_tab_id: EditorTabId,
    pub active: usize,
    pub children: Vec<(RwSignal<usize>, EditorTabChild)>,
    pub window_origin: Point,
    pub layout_rect: Rect,
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
                    let is_path = e.doc.with_untracked(|doc| {
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
