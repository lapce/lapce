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
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    command::{LapceCommand, LAPCE_COMMAND},
    editor::EditorSplitState,
    language::TreeSitter,
    palette::PaletteState,
};

lazy_static! {
    pub static ref LAPCE_STATE: LapceState = LapceState::new();
}

enum KeymapMatch {
    Full,
    Prefix,
}

#[derive(Clone, PartialEq)]
pub enum LapceWidget {
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

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Vec<Mode>,
    pub when: Option<String>,
    pub command: String,
}

#[derive(Clone)]
pub struct LapceState {
    pub palette: Arc<Mutex<PaletteState>>,
    keypress_sequence: Arc<Mutex<String>>,
    pending_keypress: Arc<Mutex<Vec<KeyPress>>>,
    count: Arc<Mutex<Option<usize>>>,
    keymaps: Arc<Mutex<Vec<KeyMap>>>,
    pub last_focus: Arc<Mutex<LapceWidget>>,
    pub focus: Arc<Mutex<LapceWidget>>,
    pub ui_sink: Arc<Mutex<Option<ExtEventSink>>>,
    pub editor_split: Arc<Mutex<EditorSplitState>>,
}

impl LapceState {
    pub fn new() -> LapceState {
        LapceState {
            pending_keypress: Arc::new(Mutex::new(Vec::new())),
            keymaps: Arc::new(Mutex::new(
                Self::get_keymaps().unwrap_or(Vec::new()),
            )),
            keypress_sequence: Arc::new(Mutex::new("".to_string())),
            count: Arc::new(Mutex::new(None)),
            ui_sink: Arc::new(Mutex::new(None)),
            focus: Arc::new(Mutex::new(LapceWidget::Editor)),
            last_focus: Arc::new(Mutex::new(LapceWidget::Editor)),
            palette: Arc::new(Mutex::new(PaletteState::new())),
            editor_split: Arc::new(Mutex::new(EditorSplitState::new())),
        }
    }

    fn get_keymaps() -> Result<Vec<KeyMap>> {
        let mut keymaps = Vec::new();
        let mut f = File::open("/Users/Lulu/lapce/.lapce/keymaps.toml")?;
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
                        "arrowup" => druid::keyboard_types::Key::ArrowUp,
                        "arrowdown" => druid::keyboard_types::Key::ArrowDown,
                        "arrowright" => druid::keyboard_types::Key::ArrowRight,
                        "arrowleft" => druid::keyboard_types::Key::ArrowLeft,
                        "tab" => druid::keyboard_types::Key::Tab,
                        "enter" => druid::keyboard_types::Key::Enter,
                        "del" => druid::keyboard_types::Key::Delete,
                        _ => druid::keyboard_types::Key::Character(
                            part.to_string(),
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
            LapceWidget::Palette => Mode::Insert,
            LapceWidget::Editor => self.editor_split.lock().unwrap().get_mode(),
        }
    }

    pub fn insert(&self, content: &str) {
        let foucus = self.focus.lock().unwrap().clone();
        match foucus {
            LapceWidget::Palette => {
                self.palette.lock().unwrap().insert(content);
            }
            LapceWidget::Editor => {
                self.editor_split.lock().unwrap().insert(content);
            }
        }
    }

    pub fn handle_count(&self, keypress: &KeyPress) -> bool {
        if self.get_mode() == Mode::Insert {
            return false;
        }

        match &keypress.key {
            druid::keyboard_types::Key::Character(c) => {
                if let Ok(n) = c.parse::<usize>() {
                    let mut count = self.count.lock().unwrap();
                    if count.is_some() || n > 0 {
                        *count = Some(count.unwrap_or(0) * 10 + n);
                        return true;
                    }
                }
            }
            _ => (),
        }

        false
    }

    pub fn get_count(&self) -> Option<usize> {
        let mut count = self.count.lock().unwrap();
        let new_count = count.clone();
        *count = None;
        new_count
    }

