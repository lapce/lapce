use std::collections::HashSet;

use xi_rope::RopeDelta;

use crate::{
    buffer::{
        data::{BufferDataListener, EditableBufferData},
        EditType,
    },
    movement::{Cursor, Selection},
};

pub struct IndentLineCommand<'a> {
    pub(super) selection: Option<Selection>,
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
}

impl<'a> IndentLineCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        mut buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        let Self {
            selection,
            cursor,
            tab_width,
        } = self;

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
                let new_indent = if indent.starts_with('\t') {
                    indent.to_string()
                } else {
                    let (_, col) = buffer.offset_to_line_col(nonblank, tab_width);
                    " ".repeat(indent.len() - col % indent.len())
                };
                edits.push((Selection::caret(nonblank), new_indent));
            }
        }

        let edits = edits
            .iter()
            .map(|(selection, s)| (selection, s.as_str()))
            .collect::<Vec<(&Selection, &str)>>();

        let delta = buffer.edit_multiple(&edits, EditType::InsertChars);
        cursor.apply_delta(&delta);

        Some(delta)
    }
}

#[cfg(test)]
mod test {
    use crate::editor::commands::{test::MockEditor, EditCommandKind};

    #[test]
    fn indent_single_line() {
        let mut editor = MockEditor::new("line\n<$0>foo</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n    <$0>foo</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_indents_to_next_width_if_leading_space_is_selected() {
        // indent width is 4 characters, this test case intentionally has 3 spaces
        let mut editor = MockEditor::new("line\n<$0>   foo</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n<$0>    foo</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_partially_indented_line_indents_to_next_width() {
        // indent width is 4 characters, this test case intentionally has 3 spaces
        let mut editor = MockEditor::new("line\n   fo<$0>o</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n    fo<$0>o</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_line_indents_partially_selected() {
        let mut editor = MockEditor::new("line\nf<$0>o</$0>o\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("line\n    f<$0>o</$0>o\nthird line", editor.state());
    }

    #[test]
    fn indent_multiple_lines() {
        let mut editor = MockEditor::new("li<$0>ne\nfoo</$0>\nthird line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!("    li<$0>ne\n    foo</$0>\nthird line", editor.state());
    }

    #[test]
    fn indent_multiple_selections() {
        let mut editor = MockEditor::new("li<$0>n</$0>e\nfoo\nth<$1>ir</$1>d line");

        editor.command(EditCommandKind::IndentLine { selection: None });

        assert_eq!(
            "    li<$0>n</$0>e\nfoo\n    th<$1>ir</$1>d line",
            editor.state()
        );
    }
}
