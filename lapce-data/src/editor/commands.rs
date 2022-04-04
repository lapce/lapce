use xi_rope::{RopeDelta, Transformer};

use crate::{
    buffer::{
        data::{BufferDataListener, EditableBufferData},
        EditType,
    },
    movement::{Cursor, CursorMode, Selection},
};

/// This structure handles text editing commands.
pub struct EditCommandFactory<'a> {
    pub cursor: &'a mut Cursor,
    pub tab_width: usize,
}

impl<'a> EditCommandFactory<'a> {
    pub fn create_command(
        self,
        command: EditCommandKind,
    ) -> Option<EditCommand<'a>> {
        match command {
            EditCommandKind::InsertTab => {
                if let CursorMode::Insert(selection) = &self.cursor.mode {
                    Some(EditCommand::InsertTab(InsertTabCommand {
                        selection: selection.clone(),
                        cursor: self.cursor,
                        tab_width: self.tab_width,
                    }))
                } else {
                    None
                }
            }
            EditCommandKind::Undo => Some(EditCommand::Undo(UndoCommand {
                cursor: self.cursor,
            })),
            EditCommandKind::Redo => Some(EditCommand::Redo(RedoCommand {
                cursor: self.cursor,
            })),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditCommandKind {
    InsertTab,
    Undo,
    Redo,
}

pub enum EditCommand<'a> {
    InsertTab(InsertTabCommand<'a>),
    Undo(UndoCommand<'a>),
    Redo(RedoCommand<'a>),
}

impl<'a> EditCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        buffer: EditableBufferData<'a, L>,
    ) -> Option<RopeDelta> {
        match self {
            Self::InsertTab(command) => command.execute(buffer),
            Self::Undo(command) => command.execute(buffer),
            Self::Redo(command) => command.execute(buffer),
        }
    }
}

pub struct InsertTabCommand<'a> {
    selection: Selection,
    cursor: &'a mut Cursor,
    tab_width: usize,
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

/// Computes where the cursor should be after the undo operation
fn get_first_selection_after<'a, L: BufferDataListener>(
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

pub struct UndoCommand<'a> {
    cursor: &'a mut Cursor,
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

pub struct RedoCommand<'a> {
    cursor: &'a mut Cursor,
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
