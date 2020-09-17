use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use druid::{ExtEventSink, KeyEvent, Modifiers, Target};
use lazy_static::lazy_static;

use crate::{
    command::{CraneCommand, CRANE_COMMAND},
    palette::PaletteState,
};

lazy_static! {
    pub static ref CRANE_STATE: CraneState = CraneState::new();
}

#[derive(Clone, PartialEq)]
pub enum CraneWidget {
    Palette,
    Editor,
}

#[derive(Clone)]
pub struct CraneState {
    pub palette: Arc<Mutex<PaletteState>>,
    keypress_sequence: Arc<Mutex<String>>,
    pending_keypress: Arc<Mutex<Vec<String>>>,
    keymaps: Arc<HashMap<Vec<String>, String>>,
    pub last_focus: Arc<Mutex<CraneWidget>>,
    pub focus: Arc<Mutex<CraneWidget>>,
    pub ui_sink: Arc<Mutex<Option<ExtEventSink>>>,
}

impl CraneState {
    pub fn new() -> CraneState {
        let mut keymaps = HashMap::new();
        keymaps.insert(
            vec!["ctrl+meta+p".to_string()],
            CraneCommand::Palette.to_string(),
        );
        keymaps.insert(
            vec!["ctrl+m".to_string()],
            CraneCommand::ListSelect.to_string(),
        );
        keymaps.insert(
            vec!["ctrl+n".to_string()],
            CraneCommand::ListNext.to_string(),
        );
        keymaps.insert(
            vec!["arrowdown".to_string()],
            CraneCommand::ListNext.to_string(),
        );
        keymaps.insert(
            vec!["arrowup".to_string()],
            CraneCommand::ListPrevious.to_string(),
        );
        keymaps
            .insert(vec!["ctrl+b".to_string()], CraneCommand::Left.to_string());
        keymaps.insert(
            vec!["ctrl+f".to_string()],
            CraneCommand::Right.to_string(),
        );
        keymaps.insert(
            vec!["arrowleft".to_string()],
            CraneCommand::Left.to_string(),
        );
        keymaps.insert(
            vec!["arrowright".to_string()],
            CraneCommand::Right.to_string(),
        );
        keymaps.insert(
            vec!["ctrl+p".to_string()],
            CraneCommand::ListPrevious.to_string(),
        );
        keymaps.insert(
            vec!["enter".to_string()],
            CraneCommand::ListSelect.to_string(),
        );
        keymaps.insert(
            vec!["escape".to_string()],
            CraneCommand::PaletteCancel.to_string(),
        );
        keymaps.insert(
            vec!["ctrl+u".to_string()],
            CraneCommand::DeleteToBeginningOfLine.to_string(),
        );
        keymaps.insert(
            vec!["ctrl+h".to_string()],
            CraneCommand::DeleteBackward.to_string(),
        );
        keymaps.insert(
            vec!["backspace".to_string()],
            CraneCommand::DeleteBackward.to_string(),
        );
        CraneState {
            pending_keypress: Arc::new(Mutex::new(Vec::new())),
            keymaps: Arc::new(keymaps),
            keypress_sequence: Arc::new(Mutex::new("".to_string())),
            ui_sink: Arc::new(Mutex::new(None)),
            focus: Arc::new(Mutex::new(CraneWidget::Editor)),
            last_focus: Arc::new(Mutex::new(CraneWidget::Editor)),
            palette: Arc::new(Mutex::new(PaletteState::new())),
        }
    }

    pub fn insert(&self, content: &str) {
        self.palette.lock().unwrap().insert(content);
    }

    pub fn run_command(&self, command: &str) {
        if let Ok(cmd) = CraneCommand::from_str(command) {
            match cmd {
                CraneCommand::Palette => {
                    self.palette.lock().unwrap().run();
                }
                CraneCommand::PaletteCancel => {
                    self.palette.lock().unwrap().cancel();
                }
                CraneCommand::ListSelect => {
                    self.palette.lock().unwrap().select();
                }
                CraneCommand::ListNext => {
                    self.palette.lock().unwrap().change_index(1);
                }
                CraneCommand::ListPrevious => {
                    self.palette.lock().unwrap().change_index(-1);
                }
                CraneCommand::Left => {
                    self.palette.lock().unwrap().move_cursor(-1);
                }
                CraneCommand::Right => {
                    self.palette.lock().unwrap().move_cursor(1);
                }
                CraneCommand::DeleteBackward => {
                    self.palette.lock().unwrap().delete_backward();
                }
                CraneCommand::DeleteToBeginningOfLine => {
                    self.palette.lock().unwrap().delete_to_beginning_of_line();
                }
                _ => println!("unhandled command {}", command),
            };
        }
    }

