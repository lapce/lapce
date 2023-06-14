use std::path::PathBuf;

use floem::reactive::{create_effect, create_rw_signal, Scope};
use lapce_core::buffer::DiffLines;

use crate::{
    doc::Document,
    id::{DiffEditorId, EditorId},
    window_tab::CommonData,
};

use super::EditorData;

pub struct DiffEditorData {
    pub id: DiffEditorId,
    pub left: EditorData,
    pub right: EditorData,
}

impl DiffEditorData {
    pub fn new(
        cx: Scope,
        id: DiffEditorId,
        left_doc: Document,
        right_doc: Document,
        common: CommonData,
    ) -> Self {
        let left_doc = create_rw_signal(cx, left_doc);
        let left =
            EditorData::new(cx, None, EditorId::next(), left_doc, common.clone());
        let right_doc = create_rw_signal(cx, right_doc);
        let right = EditorData::new(cx, None, EditorId::next(), right_doc, common);

        create_effect(cx, move |_| {});

        Self { id, left, right }
    }
}

struct DocHistory {
    path: PathBuf,
    version: String,
}

enum DocContent {
    /// A file at some location. This can be a remote path.
    File(PathBuf),
    /// A local document, which doens't need to be sync to the disk.
    Local,
    History(DocHistory),
}

struct DiffData {
    doc: Document,
    changes: DiffLines,
}

enum EditorViewKind {
    Normal,
    Diff(DiffLines),
}
