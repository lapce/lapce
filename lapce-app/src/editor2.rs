use std::rc::Rc;

use floem::{
    kurbo::Rect,
    reactive::{RwSignal, Scope},
};
use floem_editor::{editor::Editor, id::EditorId};
use lapce_core::{cursor::Cursor, movement::Movement};

use crate::{
    doc2::Doc,
    editor::{EditorInfo, InlineFindDirection, SnippetIndex},
    id::{DiffEditorId, EditorTabId},
    window_tab::{CommonData, WindowTabData},
};

#[derive(Clone)]
pub struct EditorData {
    pub editor_tab_id: RwSignal<Option<EditorTabId>>,
    pub diff_editor_id: RwSignal<Option<(EditorTabId, DiffEditorId)>>,
    // TODO(floem-editor): confirmed?
    // TODO(floem-editor): window origin? maybe on editor..?
    // TODO(floem-editor): scroll_delta/scroll_to? maybe on editor?
    pub snippet: RwSignal<Option<SnippetIndex>>,
    // TODO(floem-editor): should this be on the editor?
    pub last_movement: RwSignal<Movement>,
    pub inline_find: RwSignal<Option<InlineFindDirection>>,
    pub last_inline_find: RwSignal<Option<(InlineFindDirection, String)>>,
    pub find_focus: RwSignal<bool>,
    pub editor: Rc<Editor>,
    pub common: Rc<CommonData>,
}
impl PartialEq for EditorData {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
impl EditorData {
    // TODO(floem-editor): should this construct `Editor`?
    pub fn new(
        cx: Scope,
        editor: Rc<Editor>,
        editor_tab_id: Option<EditorTabId>,
        diff_editor_id: Option<(EditorTabId, DiffEditorId)>,
        common: Rc<CommonData>,
    ) -> Self {
        let cx = cx.create_child();
        EditorData {
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            diff_editor_id: cx.create_rw_signal(diff_editor_id),
            snippet: cx.create_rw_signal(None),
            last_movement: cx.create_rw_signal(Movement::Left),
            inline_find: cx.create_rw_signal(None),
            last_inline_find: cx.create_rw_signal(None),
            find_focus: cx.create_rw_signal(false),
            editor,
            common,
        }
    }

    pub fn id(&self) -> EditorId {
        self.editor.id()
    }

    pub fn editor_info(&self, _data: &WindowTabData) -> EditorInfo {
        let offset = self.cursor().get_untracked().offset();
        let scroll_offset = self.viewport().get_untracked().origin();
        EditorInfo {
            content: self.doc().content.get_untracked(),
            offset,
            scroll_offset: (scroll_offset.x, scroll_offset.y),
        }
    }

    pub fn cursor(&self) -> RwSignal<Cursor> {
        self.editor.cursor
    }

    pub fn viewport(&self) -> RwSignal<Rect> {
        self.editor.viewport
    }

    pub fn doc(&self) -> Rc<Doc> {
        let doc = self.editor.doc();
        let Ok(doc) = doc.downcast_rc() else {
            panic!("doc is not Rc<Doc>");
        };

        doc
    }

    // TODO: update doc? That doesn't entirely fit with us having a specific editr id -> editor
    // mapping

    // TODO: copy
}
