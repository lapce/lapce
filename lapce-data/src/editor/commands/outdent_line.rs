use std::collections::HashSet;

use xi_rope::RopeDelta;

use crate::{
    buffer::{
        data::{BufferDataListener, EditableBufferData},
        EditType,
    },
    movement::{Cursor, Selection},
};

pub struct OutdentLineCommand<'a> {
    pub(super) selection: Option<Selection>,
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
}

impl<'a> OutdentLineCommand<'a> {
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
            .unwrap_or_else(|| cursor.edit_selection(&buffer.buffer, tab_width));

        let indent = buffer.indent_unit();
        let mut edits = Vec::new();

        let mut lines = HashSet::new();
        for region in selection.regions() {
            let start_line = buffer.buffer.line_of_offset(region.min());
            let mut end_line = buffer.buffer.line_of_offset(region.max());
            if end_line > start_line {
                let end_line_start = buffer.buffer.offset_of_line(end_line);
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
                let nonblank = buffer.buffer.first_non_blank_character_on_line(line);
                let (_, col) = buffer.buffer.offset_to_line_col(nonblank, tab_width);
                if col == 0 {
                    continue;
                }

                if indent.starts_with('\t') {
                    edits.push((
                        Selection::region(nonblank - 1, nonblank),
                        "".to_string(),
                    ));
                } else {
                    let r = col % indent.len();
                    let r = if r == 0 { indent.len() } else { r };
                    edits.push((
                        Selection::region(nonblank - r, nonblank),
                        "".to_string(),
                    ));
                }
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
    fn outdent_does_nothing_if_line_is_not_indented() {
        let mut editor = MockEditor::new("line\n<$0>foo</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>", editor.state());
    }

    #[test]
    fn outdent_single_indented_line() {
        let mut editor = MockEditor::new("line\n<$0>    foo</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>", editor.state());
    }

    #[test]
    fn outdent_partially_selected_line() {
        let mut editor = MockEditor::new("line\n    f<$0>o</$0>o");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\nf<$0>o</$0>o", editor.state());
    }

    #[test]
    fn outdent_removes_incomplete_indentation() {
        // indent width is 4 characters, this test case intentionally has 3 spaces
        let mut editor = MockEditor::new("line\n<$0>   foo</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>", editor.state());
    }

    #[test]
    fn outdent_multiple_lines() {
        let mut editor = MockEditor::new("line\n<$0>    foo\n    bar</$0>");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo\nbar</$0>", editor.state());
    }

    #[test]
    fn outdent_multiple_selections() {
        let mut editor = MockEditor::new("line\n<$0>    foo</$0>\n    b<$1>a</$1>r");

        editor.command(EditCommandKind::OutdentLine { selection: None });

        assert_eq!("line\n<$0>foo</$0>\nb<$1>a</$1>r", editor.state());
    }
}
