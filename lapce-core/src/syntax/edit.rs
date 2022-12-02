use lapce_xi_rope::Rope;
use tree_sitter::Point;

use crate::buffer::rope_text::RopeText;

#[derive(Clone)]
pub struct SyntaxEdit(pub(crate) Vec<tree_sitter::InputEdit>);

impl SyntaxEdit {
    pub fn new(edits: Vec<tree_sitter::InputEdit>) -> Self {
        Self(edits)
    }
}

fn point_at_offset(text: &Rope, offset: usize) -> Point {
    let text = RopeText::new(text);
    let line = text.line_of_offset(offset);
    let col = text.offset_of_line(line + 1).saturating_sub(offset);
    Point::new(line, col)
}

fn traverse(point: Point, text: &str) -> Point {
    let Point {
        mut row,
        mut column,
    } = point;

    for ch in text.chars() {
        if ch == '\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Point { row, column }
}

pub fn create_insert_edit(
    old_text: &Rope,
    start: usize,
    inserted: &Rope,
) -> tree_sitter::InputEdit {
    let start_position = point_at_offset(old_text, start);
    tree_sitter::InputEdit {
        start_byte: start,
        old_end_byte: start,
        new_end_byte: start + inserted.len(),
        start_position,
        old_end_position: start_position,
        new_end_position: traverse(
            start_position,
            &inserted.slice_to_cow(0..inserted.len()),
        ),
    }
}

pub fn create_delete_edit(
    old_text: &Rope,
    start: usize,
    end: usize,
) -> tree_sitter::InputEdit {
    let start_position = point_at_offset(old_text, start);
    let end_position = point_at_offset(old_text, end);
    tree_sitter::InputEdit {
        start_byte: start,
        // The old end byte position was at the end
        old_end_byte: end,
        // but since we're deleting everything up to it, it gets 'moved' to where we start
        new_end_byte: start,

        start_position,
        old_end_position: end_position,
        new_end_position: start_position,
    }
}
