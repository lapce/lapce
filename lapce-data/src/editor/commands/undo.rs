use xi_rope::{RopeDelta, Transformer};

use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    movement::{Cursor, CursorMode, Selection},
};

pub struct UndoCommand<'a> {
    pub(super) cursor: &'a mut Cursor,
}

impl<'a> UndoCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        mut buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        if let Some(delta) = buffer.do_undo() {
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

/// Computes where the cursor should be after the undo/redo operation
pub(super) fn get_first_selection_after<'a, L: BufferDataListener>(
    cursor: &Cursor,
    buffer: &EditableBufferData<'a, L>,
    delta: &RopeDelta,
) -> Option<Cursor> {
    let mut transformer = Transformer::new(delta);

    let offset = cursor.offset();
    let offset = transformer.transform(offset, false);
    let (ins, del) = delta.clone().factor();
    let ins = ins.transform_shrink(&del);
    for el in ins.els.iter() {
        match el {
            xi_rope::DeltaElement::Copy(b, e) => {
                // if b == e, ins.inserted_subset() will panic
                if b == e {
                    return None;
                }
            }
            xi_rope::DeltaElement::Insert(_) => {}
        }
    }

    // TODO it's silly to store the whole thing in memory, we only need the first element.
    let mut positions = ins
        .inserted_subset()
        .complement_iter()
        .map(|s| s.1)
        .collect::<Vec<usize>>();
    positions.append(
        &mut del
            .complement_iter()
            .map(|s| transformer.transform(s.1, false))
            .collect::<Vec<usize>>(),
    );
    positions.sort_by_key(|p| {
        let p = *p as i32 - offset as i32;
        if p > 0 {
            p as usize
        } else {
            -p as usize
        }
    });

    positions
        .get(0)
        .cloned()
        .map(Selection::caret)
        .map(|selection| {
            let cursor_mode = match cursor.mode {
                CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                    let offset = selection.min_offset();
                    let offset = buffer.offset_line_end(offset, false).min(offset);
                    CursorMode::Normal(offset)
                }
                CursorMode::Insert(_) => CursorMode::Insert(selection),
            };

            Cursor::new(cursor_mode, None)
        })
}
