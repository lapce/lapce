
use xi_rope::RopeDelta;

use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    movement::{Cursor, Selection},
};
use super::indent;

pub struct InsertTabCommand<'a> {
    pub(super) selection: Selection,
    pub(super) cursor: &'a mut Cursor,
    pub(super) tab_width: usize,
}

impl<'a> InsertTabCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        let indent = buffer.indent_unit();
        let mut edits = Vec::new();

        for region in self.selection.regions() {
            if region.is_caret() {
                edits.push(indent::create_edit(
                    &buffer, region.start, indent, self.tab_width
                ))
            } else {
                let start_line = buffer.line_of_offset(region.min());
                let end_line = buffer.line_of_offset(region.max());
                for line in start_line..end_line + 1 {
                    let offset = buffer.first_non_blank_character_on_line(line);
                    edits.push(indent::create_edit(
                        &buffer, offset, indent, self.tab_width
                    ))
                }
            }
        }

        Some(indent::apply_edits(buffer, self.cursor, edits))
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

    #[test]
    fn insert_tab_aligns_to_tab_width() {
        let mut editor = MockEditor::new(
            r#"<$0>
 <$1>
  <$2>
   <$3>"#,
        );

        editor.command(EditCommandKind::InsertTab);

        assert_eq!(
            r#"    <$0>
    <$1>
    <$2>
    <$3>"#,
            editor.state()
        );
    }
}
