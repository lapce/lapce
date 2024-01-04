pub mod key;
pub mod press;

use std::{collections::HashMap, rc::Rc, str::FromStr};

use floem::{
    keyboard::{KeyEvent, ModifiersState},
    reactive::RwSignal,
};
use lapce_core::command::{
    EditCommand, FocusCommand2, MoveCommand, MultiSelectionCommand,
};

use crate::{
    command::{Command, CommandExecuted},
    editor::Editor,
};

use self::{key::KeyInput, press::KeyPress};

/// The default keymap handler does not have modal-mode specific
/// keybindings.
#[derive(Clone)]
pub struct KeypressMap {
    pub keymaps: HashMap<KeyPress, Command>,
}
impl KeypressMap {
    pub fn default_windows() -> Self {
        let mut keymaps = HashMap::new();
        add_default_common(&mut keymaps);
        add_default_windows(&mut keymaps);
        Self { keymaps }
    }

    pub fn default_macos() -> Self {
        let mut keymaps = HashMap::new();
        add_default_common(&mut keymaps);
        add_default_macos(&mut keymaps);
        Self { keymaps }
    }

    pub fn default_linux() -> Self {
        let mut keymaps = HashMap::new();
        add_default_common(&mut keymaps);
        add_default_linux(&mut keymaps);
        Self { keymaps }
    }
}
impl Default for KeypressMap {
    fn default() -> Self {
        match std::env::consts::OS {
            "macos" => Self::default_macos(),
            "windows" => Self::default_windows(),
            _ => Self::default_linux(),
        }
    }
}

fn key(s: &str, m: ModifiersState) -> KeyPress {
    KeyPress::new(KeyInput::from_str(s).unwrap(), m)
}

fn key_d(s: &str) -> KeyPress {
    key(s, ModifiersState::default())
}

fn add_default_common(c: &mut HashMap<KeyPress, Command>) {
    // Note: this should typically be kept in sync with Lapce's
    // `defaults/keymaps-common.toml`

    // --- Basic editing ---

    c.insert(
        key("up", ModifiersState::ALT),
        Command::Edit(EditCommand::MoveLineUp),
    );
    c.insert(
        key("down", ModifiersState::ALT),
        Command::Edit(EditCommand::MoveLineDown),
    );

    c.insert(key_d("delete"), Command::Edit(EditCommand::DeleteForward));
    c.insert(
        key_d("backspace"),
        Command::Edit(EditCommand::DeleteBackward),
    );
    c.insert(
        key("backspace", ModifiersState::SHIFT),
        Command::Edit(EditCommand::DeleteForward),
    );

    c.insert(key_d("home"), Command::Move(MoveCommand::LineStartNonBlank));
    c.insert(key_d("end"), Command::Move(MoveCommand::LineEnd));

    c.insert(key_d("pageup"), Command::Focus(FocusCommand2::PageUp));
    c.insert(key_d("pagedown"), Command::Focus(FocusCommand2::PageDown));
    c.insert(
        key("pageup", ModifiersState::CONTROL),
        Command::Focus(FocusCommand2::ScrollUp),
    );
    c.insert(
        key("pagedown", ModifiersState::CONTROL),
        Command::Focus(FocusCommand2::ScrollDown),
    );

    // --- Multi cursor ---

    c.insert(
        key("i", ModifiersState::ALT | ModifiersState::SHIFT),
        Command::MultiSelection(MultiSelectionCommand::InsertCursorEndOfLine),
    );

    // TODO: should we have jump location backward/forward?

    // TODO: jump to snippet positions?

    // --- ---- ---
    c.insert(key_d("right"), Command::Move(MoveCommand::Right));
    c.insert(key_d("left"), Command::Move(MoveCommand::Left));
    c.insert(key_d("up"), Command::Move(MoveCommand::Up));
    c.insert(key_d("down"), Command::Move(MoveCommand::Down));

    c.insert(key_d("enter"), Command::Edit(EditCommand::InsertNewLine));

    c.insert(key_d("tab"), Command::Edit(EditCommand::InsertTab));

    c.insert(
        key("up", ModifiersState::ALT | ModifiersState::SHIFT),
        Command::Edit(EditCommand::DuplicateLineUp),
    );
    c.insert(
        key("down", ModifiersState::ALT | ModifiersState::SHIFT),
        Command::Edit(EditCommand::DuplicateLineDown),
    );
}

