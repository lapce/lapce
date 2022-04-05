use xi_rope::RopeDelta;

use crate::{
    buffer::{
        data::{BufferDataListener, EditableBufferData},
        EditType,
    },
    movement::{Cursor, Selection},
};

pub struct InsertTabCommand<'a> {
    pub(super) selection: Selection,
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
}

impl<'a> InsertTabCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        mut buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        let indent = buffer.indent_unit();
        let mut edits = Vec::new();
        for region in self.selection.regions() {
            if region.is_caret() {
                if indent.starts_with('\t') {
                    edits.push((Selection::caret(region.start), indent.to_string()));
                } else {
                    let (_, col) =
                        buffer.offset_to_line_col(region.start, self.tab_width);
                    let indent = " ".repeat(indent.len() - col % indent.len());
                    edits.push((Selection::caret(region.start), indent));
                }
            } else {
                let start_line = buffer.line_of_offset(region.min());
                let end_line = buffer.line_of_offset(region.max());
                for line in start_line..end_line + 1 {
                    let offset = buffer.first_non_blank_character_on_line(line);
                    if indent.starts_with('\t') {
                        edits.push((Selection::caret(offset), indent.to_string()));
                    } else {
                        let (_, col) =
                            buffer.offset_to_line_col(offset, self.tab_width);
                        let indent = " ".repeat(indent.len() - col % indent.len());
                        edits.push((Selection::caret(offset), indent));
                    }
                }
            }
        }

        let edits = edits
            .iter()
            .map(|(selection, s)| (selection, s.as_str()))
            .collect::<Vec<(&Selection, &str)>>();

        let delta = buffer.edit_multiple(&edits, EditType::InsertChars);

        self.cursor.apply_delta(&delta);

        Some(delta)
    }
}

#[cfg(test)]
mod test {
    use crate::editor::commands::{test::MockEditor, EditCommandKind};

    #[test]
    fn insert_tab_inserts_spaces() {
        let mut editor = MockEditor::new("<$0>");

        editor.command(EditCommandKind::InsertTab);

        assert_eq!("    <$0>", editor.state());
    }

    #[test]
    fn insert_tab_inserts_at_multiple_places() {
        let mut editor = MockEditor::new(
            r#"<$0>
<$1>"#,
        );

        editor.command(EditCommandKind::InsertTab);

        assert_eq!(
            r#"    <$0>
    <$1>"#,
            editor.state()
        );
    }
}
