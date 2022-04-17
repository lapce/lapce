use std::collections::HashSet;
use xi_rope::RopeDelta;

use crate::{
    buffer::{data::{BufferDataListener, EditableBufferData}, EditType},
    movement::{Selection, Cursor},
};
use lapce_core::indent::IndentStyle;

pub(super) fn create_edit<'s, 'b, L: BufferDataListener>(
    buffer: &EditableBufferData<'b, L>,
    offset: usize,
    indent: &'s str,
    tab_width: usize,
) -> (Selection, &'s str) {
    let longest_indent = IndentStyle::LONGEST_INDENT;
    let indent = if indent.starts_with('\t') {
        indent
    } else {
        let (_, col) = buffer.offset_to_line_col(offset, tab_width);
        longest_indent.split_at(indent.len() - col % indent.len()).0
    };
    (Selection::caret(offset), indent)
}

pub(super) fn apply_edits<'b, L: BufferDataListener>(
    mut buffer: EditableBufferData<'b, L>,
    cursor: &mut Cursor,
    edits: Vec<(Selection, &str)>
) -> RopeDelta {
    let edits = edits
        .iter()
        .map(|(selection, s)| (selection, *s))
        .collect::<Vec<(&Selection, &str)>>();

    let delta = buffer.edit_multiple(&edits, EditType::InsertChars);
    cursor.apply_delta(&delta);
    delta
}

/// Run indentation changes in regions of `selection` in `buffer`.  The
/// `edit_one_line` function can indent or outdent, depending on the calling
/// command (either `IndentLine` or `OutdentLine`).
pub(super) fn execute<'s, 'b, L: BufferDataListener, F>(
    buffer: EditableBufferData<'b, L>,
    selection: Option<Selection>,
    cursor: &mut Cursor,
    tab_width: usize,
    edit_one_line: F,
) -> RopeDelta
where
    F: Fn(&EditableBufferData<'b, L>, usize, &'s str, usize) -> Option<(Selection, &'s str)>
{
    let selection = selection
        .unwrap_or_else(|| cursor.edit_selection(buffer.buffer, tab_width));

    let indent = buffer.indent_unit();
    let mut edits = Vec::new();

    let mut lines = HashSet::new();
    for region in selection.regions() {
        let start_line = buffer.line_of_offset(region.min());
        let mut end_line = buffer.line_of_offset(region.max());
        if end_line > start_line {
            let end_line_start = buffer.offset_of_line(end_line);
            if end_line_start == region.max() {
                end_line -= 1;
            }
        }
        for line in start_line..end_line + 1 {
            if lines.contains(&line) {
                continue;
            }
            lines.insert(line);
            let line_content = buffer.buffer.line_content(line);
            if line_content == "\n" || line_content == "\r\n" {
                continue;
            }
            let nonblank = buffer.first_non_blank_character_on_line(line);
            if let Some(edit) = edit_one_line(&buffer, nonblank, indent, tab_width) {
                edits.push(edit)
            }
        }
    }
    apply_edits(buffer, cursor, edits)
}
