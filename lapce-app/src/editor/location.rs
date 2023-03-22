use std::path::PathBuf;

use lapce_core::buffer::Buffer;
use lsp_types::Position;

#[derive(Clone)]
pub struct EditorLocation {
    pub path: PathBuf,
    pub position: Option<EditorPosition>,
}

#[derive(Clone, Copy)]
pub enum EditorPosition {
    Line(usize),
    Position(Position),
}

impl EditorPosition {
    pub fn to_offset(&self, buffer: &Buffer) -> usize {
        match self {
            EditorPosition::Line(n) => {
                buffer.first_non_blank_character_on_line(n.saturating_sub(1))
            }
            EditorPosition::Position(position) => {
                buffer.offset_of_position(position)
            }
        }
    }
}
