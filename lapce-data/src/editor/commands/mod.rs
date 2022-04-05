use xi_rope::RopeDelta;

use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    editor::commands::{
        insert_tab::InsertTabCommand, redo::RedoCommand, undo::UndoCommand,
    },
    movement::{Cursor, CursorMode},
};

#[cfg(test)]
pub mod test;

pub mod insert_tab;
pub mod redo;
pub mod undo;

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
