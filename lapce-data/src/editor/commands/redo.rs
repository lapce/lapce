use xi_rope::RopeDelta;

use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    editor::commands::undo::get_first_selection_after,
    movement::Cursor,
};

pub struct RedoCommand<'a> {
    pub(super) cursor: &'a mut Cursor,
}

impl<'a> RedoCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        mut buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        if let Some(delta) = buffer.do_redo() {
            if let Some(cursor) =
                get_first_selection_after(self.cursor, &buffer, &delta)
            {
                *self.cursor = cursor;
            }
            Some(delta)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use crate::editor::commands::{test::MockEditor, EditCommandKind};

    #[test]
    fn redo_doesnt_do_anything_when_there_is_nothing_to_redo() {
        let mut editor = MockEditor::new("<$0>");

        editor.command(EditCommandKind::Redo);

        assert_eq!("<$0>", editor.state());
    }

    #[test]
    fn redo_doesnt_do_anything_when_the_last_command_is_not_an_undo() {
        let mut editor = MockEditor::new("foobar<$0>");

        editor.command(EditCommandKind::InsertTab);
        editor.command(EditCommandKind::Undo);

        // Insert a tab. The command shouldn't matter here, just that the internal state changes.
        editor.command(EditCommandKind::InsertTab);
        let state_before_redo = editor.state();

        editor.command(EditCommandKind::Redo);
        assert_eq!(state_before_redo, editor.state());
    }

    #[test]
    fn redo_reverts_last_undo() {
        let mut editor = MockEditor::new("<$0>");

        // Insert a tab. The command shouldn't matter here, just that the internal state changes.
        editor.command(EditCommandKind::InsertTab);
        let state_before_undo = editor.state();

        editor.command(EditCommandKind::Undo);
        editor.command(EditCommandKind::Redo);

        assert_eq!(state_before_undo, editor.state());
    }
}
