use std::path::PathBuf;

use floem::peniko::kurbo::Vec2;
use lapce_core::{buffer::rope_text::RopeText, rope_text_pos::RopeTextPosition};
use lsp_types::Position;

#[derive(Clone, Debug, PartialEq)]
pub struct EditorLocation {
    pub path: PathBuf,
    pub position: Option<EditorPosition>,
    pub scroll_offset: Option<Vec2>,
    // This will ignore unconfirmed editors, and always create new editors
    // if there's no match path on the active editor tab
    pub ignore_unconfirmed: bool,
    // This will stop finding matching path on different editor tabs
    pub same_editor_tab: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorPosition {
    Line(usize),
    Position(Position),
    Offset(usize),
}

impl EditorPosition {
    pub fn to_offset(&self, text: &impl RopeText) -> usize {
        match self {
            EditorPosition::Line(n) => text.first_non_blank_character_on_line(*n),
            EditorPosition::Position(position) => text.offset_of_position(position),
            EditorPosition::Offset(offset) => *offset,
        }
    }
}
