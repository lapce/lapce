//! Mock implementations necessary for automated tests

mod test_state;

mod commands;

use std::path::PathBuf;

use test_state::TestState;
use xi_rope::RopeDelta;

use crate::{
    buffer::{
        data::{BufferData, BufferDataListener, EditableBufferData},
        BufferContent,
    },
    editor::commands::{EditCommandFactory, EditCommandKind},
    movement::{Cursor, CursorMode},
};

pub struct MockEditor {
    buffer: BufferData,
    cursor: Cursor,
}

struct DefaultListener;
impl BufferDataListener for DefaultListener {
    fn should_apply_edit(&self) -> bool {
        true
    }

    fn on_edit_applied(&mut self, _buffer: &BufferData, _delta: &RopeDelta) {}
}

impl MockEditor {
    /// Constructs a new mock editor with the given initial state.
    pub fn new(initial: &str) -> Self {
        let state = TestState::parse(initial);
        Self::from_state(state)
    }

    /// Constructs a new mock editor with the given initial state.
    pub fn from_state(initial: TestState) -> Self {
        Self {
            buffer: BufferData::new(
                &initial.contents,
                BufferContent::File(PathBuf::default()),
            ),
            cursor: Cursor {
                mode: CursorMode::Insert(initial.selection),
                horiz: None,
            },
        }
    }

    /// Retrieves the visible editor state.
    pub fn state(&self) -> TestState {
        let selection = match &self.cursor.mode {
            CursorMode::Insert(selection) => selection.clone(),
            // not yet supported
            CursorMode::Visual { .. } | CursorMode::Normal(_) => unimplemented!(),
        };
        TestState {
            contents: self.buffer.rope().to_string(),
            selection,
        }
    }

    /// Executes a command in the editor.
    pub fn command(&mut self, command: EditCommandKind) {
        let buffer = EditableBufferData {
            listener: DefaultListener,
            buffer: &mut self.buffer,
        };

        let factory = EditCommandFactory {
            cursor: &mut self.cursor,
            tab_width: 4,
        };
        if let Some(edit_command) = factory.create_command(command) {
            edit_command.execute(buffer);
        }
    }
}
