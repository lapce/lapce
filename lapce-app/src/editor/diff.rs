use std::path::PathBuf;

use floem::reactive::{
    create_effect, create_rw_signal, RwSignal, Scope, SignalGetUntracked,
    SignalUpdate,
};
use lapce_core::buffer::DiffLines;
use serde::{Deserialize, Serialize};

use crate::{
    doc::{DocContent, Document},
    id::{DiffEditorId, EditorId, EditorTabId},
    main_split::MainSplitData,
    window_tab::CommonData,
};

use super::{location::EditorLocation, EditorData};

#[derive(Clone, Serialize, Deserialize)]
pub struct DiffEditorInfo {
    pub left_content: DocContent,
    pub right_content: DocContent,
}

impl DiffEditorInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> DiffEditorData {
        let (cx, _) = data.scope.run_child_scope(|cx| cx);

        let diff_editor_id = DiffEditorId::next();

        let new_editor = {
            let data = data.clone();
            let common = data.common.clone();
            move |content: &DocContent| match content {
                DocContent::File(path) => {
                    let editor_id = EditorId::next();
                    let (doc, new_doc) = data.get_doc(path.clone());
                    let editor_data =
                        EditorData::new(cx, None, editor_id, doc, common.clone());
                    editor_data.go_to_location(
                        EditorLocation {
                            path: path.clone(),
                            position: None,
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                        new_doc,
                        None,
                    );
                    editor_data
                }
                DocContent::Local => {
                    let editor_id = EditorId::next();
                    EditorData::new_local(data.scope, editor_id, common.clone())
                }
                DocContent::History(_) => {
                    let editor_id = EditorId::next();
                    EditorData::new_local(data.scope, editor_id, common.clone())
                }
            }
        };

        let left = new_editor(&self.left_content);
        let left = create_rw_signal(cx, left);
        let right = new_editor(&self.right_content);
        let right = create_rw_signal(cx, right);

        let diff_editor_data = DiffEditorData {
            id: diff_editor_id,
            editor_tab_id,
            scope: cx,
            left,
            right,
        };

        data.diff_editors.update(|diff_editors| {
            diff_editors.insert(diff_editor_id, diff_editor_data.clone());
        });

        diff_editor_data
    }
}

#[derive(Clone)]
pub struct DiffEditorData {
    pub id: DiffEditorId,
    pub editor_tab_id: EditorTabId,
    pub scope: Scope,
    pub left: RwSignal<EditorData>,
    pub right: RwSignal<EditorData>,
}

impl DiffEditorData {
    pub fn new(
        cx: Scope,
        id: DiffEditorId,
        editor_tab_id: EditorTabId,
        left_doc: RwSignal<Document>,
        right_doc: RwSignal<Document>,
        common: CommonData,
    ) -> Self {
        let (cx, _) = cx.run_child_scope(|cx| cx);
        let left =
            EditorData::new(cx, None, EditorId::next(), left_doc, common.clone());
        let left = create_rw_signal(left.scope, left);
        let right = EditorData::new(cx, None, EditorId::next(), right_doc, common);
        let right = create_rw_signal(right.scope, right);

        create_effect(cx, move |_| {});

        Self {
            id,
            editor_tab_id,
            scope: cx,
            left,
            right,
        }
    }

    pub fn diff_editor_info(&self) -> DiffEditorInfo {
        DiffEditorInfo {
            left_content: self.left.get_untracked().doc.get_untracked().content,
            right_content: self.left.get_untracked().doc.get_untracked().content,
        }
    }

    pub fn copy(&self, cx: Scope, diff_editor_id: EditorId) -> Self {
        let (cx, _) = cx.run_child_scope(|cx| cx);
        let mut diff_editor = self.clone();
        diff_editor.scope = cx;
        diff_editor.id = diff_editor_id;
        diff_editor.left = create_rw_signal(
            cx,
            diff_editor
                .left
                .get_untracked()
                .copy(cx, None, EditorId::next()),
        );
        diff_editor.right = create_rw_signal(
            cx,
            diff_editor
                .right
                .get_untracked()
                .copy(cx, None, EditorId::next()),
        );
        diff_editor
    }
}

struct DocHistory {
    path: PathBuf,
    version: String,
}

// #[derive(Clone)]
// enum DocContent {
//     /// A file at some location. This can be a remote path.
//     File(PathBuf),
//     /// A local document, which doens't need to be sync to the disk.
//     Local,
//     History(DocHistory),
// }

struct DiffData {
    doc: Document,
    changes: DiffLines,
}

enum EditorViewKind {
    Normal,
    Diff(DiffLines),
}
