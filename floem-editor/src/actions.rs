use floem::keyboard::ModifiersState;
use floem_editor_core::{
    command::{
        EditCommand, MotionModeCommand, MultiSelectionCommand, ScrollCommand,
    },
    cursor::Cursor,
    mode::MotionMode,
    movement::Movement,
    register::Register,
};

use crate::{
    command::{Command, CommandExecuted},
    editor::Editor,
    movement,
};

pub fn handle_command_default(
    ed: &Editor,
    action: &dyn CommonAction,
    cmd: &Command,
    count: Option<usize>,
    modifiers: ModifiersState,
) -> CommandExecuted {
    match cmd {
        Command::Edit(cmd) => handle_edit_command_default(ed, action, cmd),
        Command::Move(cmd) => {
            let movement = cmd.to_movement(count);
            handle_move_command_default(ed, action, movement, count, modifiers)
        }
        Command::Scroll(cmd) => {
            handle_scroll_command_default(ed, cmd, count, modifiers)
        }
        Command::MotionMode(cmd) => {
            handle_motion_mode_command_default(ed, action, cmd, count)
        }
        Command::MultiSelection(cmd) => {
            handle_multi_selection_command_default(ed, cmd)
        }
    }
}
fn handle_edit_command_default(
    ed: &Editor,
    action: &dyn CommonAction,
    cmd: &EditCommand,
) -> CommandExecuted {
    let modal = ed.modal.get_untracked();
    let smart_tab = ed.smart_tab.get_untracked();
    let mut cursor = ed.cursor.get_untracked();
    let mut register = ed.register.get_untracked();

    let text = ed.rope_text();

    let yank_data =
        if let floem_editor_core::cursor::CursorMode::Visual { .. } = &cursor.mode {
            Some(cursor.yank(&text))
        } else {
            None
        };

    // TODO(floem-editor): Should we instead pass the editor so that it can grab
    // modal + smart-tab (etc) if it wants?
    let had_edits =
        action.do_edit(&mut cursor, cmd, modal, &mut register, smart_tab);

    if had_edits {
        if let Some(data) = yank_data {
            register.add_delete(data);
        }
    }

    ed.cursor.set(cursor);
    ed.register.set(register);

    CommandExecuted::Yes
}
fn handle_move_command_default(
    ed: &Editor,
    action: &dyn CommonAction,
    movement: Movement,
    count: Option<usize>,
    modifiers: ModifiersState,
) -> CommandExecuted {
    // TODO: should we track jump locations?

    ed.last_movement.set(movement.clone());

    let mut cursor = ed.cursor.get_untracked();
    let modify = modifiers.shift_key();
    ed.register.update(|register| {
        movement::move_cursor(
            ed,
            action,
            &mut cursor,
            &movement,
            count.unwrap_or(1),
            modify,
            register,
        )
    });

    ed.cursor.set(cursor);

    CommandExecuted::Yes
}

fn handle_scroll_command_default(
    ed: &Editor,
    cmd: &ScrollCommand,
    count: Option<usize>,
    mods: ModifiersState,
) -> CommandExecuted {
    match cmd {
        ScrollCommand::PageUp => {
            ed.page_move(false, mods);
        }
        ScrollCommand::PageDown => {
            ed.page_move(true, mods);
        }
        ScrollCommand::ScrollUp => ed.scroll(0.0, false, count.unwrap_or(1), mods),
        ScrollCommand::ScrollDown => {
            ed.scroll(0.0, true, count.unwrap_or(1), mods);
        }
        // TODO:
        ScrollCommand::CenterOfWindow => {}
        ScrollCommand::TopOfWindow => {}
        ScrollCommand::BottomOfWindow => {}
    }

    CommandExecuted::Yes
}

fn handle_motion_mode_command_default(
    ed: &Editor,
    action: &dyn CommonAction,
    cmd: &MotionModeCommand,
    count: Option<usize>,
) -> CommandExecuted {
    let count = count.unwrap_or(1);
    let motion_mode = match cmd {
        MotionModeCommand::MotionModeDelete => MotionMode::Delete { count },
        MotionModeCommand::MotionModeIndent => MotionMode::Indent,
        MotionModeCommand::MotionModeOutdent => MotionMode::Outdent,
        MotionModeCommand::MotionModeYank => MotionMode::Yank { count },
    };
    let mut cursor = ed.cursor.get_untracked();
    let mut register = ed.register.get_untracked();

    movement::do_motion_mode(action, &mut cursor, motion_mode, &mut register);

    ed.cursor.set(cursor);
    ed.register.set(register);

    CommandExecuted::Yes
}

fn handle_multi_selection_command_default(
    ed: &Editor,
    cmd: &MultiSelectionCommand,
) -> CommandExecuted {
    let mut cursor = ed.cursor.get_untracked();
    movement::do_multi_selection(ed, &mut cursor, cmd);
    ed.cursor.set(cursor);

    CommandExecuted::Yes
}

/// Trait for common actions needed for the default implementation of the
/// operations.
pub trait CommonAction {
    // TODO: should these just take Register as an RwSignal?
    // TODO: should these just take Cursor as an RwSignal?

    fn exec_motion_mode(
        &self,
        cursor: &mut Cursor,
        motion_mode: MotionMode,
        start: usize,
        end: usize,
        is_vertical: bool,
        register: &mut Register,
    );

    // TODO: should we have a more general cursor state structure?
    // since modal is about cursor, and register is sortof about cursor
    // but also there might be other state it wants. Should we just pass Editor to it?
    /// Perform an edit.  
    /// Returns `true` if there was any change.
    fn do_edit(
        &self,
        cursor: &mut Cursor,
        cmd: &EditCommand,
        modal: bool,
        register: &mut Register,
        smart_tab: bool,
    ) -> bool;
}