fn add_default_windows(c: &mut HashMap<KeyPress, Command>) {
    add_default_nonmacos(c);
}

fn add_default_macos(c: &mut HashMap<KeyPress, Command>) {
    // Note: this should typically be kept in sync with Lapce's
    // `defaults/keymaps-macos.toml`

    // --- Basic editing ---
    c.insert(
        key("z", ModifiersState::SUPER),
        Command::Edit(EditCommand::Undo),
    );
    c.insert(
        key("z", ModifiersState::SUPER | ModifiersState::SHIFT),
        Command::Edit(EditCommand::Redo),
    );
    c.insert(
        key("y", ModifiersState::SUPER),
        Command::Edit(EditCommand::Redo),
    );
    c.insert(
        key("x", ModifiersState::SUPER),
        Command::Edit(EditCommand::ClipboardCut),
    );
    c.insert(
        key("c", ModifiersState::SUPER),
        Command::Edit(EditCommand::ClipboardCopy),
    );
    c.insert(
        key("v", ModifiersState::SUPER),
        Command::Edit(EditCommand::ClipboardPaste),
    );

    c.insert(
        key("right", ModifiersState::ALT),
        Command::Move(MoveCommand::WordEndForward),
    );
    c.insert(
        key("left", ModifiersState::ALT),
        Command::Move(MoveCommand::WordBackward),
    );
    c.insert(
        key("left", ModifiersState::SUPER),
        Command::Move(MoveCommand::LineStartNonBlank),
    );
    c.insert(
        key("right", ModifiersState::SUPER),
        Command::Move(MoveCommand::LineEnd),
    );

    c.insert(
        key("a", ModifiersState::CONTROL),
        Command::Move(MoveCommand::LineStartNonBlank),
    );
    c.insert(
        key("e", ModifiersState::CONTROL),
        Command::Move(MoveCommand::LineEnd),
    );

    c.insert(
        key("k", ModifiersState::SUPER | ModifiersState::SHIFT),
        Command::Edit(EditCommand::DeleteLine),
    );

    c.insert(
        key("backspace", ModifiersState::ALT),
        Command::Edit(EditCommand::DeleteWordBackward),
    );
    c.insert(
        key("backspace", ModifiersState::SUPER),
        Command::Edit(EditCommand::DeleteToBeginningOfLine),
    );
    c.insert(
        key("k", ModifiersState::CONTROL),
        Command::Edit(EditCommand::DeleteToEndOfLine),
    );
    c.insert(
        key("delete", ModifiersState::ALT),
        Command::Edit(EditCommand::DeleteWordForward),
    );

    // TODO: match pairs?
    // TODO: indent/outdent line?

    c.insert(
        key("a", ModifiersState::SUPER),
        Command::MultiSelection(MultiSelectionCommand::SelectAll),
    );

    c.insert(
        key("enter", ModifiersState::SUPER),
        Command::Edit(EditCommand::NewLineBelow),
    );
    c.insert(
        key("enter", ModifiersState::SUPER | ModifiersState::SHIFT),
        Command::Edit(EditCommand::NewLineAbove),
    );

    // --- Multi cursor ---
    c.insert(
        key("up", ModifiersState::ALT | ModifiersState::SUPER),
        Command::MultiSelection(MultiSelectionCommand::InsertCursorAbove),
    );
    c.insert(
        key("down", ModifiersState::ALT | ModifiersState::SUPER),
        Command::MultiSelection(MultiSelectionCommand::InsertCursorBelow),
    );

    c.insert(
        key("l", ModifiersState::SUPER),
        Command::MultiSelection(MultiSelectionCommand::SelectCurrentLine),
    );
    c.insert(
        key("l", ModifiersState::SUPER | ModifiersState::SHIFT),
        Command::MultiSelection(MultiSelectionCommand::SelectAllCurrent),
    );

    c.insert(
        key("u", ModifiersState::SUPER),
        Command::MultiSelection(MultiSelectionCommand::SelectUndo),
    );

    // --- ---- ---
    c.insert(
        key("up", ModifiersState::SUPER),
        Command::Move(MoveCommand::DocumentStart),
    );
    c.insert(
        key("down", ModifiersState::SUPER),
        Command::Move(MoveCommand::DocumentEnd),
    );
}

