use std::str::FromStr;
use std::{collections::HashMap, io::Read};
use std::{fs::File, sync::Arc};

use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use druid::KbKey;
use druid::{
    Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Modifiers, Target, WidgetId,
    WindowId,
};
use toml;

use crate::{
    command::LapceCommand,
    state::{LapceFocus, Mode},
};

const default_keymaps_windows: &'static str =
    include_str!("../../defaults/keymaps-windows.toml");
const default_keymaps_macos: &'static str =
    include_str!("../../defaults/keymaps-macos.toml");
const default_keymaps_linux: &'static str =
    include_str!("../../defaults/keymaps-linux.toml");

#[derive(PartialEq)]
enum KeymapMatch {
    Full(String),
    Multiple(Vec<String>),
    Prefix,
    None,
}

#[derive(Clone, Debug)]
pub struct KeyPress {
    pub code: druid::keyboard_types::Code,
    pub key: druid::keyboard_types::Key,
    pub mods: Modifiers,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMap {
    pub key: Vec<(Modifiers, druid::keyboard_types::Code)>,
    pub modes: Vec<Mode>,
    pub when: Option<String>,
    pub command: String,
}

pub trait KeyPressFocus {
    fn get_mode(&self) -> Mode;
    fn check_condition(&self, condition: &str) -> bool;
    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    );
    fn insert(&mut self, ctx: &mut EventCtx, c: &str);
}

#[derive(Clone, Debug)]
pub struct KeyPressData {
    pending_keypress: Vec<KeyPress>,
    keymaps: im::HashMap<Vec<(Modifiers, druid::keyboard_types::Code)>, Vec<KeyMap>>,
    count: Option<usize>,
}

impl KeyPressData {
    pub fn new() -> Self {
        Self {
            pending_keypress: Vec::new(),
            keymaps: Self::get_keymaps().unwrap_or(im::HashMap::new()),
            count: None,
        }
    }

    pub fn update_keymaps(&mut self) {
        if let Ok(new_keymaps) = Self::get_keymaps() {
            self.keymaps = new_keymaps;
        }
    }

    fn run_command<T: KeyPressFocus>(
        &self,
        ctx: &mut EventCtx,
        command: &str,
        count: Option<usize>,
        focus: &mut T,
        env: &Env,
    ) -> Result<()> {
        let cmd = LapceCommand::from_str(command)?;
        focus.run_command(ctx, &cmd, count, env);
        Ok(())
    }

    fn handle_count(&mut self, mode: &Mode, keypress: &KeyPress) -> bool {
        if mode == &Mode::Insert {
            return false;
        }

        match &keypress.key {
            druid::keyboard_types::Key::Character(c) => {
                if let Ok(n) = c.parse::<usize>() {
                    if self.count.is_some() || n > 0 {
                        self.count = Some(self.count.unwrap_or(0) * 10 + n);
                        return true;
                    }
                }
            }
            _ => (),
        }

        false
    }

