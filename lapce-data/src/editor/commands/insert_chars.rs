use lapce_core::syntax::Syntax;
use xi_rope::RopeDelta;

use crate::{
    buffer::{
        data::{BufferDataListener, EditableBufferData},
        get_word_property, matching_char, matching_pair_direction, EditType,
        WordProperty,
    },
    movement::{Cursor, CursorMode, InsertDrift, Selection},
};

pub struct InsertCharsCommand<'a> {
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
    pub(super) chars: &'a str,
    pub(super) syntax: Option<Syntax>,
}

impl<'a> InsertCharsCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        mut buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        let Self {
            cursor,
            tab_width,
            chars,
            syntax,
        } = self;

        let mut selection = cursor.edit_selection(buffer.buffer, tab_width);
        let cursor_char =
            buffer.buffer.char_at_offset(selection.get_cursor_offset());

        let mut content = chars.to_string();
        if chars.chars().count() == 1 {
            let c = chars.chars().next().unwrap();
            if !matching_pair_direction(c).unwrap_or(true) {
                if cursor_char == Some(c) {
                    let selection =
                        buffer.buffer.move_cursor_to_right(&selection, 1);

                    *cursor = Cursor::new(CursorMode::Insert(selection), None);
                    return None;
                }

                let offset = selection.get_cursor_offset();
                let line = buffer.buffer.line_of_offset(offset);
                let line_start = buffer.buffer.offset_of_line(line);
                if buffer.buffer.slice_to_cow(line_start..offset).trim() == "" {
                    if let Some(c) = matching_char(c) {
                        if let Some(previous_offset) = buffer
                            .buffer
                            .previous_unmatched(syntax.as_ref(), c, offset)
                        {
                            let previous_line =
                                buffer.buffer.line_of_offset(previous_offset);
                            let line_indent =
                                buffer.buffer.indent_on_line(previous_line);
                            content = line_indent + &content;
                            selection = Selection::region(line_start, offset);
                        }
                    }
                }
            }
        }

        let delta =
            buffer.edit_multiple(&[(&selection, &content)], EditType::InsertChars);
        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);

        *cursor = Cursor::new(CursorMode::Insert(selection.clone()), None);

        if chars.chars().count() == 1 {
            let c = chars.chars().next().unwrap();
            let is_whitespace_or_punct = cursor_char
                .map(|c| {
                    let prop = get_word_property(c);
                    prop == WordProperty::Lf
                        || prop == WordProperty::Space
                        || prop == WordProperty::Punctuation
                })
                .unwrap_or(true);

            if is_whitespace_or_punct && matching_pair_direction(c).unwrap_or(false)
            {
                if let Some(c) = matching_char(c) {
                    buffer.edit_multiple(
                        &[(&selection, &c.to_string())],
                        EditType::InsertChars,
                    );
                }
            }
        }

        None
    }
}