    pub fn key_down(&self, key_event: &KeyEvent) {
        let key = match &key_event.key {
            druid::keyboard_types::Key::Character(c) => &c,
            druid::keyboard_types::Key::Enter => "enter",
            druid::keyboard_types::Key::Tab => "tab",
            druid::keyboard_types::Key::ArrowDown => "arrowdown",
            druid::keyboard_types::Key::ArrowLeft => "arrowleft",
            druid::keyboard_types::Key::ArrowRight => "arrowright",
            druid::keyboard_types::Key::ArrowUp => "arrowup",
            druid::keyboard_types::Key::End => "end",
            druid::keyboard_types::Key::Home => "home",
            druid::keyboard_types::Key::PageDown => "pagedown",
            druid::keyboard_types::Key::PageUp => "pageup",
            druid::keyboard_types::Key::Backspace => "backspace",
            druid::keyboard_types::Key::Delete => "delete",
            druid::keyboard_types::Key::Escape => "escape",
            druid::keyboard_types::Key::F1 => "f1",
            druid::keyboard_types::Key::F2 => "f2",
            druid::keyboard_types::Key::F3 => "f3",
            druid::keyboard_types::Key::F4 => "f4",
            druid::keyboard_types::Key::F5 => "f5",
            druid::keyboard_types::Key::F6 => "f6",
            druid::keyboard_types::Key::F7 => "f7",
            druid::keyboard_types::Key::F8 => "f8",
            druid::keyboard_types::Key::F9 => "f9",
            druid::keyboard_types::Key::F10 => "f10",
            druid::keyboard_types::Key::F11 => "f11",
            druid::keyboard_types::Key::F12 => "f12",
            _ => return,
        };

        *self.keypress_sequence.lock().unwrap() =
            uuid::Uuid::new_v4().to_string();

        // let keypress = self.pending_keypress.lock().unwrap().clone() +

        let mut keypress = self.pending_keypress.lock().unwrap().clone();
        keypress.push(format!(
            "{}{}{}{}{}",
            if key_event.mods.alt() { "alt+" } else { "" },
            if key_event.mods.ctrl() { "ctrl+" } else { "" },
            if key_event.mods.meta() { "meta+" } else { "" },
            if key_event.mods.shift() { "shift+" } else { "" },
            key.to_lowercase(),
        ));

        for (key, value) in self.keymaps.iter() {
            if keypress.len() < key.len()
                && keypress.as_slice() == &key[..keypress.len()]
            {
                *self.pending_keypress.lock().unwrap() = keypress.clone();
                let pending_keypress = self.pending_keypress.clone();
                let keypress_sequence = self.keypress_sequence.clone();
                let keymaps = self.keymaps.clone();
                let crane_state = self.clone();
                thread::spawn(move || {
                    let pre_keypress_sequence =
                        { keypress_sequence.lock().unwrap().to_string() };
                    thread::sleep(Duration::from_millis(3000));
                    let mut pending_keypress = pending_keypress.lock().unwrap();
                    let keypress_sequence = keypress_sequence.lock().unwrap();

                    if *keypress_sequence != pre_keypress_sequence {
                        return;
                    }

                    if let Some(value) = keymaps.get(&keypress) {
                        crane_state.run_command(value);
                    }
                    *pending_keypress = Vec::new();
                });

                return;
            }
        }

        if let Some(cmd) = self.keymaps.get(&keypress) {
            *self.pending_keypress.lock().unwrap() = Vec::new();
            self.run_command(cmd);
            return;
        }

        *self.pending_keypress.lock().unwrap() = Vec::new();

        let mut mods = key_event.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            match &key_event.key {
                druid::keyboard_types::Key::Character(c) => {
                    self.insert(c);
                }
                _ => (),
            }
        }
    }

    pub fn set_ui_sink(&self, ui_sink: ExtEventSink) {
        *self.ui_sink.lock().unwrap() = Some(ui_sink);
    }
}