fn add_default_linux(c: &mut HashMap<KeyPress, Command>) {
    add_default_nonmacos(c);
}

fn add_default_nonmacos(c: &mut HashMap<KeyPress, Command>) {
    // Note: this should typically be kept in sync with Lapce's
    // `defaults/keymaps-nonmacos.toml`

    // --- Basic editing ---
    c.insert(
        key("z", ModifiersState::CONTROL),
        Command::Edit(EditCommand::Undo),
    );
    c.insert(
        key("z", ModifiersState::CONTROL | ModifiersState::SHIFT),
        Command::Edit(EditCommand::Redo),
    );
    c.insert(
        key("y", ModifiersState::CONTROL),
        Command::Edit(EditCommand::Redo),
    );
    c.insert(
        key("x", ModifiersState::CONTROL),
        Command::Edit(EditCommand::ClipboardCut),
    );
    c.insert(
        key("delete", ModifiersState::SHIFT),
        Command::Edit(EditCommand::ClipboardCut),
    );
    c.insert(
        key("c", ModifiersState::CONTROL),
        Command::Edit(EditCommand::ClipboardCopy),
    );
    c.insert(
        key("insert", ModifiersState::CONTROL),
        Command::Edit(EditCommand::ClipboardCopy),
    );
    c.insert(
        key("v", ModifiersState::CONTROL),
        Command::Edit(EditCommand::ClipboardPaste),
    );
    c.insert(
        key("insert", ModifiersState::SHIFT),
        Command::Edit(EditCommand::ClipboardPaste),
    );

    c.insert(
        key("right", ModifiersState::CONTROL),
        Command::Move(MoveCommand::WordEndForward),
    );
    c.insert(
        key("left", ModifiersState::CONTROL),
        Command::Move(MoveCommand::WordBackward),
    );

    c.insert(
        key("backspace", ModifiersState::CONTROL),
        Command::Edit(EditCommand::DeleteWordBackward),
    );
    c.insert(
        key("delete", ModifiersState::CONTROL),
        Command::Edit(EditCommand::DeleteWordForward),
    );

    // TODO: match pairs?

    // TODO: indent/outdent line?

    c.insert(
        key("a", ModifiersState::CONTROL),
        Command::MultiSelection(MultiSelectionCommand::SelectAll),
    );

    c.insert(
        key("enter", ModifiersState::CONTROL),
        Command::Edit(EditCommand::NewLineAbove),
    );

    // --- Multi cursor ---
    c.insert(
        key("up", ModifiersState::CONTROL | ModifiersState::ALT),
        Command::MultiSelection(MultiSelectionCommand::InsertCursorAbove),
    );
    c.insert(
        key("down", ModifiersState::CONTROL | ModifiersState::ALT),
        Command::MultiSelection(MultiSelectionCommand::InsertCursorBelow),
    );

    c.insert(
        key("l", ModifiersState::CONTROL),
        Command::MultiSelection(MultiSelectionCommand::SelectCurrentLine),
    );
    c.insert(
        key("l", ModifiersState::CONTROL | ModifiersState::SHIFT),
        Command::MultiSelection(MultiSelectionCommand::SelectAllCurrent),
    );

    c.insert(
        key("u", ModifiersState::CONTROL),
        Command::MultiSelection(MultiSelectionCommand::SelectUndo),
    );
}

pub fn default_key_handler(
    editor: RwSignal<Rc<Editor>>,
) -> impl Fn(&KeyPress, ModifiersState) -> CommandExecuted + 'static {
    let keypress_map = KeypressMap::default();
    move |keypress, modifiers| {
        let Some(command) = keypress_map.keymaps.get(&keypress) else {
            return CommandExecuted::No;
        };

        editor.with_untracked(|editor| {
            editor
                .doc()
                .run_command(&editor, command, Some(1), modifiers)
        })
    }
}
