use floem::{
    peniko::kurbo::Rect,
    reactive::{RwSignal, Scope},
    views::editor::id::EditorId,
};

use crate::markdown::MarkdownContent;

#[derive(Clone)]
pub struct HoverData {
    pub active: RwSignal<bool>,
    pub offset: RwSignal<usize>,
    pub editor_id: RwSignal<EditorId>,
    pub content: RwSignal<Vec<MarkdownContent>>,
    pub layout_rect: RwSignal<Rect>,
}

impl HoverData {
    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            offset: cx.create_rw_signal(0),
            content: cx.create_rw_signal(Vec::new()),
            editor_id: cx.create_rw_signal(EditorId::next()),
            layout_rect: cx.create_rw_signal(Rect::ZERO),
        }
    }
}