    pub fn run_command(&self, command: &str) {
        let count = self.get_count();
        if let Ok(cmd) = LapceCommand::from_str(command) {
            let foucus = self.focus.lock().unwrap().clone();
            match cmd {
                LapceCommand::Palette => {
                    self.palette.lock().unwrap().run();
                }
                LapceCommand::PaletteCancel => {
                    self.palette.lock().unwrap().cancel();
                }
                _ => {
                    match foucus {
                        LapceWidget::Editor => self
                            .editor_split
                            .lock()
                            .unwrap()
                            .run_command(count, cmd),
                        LapceWidget::Palette => match cmd {
                            LapceCommand::ListSelect => {
                                self.palette.lock().unwrap().select();
                            }
                            LapceCommand::ListNext => {
                                self.palette.lock().unwrap().change_index(1);
                            }
                            LapceCommand::ListPrevious => {
                                self.palette.lock().unwrap().change_index(-1);
                            }
                            LapceCommand::Left => {
                                self.palette.lock().unwrap().move_cursor(-1);
                            }
                            LapceCommand::Right => {
                                self.palette.lock().unwrap().move_cursor(1);
                            }
                            LapceCommand::DeleteBackward => {
                                self.palette.lock().unwrap().delete_backward();
                            }
                            LapceCommand::DeleteToBeginningOfLine => {
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

    fn match_keymap_new(
        &self,
        keypresses: &Vec<KeyPress>,
        keymap: &KeyMap,
    ) -> Option<KeymapMatch> {
        // let mut keypresses = self.pending_keypress.lock().unwrap().clone();
        // if let Some(keypress) = keypress {
        //     keypresses.push(keypress.clone());
        // }

        let match_result = if keymap.key.len() > keypresses.len() {
            if keymap.key[..keypresses.len()] == keypresses[..] {
                Some(KeymapMatch::Prefix)
            } else {
                None
            }
        } else if &keymap.key == keypresses {
            Some(KeymapMatch::Full)
        } else {
            None
        };

        let mode = self.get_mode();
        if !keymap.modes.is_empty() && !keymap.modes.contains(&mode) {
            return None;
        }

        if let Some(condition) = &keymap.when {
            if !self.check_condition(condition) {
                return None;
            }
        }
        match_result
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
        println!("key_event {:?}", key_event);
        let mut keypress_sequence = self.keypress_sequence.lock().unwrap();
        *keypress_sequence = uuid::Uuid::new_v4().to_string();
        let mut mods = key_event.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        let keypress = KeyPress {
            key: key_event.key.clone(),
            mods,
        };

        if self.handle_count(&keypress) {
            return;
        }

        let mut full_match_keymap = None;
        let mut keypresses = self.pending_keypress.lock().unwrap().clone();
        keypresses.push(keypress.clone());
        for keymap in self.keymaps.lock().unwrap().iter() {
            if let Some(match_result) =
                self.match_keymap_new(&keypresses, keymap)
            {
                match match_result {
                    KeymapMatch::Full => {
                        if full_match_keymap.is_none() {
                            full_match_keymap = Some(keymap.clone());
                        }
                    }
                    KeymapMatch::Prefix => {
                        self.pending_keypress
                            .lock()
                            .unwrap()
                            .push(keypress.clone());
                        let keypress_sequence = self.keypress_sequence.clone();
                        let keymaps = self.keymaps.clone();
                        let state = self.clone();
                        thread::spawn(move || {
                            let pre_keypress_sequence =
                                keypress_sequence.lock().unwrap().clone();
                            thread::sleep(Duration::from_millis(3000));
                            let keypress_sequence =
                                keypress_sequence.lock().unwrap();
                            if *keypress_sequence != pre_keypress_sequence {
                                return;
                            }
                            let keypresses =
                                state.pending_keypress.lock().unwrap().clone();
                            *state.pending_keypress.lock().unwrap() =
                                Vec::new();
                            for keymap in keymaps.lock().unwrap().iter() {
                                if let Some(match_result) =
                                    state.match_keymap_new(&keypresses, keymap)
                                {
                                    match match_result {
                                        KeymapMatch::Full => {
                                            state.run_command(&keymap.command);
                                            return;
                                        }
                                        _ => (),
                                    }
                                }
                            }
                        });
                        return;
                    }
                }
            }
        }

        *self.pending_keypress.lock().unwrap() = Vec::new();

        if let Some(keymap) = full_match_keymap {
            self.run_command(&keymap.command);
            return;
        }

        if self.get_mode() != Mode::Insert {
            self.handle_count(&keypress);
            return;
        }

        *self.count.lock().unwrap() = None;

        if mods.is_empty() {
            match &key_event.key {
                druid::keyboard_types::Key::Character(c) => {
                    self.insert(c);
                }
                _ => (),
            }
        }
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
                *self.focus.lock().unwrap() == LapceWidget::Palette
            }
            "list_focus" => *self.focus.lock().unwrap() == LapceWidget::Palette,
            _ => false,
        }
    }

    pub fn set_ui_sink(&self, ui_sink: ExtEventSink) {
        *self.ui_sink.lock().unwrap() = Some(ui_sink);
    }

    pub fn open_file(&self, path: &str) {
        self.editor_split.lock().unwrap().open_file(path);
    }

    pub fn submit_ui_command(&self, cmd: LapceUICommand, widget_id: WidgetId) {
        self.ui_sink
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .submit_command(LAPCE_UI_COMMAND, cmd, Target::Widget(widget_id));
    }
}

#[cfg(test)]
mod tests {
    use xi_rope::Rope;

    use super::*;

    #[test]
    fn test_check_condition() {
        let rope = Rope::from_str("abc\nabc\n").unwrap();
        // assert_eq!(rope.len(), 9);
        assert_eq!(rope.offset_of_line(1), 1);
        // assert_eq!(rope.line_of_offset(rope.len()), 9);
    }
}
