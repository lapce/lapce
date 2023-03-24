use std::path::Path;

use floem::{
    peniko::kurbo::{Point, Rect},
    reactive::{RwSignal, SignalGetUntracked, SignalWithUntracked},
};

use crate::{
    doc::DocContent,
    editor::EditorData,
    id::{EditorId, EditorTabId, SplitId},
};

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
}
