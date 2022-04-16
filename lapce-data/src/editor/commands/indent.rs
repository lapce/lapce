use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    movement::Selection,
};
use lapce_core::indent::IndentStyle;

pub(super) fn create_edit<'s, 'b, L: BufferDataListener>(
    buffer: &EditableBufferData<'b, L>,
    offset: usize,
    indent: &'s str,
    tabwidth: usize,
) -> (Selection, &'s str) {
    let longest_indent = IndentStyle::LONGEST_INDENT;
    let indent = if indent.starts_with('\t') {
        indent
    } else {
        let (_, col) = buffer.offset_to_line_col(offset, tabwidth);
        longest_indent.split_at(indent.len() - col % indent.len()).0
    };
    (Selection::caret(offset), indent)
}
