use std::path::Path;

use floem::{
    peniko::kurbo::{Point, Rect},
    reactive::RwSignal,
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
    pub children: Vec<EditorTabChild>,
    pub window_origin: Point,
    pub layout_rect: Rect,
}

impl EditorTabData {
    pub fn get_editor(
        &self,
        editors: &im::HashMap<EditorId, RwSignal<EditorData>>,
        path: &Path,
    ) -> Option<RwSignal<EditorData>> {
        for child in &self.children {
            if let EditorTabChild::Editor(editor_id) = child {
                if let Some(editor) = editors.get(editor_id) {
                    let e = editor.get();
                    let is_path = e.doc.with(|doc| {
                        if let DocContent::File(p) = &doc.content {
                            p == path
                        } else {
                            false
                        }
                    });
                    if is_path {
                        return Some(*editor);
                    }
                }
            }
        }
        None
    }
}
