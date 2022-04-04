use xi_rope::RopeDelta;

use crate::{
    buffer::{
        data::{BufferDataListener, EditableBufferData},
        EditType,
    },
    command::LapceCommand,
    movement::{Cursor, CursorMode, Selection},
};

/// This structure handles text editing commands.
pub struct EditCommandFactory<'a> {
    pub cursor: &'a mut Cursor,
    pub tab_width: usize,
}

impl<'a> EditCommandFactory<'a> {
    pub fn create_command(self, command: &LapceCommand) -> Option<EditCommand<'a>> {
        match command {
            LapceCommand::InsertTab => {
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
            _ => None,
        }
    }
}

pub enum EditCommand<'a> {
    InsertTab(InsertTabCommand<'a>),
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
    ) -> RopeDelta {
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

        delta
    }
}

impl<'a> EditCommand<'a> {
    pub fn execute<L: BufferDataListener>(
        self,
        buffer: EditableBufferData<'a, L>,
    ) -> RopeDelta {
        match self {
            Self::InsertTab(command) => command.execute(buffer),
        }
    }
}
