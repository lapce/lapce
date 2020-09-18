use std::{
    cell::Cell,
    collections::HashMap,
    fs::File,
    io::Read,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use druid::{ExtEventSink, KeyEvent, Modifiers, Target, WidgetId};
use lazy_static::lazy_static;
use toml;

use crate::{
    buffer::Buffer,
    buffer::BufferId,
    command::CraneUICommand,
    command::CRANE_UI_COMMAND,
    command::{CraneCommand, CRANE_COMMAND},
    editor::EditorSplitState,
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

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Mode {
    Insert,
    Visual,
    Normal,
}

#[derive(PartialEq, Eq, Hash, Default, Clone)]
pub struct KeyPress {
    pub key: druid::keyboard_types::Key,
    pub mods: Modifiers,
}

#[derive(PartialEq, Eq, Hash)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Vec<Mode>,
    pub when: Option<String>,
    pub command: String,
}

#[derive(Clone)]
pub struct CraneState {
    pub palette: Arc<Mutex<PaletteState>>,
    keypress_sequence: Arc<Mutex<String>>,
    pending_keypress: Arc<Mutex<Vec<String>>>,
    keymaps: Arc<Mutex<Vec<KeyMap>>>,
    pub last_focus: Arc<Mutex<CraneWidget>>,
    pub focus: Arc<Mutex<CraneWidget>>,
    pub ui_sink: Arc<Mutex<Option<ExtEventSink>>>,
    pub editor_split: Arc<Mutex<EditorSplitState>>,
}

impl CraneState {
    pub fn new() -> CraneState {
        CraneState {
            pending_keypress: Arc::new(Mutex::new(Vec::new())),
            keymaps: Arc::new(Mutex::new(
                Self::get_keymaps().unwrap_or(Vec::new()),
            )),
            keypress_sequence: Arc::new(Mutex::new("".to_string())),
            ui_sink: Arc::new(Mutex::new(None)),
            focus: Arc::new(Mutex::new(CraneWidget::Editor)),
            last_focus: Arc::new(Mutex::new(CraneWidget::Editor)),
            palette: Arc::new(Mutex::new(PaletteState::new())),
            editor_split: Arc::new(Mutex::new(EditorSplitState::new())),
        }
    }

    fn get_keymaps() -> Result<Vec<KeyMap>> {
        let mut keymaps = Vec::new();
        let mut f = File::open("/Users/Lulu/crane/.crane/keymaps.toml")?;
        let mut content = vec![];
        f.read_to_end(&mut content)?;
        let toml_keymaps: toml::Value = toml::from_slice(&content)?;
        let toml_keymaps = toml_keymaps
            .get("keymaps")
            .and_then(|v| v.as_array())
            .ok_or(anyhow!("no keymaps"))?;

        for toml_keymap in toml_keymaps {
            if let Ok(keymap) = Self::get_keymap(toml_keymap) {
                keymaps.push(keymap);
            }
        }

        Ok(keymaps)
    }

    fn get_modes(toml_keymap: &toml::Value) -> Vec<Mode> {
        toml_keymap
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|m| {
                m.chars()
                    .filter_map(|c| {
                        match c.to_lowercase().to_string().as_ref() {
                            "i" => Some(Mode::Insert),
                            "n" => Some(Mode::Normal),
                            "v" => Some(Mode::Visual),
                            _ => None,
                        }
                    })
                    .collect()
            })
            .unwrap_or(Vec::new())
    }

    fn get_keymap(toml_keymap: &toml::Value) -> Result<KeyMap> {
        let key = toml_keymap
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or(anyhow!("no key in keymap"))?;
        let mut keypresses = Vec::new();
        for k in key.split(" ") {
            let mut keypress = KeyPress::default();
            for (i, part) in
                k.split("+").collect::<Vec<&str>>().iter().rev().enumerate()
            {
                if i == 0 {
                    keypress.key = match part.to_lowercase().as_ref() {
                        "escape" => druid::keyboard_types::Key::Escape,
                        "esc" => druid::keyboard_types::Key::Escape,
                        "delete" => druid::keyboard_types::Key::Delete,
                        "backspace" => druid::keyboard_types::Key::Backspace,
                        "bs" => druid::keyboard_types::Key::Backspace,
                        "arrowright" => druid::keyboard_types::Key::ArrowRight,
                        "arrowleft" => druid::keyboard_types::Key::ArrowLeft,
                        "tab" => druid::keyboard_types::Key::Tab,
                        "enter" => druid::keyboard_types::Key::Enter,
                        "del" => druid::keyboard_types::Key::Delete,
                        _ => druid::keyboard_types::Key::Character(
                            part.to_lowercase(),
                        ),
                    }
                } else {
                    match part.to_lowercase().as_ref() {
                        "ctrl" => keypress.mods.set(Modifiers::CONTROL, true),
                        "meta" => keypress.mods.set(Modifiers::META, true),
                        "shift" => keypress.mods.set(Modifiers::SHIFT, true),
                        "alt" => keypress.mods.set(Modifiers::ALT, true),
                        _ => (),
                    }
                }
            }
            keypresses.push(keypress);
        }

        Ok(KeyMap {
            key: keypresses,
            modes: Self::get_modes(toml_keymap),
            when: toml_keymap
                .get("when")
                .and_then(|w| w.as_str())
                .map(|w| w.to_string()),
            command: toml_keymap
                .get("command")
                .and_then(|c| c.as_str())
                .map(|w| w.trim().to_string())
                .unwrap_or("".to_string()),
        })
    }

    fn get_mode(&self) -> Mode {
        let foucus = self.focus.lock().unwrap().clone();
        match foucus {
            CraneWidget::Palette => Mode::Insert,
            CraneWidget::Editor => self.editor_split.lock().unwrap().get_mode(),
        }
    }

    pub fn insert(&self, content: &str) {
        let foucus = self.focus.lock().unwrap().clone();
        match foucus {
            CraneWidget::Palette => {
                self.palette.lock().unwrap().insert(content);
            }
            CraneWidget::Editor => {}
        }
    }

    pub fn run_command(&self, command: &str) {
        if let Ok(cmd) = CraneCommand::from_str(command) {
            let foucus = self.focus.lock().unwrap().clone();
            match cmd {
                CraneCommand::Palette => {
                    self.palette.lock().unwrap().run();
                }
                CraneCommand::PaletteCancel => {
                    self.palette.lock().unwrap().cancel();
                }
                _ => {
                    match foucus {
                        CraneWidget::Editor => {
                            self.editor_split.lock().unwrap().run_command(cmd)
                        }
                        CraneWidget::Palette => match cmd {
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
                                self.palette
                                    .lock()
                                    .unwrap()
                                    .delete_to_beginning_of_line();
                            }
                            _ => (),
                        },
                    };
                }
            };
        }
    }

    fn match_keymap(&self, keypress: &KeyPress, keymap: &KeyMap) -> bool {
        let keypress = vec![keypress.clone()];
        if keymap.key != keypress {
            return false;
        }

        let mode = self.get_mode();
        if !keymap.modes.is_empty() && !keymap.modes.contains(&mode) {
            return false;
        }

        if let Some(condition) = &keymap.when {
            if !self.check_condition(condition) {
                return false;
            }
        }
        true
    }

    pub fn key_down(&self, key_event: &KeyEvent) {
        let keypress = KeyPress {
            key: key_event.key.clone(),
            mods: key_event.mods,
        };
        for keymap in self.keymaps.lock().unwrap().iter() {
            if self.match_keymap(&keypress, keymap) {
                self.run_command(&keymap.command);
                return;
            }
        }

        let mut mods = keypress.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            match &keypress.key {
                druid::keyboard_types::Key::Character(c) => {
                    self.insert(c);
                }
                _ => (),
            }
        }

        // let key = match &key_event.key {
        //     druid::keyboard_types::Key::Character(c) => &c,
        //     druid::keyboard_types::Key::Enter => "enter",
        //     druid::keyboard_types::Key::Tab => "tab",
        //     druid::keyboard_types::Key::ArrowDown => "arrowdown",
        //     druid::keyboard_types::Key::ArrowLeft => "arrowleft",
        //     druid::keyboard_types::Key::ArrowRight => "arrowright",
        //     druid::keyboard_types::Key::ArrowUp => "arrowup",
        //     druid::keyboard_types::Key::End => "end",
        //     druid::keyboard_types::Key::Home => "home",
        //     druid::keyboard_types::Key::PageDown => "pagedown",
        //     druid::keyboard_types::Key::PageUp => "pageup",
        //     druid::keyboard_types::Key::Backspace => "backspace",
        //     druid::keyboard_types::Key::Delete => "delete",
        //     druid::keyboard_types::Key::Escape => "escape",
        //     druid::keyboard_types::Key::F1 => "f1",
        //     druid::keyboard_types::Key::F2 => "f2",
        //     druid::keyboard_types::Key::F3 => "f3",
        //     druid::keyboard_types::Key::F4 => "f4",
        //     druid::keyboard_types::Key::F5 => "f5",
        //     druid::keyboard_types::Key::F6 => "f6",
        //     druid::keyboard_types::Key::F7 => "f7",
        //     druid::keyboard_types::Key::F8 => "f8",
        //     druid::keyboard_types::Key::F9 => "f9",
        //     druid::keyboard_types::Key::F10 => "f10",
        //     druid::keyboard_types::Key::F11 => "f11",
        //     druid::keyboard_types::Key::F12 => "f12",
        //     _ => return,
        // };

        // *self.keypress_sequence.lock().unwrap() =
        //     uuid::Uuid::new_v4().to_string();

        // // let keypress = self.pending_keypress.lock().unwrap().clone() +

        // let mut keypress = self.pending_keypress.lock().unwrap().clone();
        // keypress.push(format!(
        //     "{}{}{}{}{}",
        //     if key_event.mods.alt() { "alt+" } else { "" },
        //     if key_event.mods.ctrl() { "ctrl+" } else { "" },
        //     if key_event.mods.meta() { "meta+" } else { "" },
        //     if key_event.mods.shift() { "shift+" } else { "" },
        //     key.to_lowercase(),
        // ));

        // for (key, value) in self.keymaps.iter() {
        //     if keypress.len() < key.len()
        //         && keypress.as_slice() == &key[..keypress.len()]
        //     {
        //         *self.pending_keypress.lock().unwrap() = keypress.clone();
        //         let pending_keypress = self.pending_keypress.clone();
        //         let keypress_sequence = self.keypress_sequence.clone();
        //         let keymaps = self.keymaps.clone();
        //         let crane_state = self.clone();
        //         thread::spawn(move || {
        //             let pre_keypress_sequence =
        //                 { keypress_sequence.lock().unwrap().to_string() };
        //             thread::sleep(Duration::from_millis(3000));
        //             let mut pending_keypress = pending_keypress.lock().unwrap();
        //             let keypress_sequence = keypress_sequence.lock().unwrap();

        //             if *keypress_sequence != pre_keypress_sequence {
        //                 return;
        //             }

        //             if let Some(value) = keymaps.get(&keypress) {
        //                 crane_state.run_command(value);
        //             }
        //             *pending_keypress = Vec::new();
        //         });

        //         return;
        //     }
        // }

        // if let Some(cmd) = self.keymaps.get(&keypress) {
        //     *self.pending_keypress.lock().unwrap() = Vec::new();
        //     self.run_command(cmd);
        //     return;
        // }

        // *self.pending_keypress.lock().unwrap() = Vec::new();

        // let mut mods = key_event.mods.clone();
        // mods.set(Modifiers::SHIFT, false);
        // if mods.is_empty() {
        //     match &key_event.key {
        //         druid::keyboard_types::Key::Character(c) => {
        //             self.insert(c);
        //         }
        //         _ => (),
        //     }
        // }
    }

    fn check_condition(&self, condition: &str) -> bool {
        let or_indics: Vec<_> = condition.match_indices("||").collect();
        let and_indics: Vec<_> = condition.match_indices("&&").collect();
        if and_indics.is_empty() {
            if or_indics.is_empty() {
                return self.check_one_condition(condition);
            } else {
                return self.check_one_condition(&condition[..or_indics[0].0])
                    || self.check_condition(&condition[or_indics[0].0 + 2..]);
            }
        } else {
            if or_indics.is_empty() {
                return self.check_one_condition(&condition[..and_indics[0].0])
                    && self.check_condition(&condition[and_indics[0].0 + 2..]);
            } else {
                if or_indics[0].0 < and_indics[0].0 {
                    return self
                        .check_one_condition(&condition[..or_indics[0].0])
                        || self
                            .check_condition(&condition[or_indics[0].0 + 2..]);
                } else {
                    return self
                        .check_one_condition(&condition[..and_indics[0].0])
                        && self.check_condition(
                            &condition[and_indics[0].0 + 2..],
                        );
                }
            }
        }
    }

    fn check_one_condition(&self, condition: &str) -> bool {
        match condition.trim() {
            "palette_focus" => {
                *self.focus.lock().unwrap() == CraneWidget::Palette
            }
            "list_focus" => *self.focus.lock().unwrap() == CraneWidget::Palette,
            _ => false,
        }
    }

    pub fn set_ui_sink(&self, ui_sink: ExtEventSink) {
        *self.ui_sink.lock().unwrap() = Some(ui_sink);
    }

    pub fn open_file(&self, path: &str) {
        self.editor_split.lock().unwrap().open_file(path);
    }

    pub fn submit_ui_command(&self, cmd: CraneUICommand, widget_id: WidgetId) {
        self.ui_sink
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .submit_command(CRANE_UI_COMMAND, cmd, Target::Widget(widget_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_condition() {
        let state = CraneState::new();
        assert_eq!(state.check_condition("palette_focus"), false);
        assert_eq!(
            state.check_condition(" palette_focus ||   editor_focus"),
            true
        );

        *state.focus.lock().unwrap() = CraneWidget::Palette;
        assert_eq!(state.check_condition("palette_focus"), true);
        assert_eq!(
            state.check_condition(" palette_focus ||   editor_focus"),
            true
        );
    }
}
