use lapce_core::{mode::Mode, syntax::Syntax};
use xi_rope::RopeDelta;

use crate::{
    buffer::data::{BufferDataListener, EditableBufferData},
    editor::commands::{
        indent_line::IndentLineCommand, insert_chars::InsertCharsCommand,
        insert_tab::InsertTabCommand, outdent_line::OutdentLineCommand,
        redo::RedoCommand, undo::UndoCommand,
    },
    movement::{Cursor, CursorMode, Selection},
};

#[cfg(test)]
pub mod test;

pub mod insert_chars;
pub mod insert_tab;
pub mod redo;
pub mod undo;

pub mod indent_line;
pub mod outdent_line;

mod indentation;

/// This structure handles text editing commands.
pub struct EditCommandFactory<'a> {
    pub cursor: &'a mut Cursor,
    pub syntax: Option<Syntax>,
    pub tab_width: usize,
}

impl<'a> EditCommandFactory<'a> {
    pub fn create_command(
        self,
        command: EditCommandKind<'a>,
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
            EditCommandKind::IndentLine { selection } => {
                Some(EditCommand::IndentLine(IndentLineCommand {
                    selection,
                    cursor: self.cursor,
                    tab_width: self.tab_width,
                }))
            }
            EditCommandKind::OutdentLine { selection } => {
                Some(EditCommand::OutdentLine(OutdentLineCommand {
                    selection,
                    cursor: self.cursor,
                    tab_width: self.tab_width,
                }))
            }
            EditCommandKind::InsertChars { chars } => {
                if self.cursor.get_mode() == Mode::Insert {
                    Some(EditCommand::InsertChars(InsertCharsCommand {
                        cursor: self.cursor,
                        tab_width: self.tab_width,
                        syntax: self.syntax,
                        chars,
                    }))
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Clone)]
pub enum EditCommandKind<'a> {
    InsertTab,
    InsertChars { chars: &'a str },
    Undo,
    Redo,
    IndentLine { selection: Option<Selection> },
    OutdentLine { selection: Option<Selection> },
}

pub enum EditCommand<'a> {
    InsertTab(InsertTabCommand<'a>),
    Undo(UndoCommand<'a>),
    Redo(RedoCommand<'a>),
    IndentLine(IndentLineCommand<'a>),
    OutdentLine(OutdentLineCommand<'a>),
    InsertChars(InsertCharsCommand<'a>),
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
            Self::IndentLine(command) => command.execute(buffer),
            Self::OutdentLine(command) => command.execute(buffer),
            Self::InsertChars(command) => command.execute(buffer),
        }
    }
}