    pub fn key_down<T: KeyPressFocus>(
        &mut self,
        ctx: &mut EventCtx,
        key_event: &KeyEvent,
        focus: &mut T,
        env: &Env,
    ) -> bool {
        let code = match key_event.code {
            druid::keyboard_types::Code::AltRight => {
                druid::keyboard_types::Code::AltLeft
            }
            druid::keyboard_types::Code::ShiftRight => {
                druid::keyboard_types::Code::ShiftLeft
            }
            druid::keyboard_types::Code::MetaRight => {
                druid::keyboard_types::Code::MetaLeft
            }
            druid::keyboard_types::Code::ControlRight => {
                druid::keyboard_types::Code::ControlLeft
            }
            _ => key_event.code,
        };
        let keypress = KeyPress {
            code,
            key: key_event.key.clone(),
            mods: key_event.mods.clone(),
        };

        println!("{:?}", keypress);

        let mode = focus.get_mode();
        if self.handle_count(&mode, &keypress) {
            return false;
        }

        let mut keypresses: Vec<(Modifiers, druid::keyboard_types::Code)> = self
            .pending_keypress
            .iter()
            .map(|k| (k.mods.clone(), k.code.clone()))
            .collect();
        keypresses.push((keypress.mods.clone(), keypress.code.clone()));

        let matches = self.match_keymap(&keypresses, focus);
        let keymatch = if matches.len() == 0 {
            KeymapMatch::None
        } else if matches.len() == 1 && matches[0].key == keypresses {
            KeymapMatch::Full(matches[0].command.clone())
        } else if matches.len() > 1
            && matches.iter().filter(|m| m.key != keypresses).count() == 0
        {
            KeymapMatch::Multiple(
                matches.iter().map(|m| m.command.clone()).collect(),
            )
        } else {
            KeymapMatch::Prefix
        };
        match keymatch {
            KeymapMatch::Full(command) => {
                let count = self.count.take();
                self.run_command(ctx, &command, count, focus, env);
                self.pending_keypress = Vec::new();
                return true;
            }
            KeymapMatch::Multiple(commands) => {
                let count = self.count.take();
                for command in commands {
                    self.run_command(ctx, &command, count, focus, env);
                }
                self.pending_keypress = Vec::new();
                return true;
            }
            KeymapMatch::Prefix => {
                self.pending_keypress.push(keypress);
                return false;
            }
            KeymapMatch::None => {
                self.pending_keypress = Vec::new();
            }
        }

        if mode != Mode::Insert {
            self.handle_count(&mode, &keypress);
            return false;
        }

        self.count = None;

        let mut mods = keypress.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            match &key_event.key {
                druid::keyboard_types::Key::Character(c) => {
                    focus.insert(ctx, c);
                    return true;
                }
                _ => (),
            }
        }
        false
    }

    fn match_keymap<T: KeyPressFocus>(
        &self,
        keypresses: &Vec<(Modifiers, druid::keyboard_types::Code)>,
        check: &T,
    ) -> Vec<&KeyMap> {
        self.keymaps
            .get(keypresses)
            .map(|keymaps| {
                keymaps
                    .iter()
                    .filter(|keymap| {
                        if keymap.modes.len() > 0
                            && !keymap.modes.contains(&check.get_mode())
                        {
                            return false;
                        }
                        if let Some(condition) = &keymap.when {
                            if !self.check_condition(condition, check) {
                                return false;
                            }
                        }
                        true
                    })
                    .collect()
            })
            .unwrap_or(Vec::new())
    }

    fn check_one_condition<T: KeyPressFocus>(
        &self,
        condition: &str,
        check: &T,
    ) -> bool {
        let condition = condition.trim();
        let (reverse, condition) = if condition.starts_with("!") {
            (true, &condition[1..])
        } else {
            (false, condition)
        };
        let matched = check.check_condition(condition);
        if reverse {
            !matched
        } else {
            matched
        }
    }

    fn check_condition<T: KeyPressFocus>(&self, condition: &str, check: &T) -> bool {
        let or_indics: Vec<_> = condition.match_indices("||").collect();
        let and_indics: Vec<_> = condition.match_indices("&&").collect();
        if and_indics.is_empty() {
            if or_indics.is_empty() {
                return self.check_one_condition(&condition, check);
            } else {
                return self
                    .check_one_condition(&condition[..or_indics[0].0], check)
                    || self
                        .check_condition(&condition[or_indics[0].0 + 2..], check);
            }
        } else {
            if or_indics.is_empty() {
                return self
                    .check_one_condition(&condition[..and_indics[0].0], check)
                    && self
                        .check_condition(&condition[and_indics[0].0 + 2..], check);
            } else {
                if or_indics[0].0 < and_indics[0].0 {
                    return self
                        .check_one_condition(&condition[..or_indics[0].0], check)
                        || self.check_condition(
                            &condition[or_indics[0].0 + 2..],
                            check,
                        );
                } else {
                    return self
                        .check_one_condition(&condition[..and_indics[0].0], check)
                        && self.check_condition(
                            &condition[and_indics[0].0 + 2..],
                            check,
                        );
                }
            }
        }
    }

    fn keymaps_from_str(
        s: &str,
    ) -> Result<
        im::HashMap<Vec<(Modifiers, druid::keyboard_types::Code)>, Vec<KeyMap>>,
    > {
        let toml_keymaps: toml::Value = toml::from_str(s)?;
        let toml_keymaps = toml_keymaps
            .get("keymaps")
            .and_then(|v| v.as_array())
            .ok_or(anyhow!("no keymaps"))?;

        let mut keymaps: im::HashMap<
            Vec<(Modifiers, druid::keyboard_types::Code)>,
            Vec<KeyMap>,
        > = im::HashMap::new();
        for toml_keymap in toml_keymaps {
            if let Ok(keymap) = Self::get_keymap(toml_keymap) {
                for i in 1..keymap.key.len() + 1 {
                    let key = keymap.key[..i].to_vec();
                    match keymaps.get_mut(&key) {
                        Some(keymaps) => keymaps.push(keymap.clone()),
                        None => {
                            keymaps.insert(key, vec![keymap.clone()]);
                        }
                    }
                }
            }
        }

        Ok(keymaps)
    }

    fn get_keymaps() -> Result<
        im::HashMap<Vec<(Modifiers, druid::keyboard_types::Code)>, Vec<KeyMap>>,
    > {
        let mut keymaps_str = if std::env::consts::OS == "macos" {
            default_keymaps_macos
        } else if std::env::consts::OS == "linux" {
            default_keymaps_linux
        } else {
            default_keymaps_windows
        }
        .to_string();

        if let Some(proj_dirs) = ProjectDirs::from("", "", "Lapce") {
            let path = proj_dirs.config_dir().join("keymaps.toml");
            if let Ok(content) = std::fs::read_to_string(path) {
                if content != "" {
                    let result: Result<toml::Value, toml::de::Error> =
                        toml::from_str(&content);
                    if result.is_ok() {
                        keymaps_str += &content;
                    }
                }
            }
        }

        Self::keymaps_from_str(&keymaps_str)
    }

    fn get_keypress(key: &str) -> Vec<(Modifiers, druid::keyboard_types::Code)> {
        let mut keypresses = Vec::new();
        for k in key.split(" ") {
            let mut mods = Modifiers::default();

            let parts = k.split("+").collect::<Vec<&str>>();
            if parts.len() == 0 {
                continue;
            }
            let code = match parts[parts.len() - 1].to_lowercase().as_ref() {
                "escape" => druid::keyboard_types::Code::Escape,
                "esc" => druid::keyboard_types::Code::Escape,
                "backspace" => druid::keyboard_types::Code::Backspace,
                "bs" => druid::keyboard_types::Code::Backspace,
                "arrowup" => druid::keyboard_types::Code::ArrowUp,
                "arrowdown" => druid::keyboard_types::Code::ArrowDown,
                "arrowright" => druid::keyboard_types::Code::ArrowRight,
                "arrowleft" => druid::keyboard_types::Code::ArrowLeft,
                "up" => druid::keyboard_types::Code::ArrowUp,
                "down" => druid::keyboard_types::Code::ArrowDown,
                "right" => druid::keyboard_types::Code::ArrowRight,
                "left" => druid::keyboard_types::Code::ArrowLeft,
                "tab" => druid::keyboard_types::Code::Tab,
                "enter" => druid::keyboard_types::Code::Enter,
                "delete" => druid::keyboard_types::Code::Delete,
                "del" => druid::keyboard_types::Code::Delete,
                "ctrl" => druid::keyboard_types::Code::ControlLeft,
                "meta" => druid::keyboard_types::Code::MetaLeft,
                "shift" => druid::keyboard_types::Code::ShiftLeft,
                "alt" => druid::keyboard_types::Code::AltLeft,
                "f1" => druid::keyboard_types::Code::F1,
                "f2" => druid::keyboard_types::Code::F2,
                "f3" => druid::keyboard_types::Code::F3,
                "f4" => druid::keyboard_types::Code::F4,
                "f5" => druid::keyboard_types::Code::F5,
                "f6" => druid::keyboard_types::Code::F6,
                "f7" => druid::keyboard_types::Code::F7,
                "f8" => druid::keyboard_types::Code::F8,
                "f9" => druid::keyboard_types::Code::F9,
                "f10" => druid::keyboard_types::Code::F10,
                "f11" => druid::keyboard_types::Code::F11,
                "f12" => druid::keyboard_types::Code::F12,
                "a" => druid::keyboard_types::Code::KeyA,
                "b" => druid::keyboard_types::Code::KeyB,
                "c" => druid::keyboard_types::Code::KeyC,
                "d" => druid::keyboard_types::Code::KeyD,
                "e" => druid::keyboard_types::Code::KeyE,
                "f" => druid::keyboard_types::Code::KeyF,
                "g" => druid::keyboard_types::Code::KeyG,
                "h" => druid::keyboard_types::Code::KeyH,
                "i" => druid::keyboard_types::Code::KeyI,
                "j" => druid::keyboard_types::Code::KeyJ,
                "k" => druid::keyboard_types::Code::KeyK,
                "l" => druid::keyboard_types::Code::KeyL,
                "m" => druid::keyboard_types::Code::KeyM,
                "n" => druid::keyboard_types::Code::KeyN,
                "o" => druid::keyboard_types::Code::KeyO,
                "p" => druid::keyboard_types::Code::KeyP,
                "q" => druid::keyboard_types::Code::KeyQ,
                "r" => druid::keyboard_types::Code::KeyR,
                "s" => druid::keyboard_types::Code::KeyS,
                "t" => druid::keyboard_types::Code::KeyT,
                "u" => druid::keyboard_types::Code::KeyU,
                "v" => druid::keyboard_types::Code::KeyV,
                "w" => druid::keyboard_types::Code::KeyW,
                "x" => druid::keyboard_types::Code::KeyX,
                "y" => druid::keyboard_types::Code::KeyY,
                "z" => druid::keyboard_types::Code::KeyZ,
                "1" => druid::keyboard_types::Code::Digit1,
                "2" => druid::keyboard_types::Code::Digit2,
                "3" => druid::keyboard_types::Code::Digit3,
                "4" => druid::keyboard_types::Code::Digit4,
                "5" => druid::keyboard_types::Code::Digit5,
                "6" => druid::keyboard_types::Code::Digit6,
                "7" => druid::keyboard_types::Code::Digit7,
                "8" => druid::keyboard_types::Code::Digit8,
                "9" => druid::keyboard_types::Code::Digit9,
                "0" => druid::keyboard_types::Code::Digit0,
                "=" => druid::keyboard_types::Code::Equal,
                "-" => druid::keyboard_types::Code::Minus,
                "]" => druid::keyboard_types::Code::BracketRight,
                "[" => druid::keyboard_types::Code::BracketLeft,
                "'" => druid::keyboard_types::Code::Quote,
                ";" => druid::keyboard_types::Code::Semicolon,
                "\\" => druid::keyboard_types::Code::Backslash,
                "," => druid::keyboard_types::Code::Comma,
                "/" => druid::keyboard_types::Code::Slash,
                "." => druid::keyboard_types::Code::Period,
                "`" => druid::keyboard_types::Code::Backquote,

                _ => druid::keyboard_types::Code::Unidentified,
            };
            for part in &parts[..parts.len() - 1] {
                match part.to_lowercase().as_ref() {
                    "ctrl" => mods.set(Modifiers::CONTROL, true),
                    "meta" => mods.set(Modifiers::META, true),
                    "shift" => mods.set(Modifiers::SHIFT, true),
                    "alt" => mods.set(Modifiers::ALT, true),
                    _ => (),
                }
            }

            keypresses.push((mods, code));
            // for (i, part) in
            //     k.split("+").collect::<Vec<&str>>().iter().rev().enumerate()
            // {
            //     if i == 0 {
            //         keypress.key = match part.to_lowercase().as_ref() {
            //             "escape" => druid::keyboard_types::Key::Escape,
            //             "esc" => druid::keyboard_types::Key::Escape,
            //             "backspace" => druid::keyboard_types::Key::Backspace,
            //             "bs" => druid::keyboard_types::Key::Backspace,
            //             "arrowup" => druid::keyboard_types::Key::ArrowUp,
            //             "arrowdown" => druid::keyboard_types::Key::ArrowDown,
            //             "arrowright" => druid::keyboard_types::Key::ArrowRight,
            //             "arrowleft" => druid::keyboard_types::Key::ArrowLeft,
            //             "up" => druid::keyboard_types::Key::ArrowUp,
            //             "down" => druid::keyboard_types::Key::ArrowDown,
            //             "right" => druid::keyboard_types::Key::ArrowRight,
            //             "left" => druid::keyboard_types::Key::ArrowLeft,
            //             "tab" => druid::keyboard_types::Key::Tab,
            //             "enter" => druid::keyboard_types::Key::Enter,
            //             "delete" => druid::keyboard_types::Key::Delete,
            //             "del" => druid::keyboard_types::Key::Delete,
            //             _ => druid::keyboard_types::Key::Character(part.to_string()),
            //         }
            //     } else {
            //         match part.to_lowercase().as_ref() {
            //             "ctrl" => keypress.mods.set(Modifiers::CONTROL, true),
            //             "meta" => keypress.mods.set(Modifiers::META, true),
            //             "shift" => keypress.mods.set(Modifiers::SHIFT, true),
            //             "alt" => keypress.mods.set(Modifiers::ALT, true),
            //             _ => (),
            //         }
            //     }
            // }
            // keypresses.push(keypress);
        }
        keypresses
    }

    fn get_keymap(toml_keymap: &toml::Value) -> Result<KeyMap> {
        let key = toml_keymap
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or(anyhow!("no key in keymap"))?;

        Ok(KeyMap {
            key: Self::get_keypress(key),
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

    fn get_modes(toml_keymap: &toml::Value) -> Vec<Mode> {
        toml_keymap
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|m| {
                m.chars()
                    .filter_map(|c| match c.to_lowercase().to_string().as_ref() {
                        "i" => Some(Mode::Insert),
                        "n" => Some(Mode::Normal),
                        "v" => Some(Mode::Visual),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keymap() {
        let keymaps = r###"
keymaps = [
    { key = "ctrl+w l l", command = "right", when = "n" },
    { key = "ctrl+w l", command = "right", when = "n" },
    { key = "ctrl+w h", command = "left", when = "n" },
    { key = "ctrl+w",   command = "left", when = "n" },
]
        "###;
        let keymaps = KeyPressData::keymaps_from_str(keymaps).unwrap();
        let keypress = KeyPressData::get_keypress("ctrl+w");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 4);

        let keypress = KeyPressData::get_keypress("ctrl+w l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 2);

        let keypress = KeyPressData::get_keypress("ctrl+w h");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPressData::get_keypress("ctrl+w l l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);
    }
}
