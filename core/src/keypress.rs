use std::str::FromStr;
use std::{collections::HashMap, io::Read};
use std::{fs::File, sync::Arc};

use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use druid::{
    Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Modifiers, Target, WidgetId,
    WindowId,
};
use druid::{Command, KbKey};
use indexmap::IndexMap;
use toml;

use crate::command::{
    lapce_internal_commands, CommandExecuted, CommandTarget, LapceCommandNew,
    LAPCE_NEW_COMMAND,
};
use crate::data::LapceTabData;
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub key: druid::KbKey,
    pub mods: Modifiers,
}

impl KeyPress {
    pub fn is_char(&self) -> bool {
        let mut mods = self.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            match &self.key {
                druid::KbKey::Character(c) => {
                    return true;
                }
                _ => (),
            }
        }
        false
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
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
    ) -> CommandExecuted;
    fn expect_char(&self) -> bool {
        false
    }
    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str);
}

#[derive(Clone, Debug)]
pub struct KeyPressData {
    pending_keypress: Vec<KeyPress>,
    pub keymaps: Arc<IndexMap<Vec<KeyPress>, Vec<KeyMap>>>,
    pub commands: Arc<IndexMap<String, LapceCommandNew>>,
    count: Option<usize>,
}

impl KeyPressData {
    pub fn new() -> Self {
        Self {
            pending_keypress: Vec::new(),
            keymaps: Arc::new(Self::get_keymaps().unwrap_or(IndexMap::new())),
            commands: Arc::new(lapce_internal_commands()),
            count: None,
        }
    }

    pub fn update_keymaps(&mut self) {
        if let Ok(new_keymaps) = Self::get_keymaps() {
            self.keymaps = Arc::new(new_keymaps);
        }
    }

    fn run_command<T: KeyPressFocus>(
        &self,
        ctx: &mut EventCtx,
        command: &str,
        count: Option<usize>,
        focus: &mut T,
        env: &Env,
    ) -> CommandExecuted {
        if let Some(cmd) = self.commands.get(command) {
            if let CommandTarget::Focus = cmd.target {
                if let Ok(cmd) = LapceCommand::from_str(command) {
                    focus.run_command(ctx, &cmd, count, env)
                } else {
                    CommandExecuted::No
                }
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_NEW_COMMAND,
                    cmd.clone(),
                    Target::Auto,
                ));
                CommandExecuted::Yes
            }
        } else {
            CommandExecuted::No
        }
    }

    fn handle_count<T: KeyPressFocus>(
        &mut self,
        focus: &T,
        keypress: &KeyPress,
    ) -> bool {
        if focus.expect_char() {
            return false;
        }
        let mode = focus.get_mode();
        if mode == Mode::Insert || mode == Mode::Terminal {
            return false;
        }

        if !keypress.mods.is_empty() {
            return false;
        }

        match &keypress.key {
            druid::KbKey::Character(c) => {
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
        if key_event.key == druid::KbKey::Shift {
            let mut mods = key_event.mods.clone();
            mods.set(Modifiers::SHIFT, false);
            if mods.is_empty() {
                return false;
            }
        }
        let mut mods = key_event.mods.clone();
        match &key_event.key {
            druid::KbKey::Character(c) => {
                mods.set(Modifiers::SHIFT, false);
            }
            _ => (),
        }

        let keypress = KeyPress {
            key: key_event.key.clone(),
            mods,
        };

        let mode = focus.get_mode();
        if self.handle_count(focus, &keypress) {
            return false;
        }

        let mut keypresses: Vec<KeyPress> = self.pending_keypress.clone();
        keypresses.push(keypress.clone());

        let matches = self.match_keymap(&keypresses, focus);
        let keymatch = if matches.len() == 0 {
            KeymapMatch::None
        } else if matches.len() == 1 && matches[0].key == keypresses {
            KeymapMatch::Full(matches[0].command.clone())
        } else if matches.len() > 1
            && matches.iter().filter(|m| m.key != keypresses).count() == 0
        {
            KeymapMatch::Multiple(
                matches.iter().rev().map(|m| m.command.clone()).collect(),
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
                self.pending_keypress = Vec::new();
                let count = self.count.take();
                for command in commands {
                    if self.run_command(ctx, &command, count, focus, env)
                        == CommandExecuted::Yes
                    {
                        return true;
                    }
                }

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

        if mode != Mode::Insert && mode != Mode::Terminal && !focus.expect_char() {
            if self.handle_count(focus, &keypress) {
                return false;
            }
        }

        self.count = None;

        let mut mods = keypress.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            match &key_event.key {
                druid::KbKey::Character(c) => {
                    focus.receive_char(ctx, c);
                    return true;
                }
                _ => (),
            }
        }
        false
    }

    fn match_keymap<T: KeyPressFocus>(
        &self,
        keypresses: &Vec<KeyPress>,
        check: &T,
    ) -> Vec<&KeyMap> {
        self.keymaps
            .get(keypresses)
            .map(|keymaps| {
                keymaps
                    .iter()
                    .filter(|keymap| {
                        if check.expect_char()
                            && keypresses.len() == 1
                            && keypresses[0].is_char()
                        {
                            return false;
                        }
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

    fn keymaps_from_str(s: &str) -> Result<IndexMap<Vec<KeyPress>, Vec<KeyMap>>> {
        let toml_keymaps: toml::Value = toml::from_str(s)?;
        let toml_keymaps = toml_keymaps
            .get("keymaps")
            .and_then(|v| v.as_array())
            .ok_or(anyhow!("no keymaps"))?;

        let mut keymaps: IndexMap<Vec<KeyPress>, Vec<KeyMap>> = IndexMap::new();
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

    fn get_keymaps() -> Result<IndexMap<Vec<KeyPress>, Vec<KeyMap>>> {
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

    fn get_keypress<'a>(key: &'a str) -> Vec<KeyPress> {
        let mut keypresses = Vec::new();
        for k in key.split(" ") {
            let mut mods = Modifiers::default();

            let parts = k.split("+").collect::<Vec<&str>>();
            if parts.len() == 0 {
                continue;
            }
            let key = match parts[parts.len() - 1].to_lowercase().as_str() {
                "escape" => druid::KbKey::Escape,
                "esc" => druid::KbKey::Escape,
                "backspace" => druid::KbKey::Backspace,
                "bs" => druid::KbKey::Backspace,
                "arrowup" => druid::KbKey::ArrowUp,
                "arrowdown" => druid::KbKey::ArrowDown,
                "arrowright" => druid::KbKey::ArrowRight,
                "arrowleft" => druid::KbKey::ArrowLeft,
                "up" => druid::KbKey::ArrowUp,
                "down" => druid::KbKey::ArrowDown,
                "right" => druid::KbKey::ArrowRight,
                "left" => druid::KbKey::ArrowLeft,
                "tab" => druid::KbKey::Tab,
                "enter" => druid::KbKey::Enter,
                "delete" => druid::KbKey::Delete,
                "del" => druid::KbKey::Delete,
                _ => druid::KbKey::Character(parts[parts.len() - 1].to_string()),
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

            let keypress = KeyPress { mods, key };
            keypresses.push(keypress);
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
                        "t" => Some(Mode::Terminal),
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

pub struct DefaultKeyPressHandler {}

impl KeyPressFocus for DefaultKeyPressHandler {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, condition: &str) -> bool {
        false
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) -> CommandExecuted {
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {}
}
