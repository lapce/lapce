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
                get_first_selection_after(&self.cursor, &buffer, &delta)
            {
                *self.cursor = cursor;
            }
            Some(delta)
        } else {
            None
        }
    }
}
