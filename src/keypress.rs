use std::io::Read;
use std::str::FromStr;
use std::{fs::File, sync::Arc};

use anyhow::{anyhow, Result};
use druid::{
    Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Modifiers, Target,
    WidgetId,
};
use toml;

use crate::{
    command::LapceCommand,
    state::{LapceFocus, LapceState, LapceUIState, Mode, LAPCE_STATE},
};

#[derive(PartialEq)]
enum KeymapMatch {
    Full,
    Prefix,
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

pub struct KeyPressState {
    pending_keypress: Vec<KeyPress>,
    count: Option<usize>,
    keymaps: Vec<KeyMap>,
}

impl KeyPressState {
    pub fn new() -> KeyPressState {
        KeyPressState {
            pending_keypress: Vec::new(),
            count: None,
            keymaps: Self::get_keymaps().unwrap_or(Vec::new()),
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

    pub fn key_down(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        key_event: &KeyEvent,
        env: &Env,
    ) {
        let mut mods = key_event.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        let keypress = KeyPress {
            key: key_event.key.clone(),
            mods,
        };

        let mode = LAPCE_STATE.get_mode();
        if self.handle_count(&mode, &keypress) {
            return;
        }

        let mut full_match_keymap = None;
        let mut keypresses = self.pending_keypress.clone();
        keypresses.push(keypress.clone());
        for keymap in self.keymaps.iter() {
            if let Some(match_result) =
                self.match_keymap_new(&mode, &keypresses, keymap)
            {
                match match_result {
                    KeymapMatch::Full => {
                        if full_match_keymap.is_none() {
                            full_match_keymap = Some(keymap.clone());
                        }
                    }
                    KeymapMatch::Prefix => {
                        self.pending_keypress.push(keypress.clone());
                        return;
                    }
                }
            }
        }

        let pending_keypresses = self.pending_keypress.clone();
        self.pending_keypress = Vec::new();

        if let Some(keymap) = full_match_keymap {
            LAPCE_STATE.run_command(
                ctx,
                ui_state,
                self.take_count(),
                &keymap.command,
                env,
            );
            return;
        }

        if pending_keypresses.len() > 0 {
            let mut full_match_keymap = None;
            for keymap in self.keymaps.iter() {
                if let Some(match_result) =
                    self.match_keymap_new(&mode, &pending_keypresses, keymap)
                {
                    if match_result == KeymapMatch::Full {
                        if full_match_keymap.is_none() {
                            full_match_keymap = Some(keymap.clone());
                        }
                    }
                }
            }
            if let Some(keymap) = full_match_keymap {
                LAPCE_STATE.run_command(
                    ctx,
                    ui_state,
                    self.take_count(),
                    &keymap.command,
                    env,
                );
                self.key_down(ctx, ui_state, key_event, env);
                return;
            }
        }

        if mode != Mode::Insert {
            self.handle_count(&mode, &keypress);
            return;
        }

        self.count = None;

        if mods.is_empty() {
            match &key_event.key {
                druid::keyboard_types::Key::Character(c) => {
                    LAPCE_STATE.insert(ctx, ui_state, c, env);
                }
                _ => (),
            }
        }
    }

    pub fn take_count(&mut self) -> Option<usize> {
        self.count.take()
    }

    fn match_keymap_new(
        &self,
        mode: &Mode,
        keypresses: &Vec<KeyPress>,
        keymap: &KeyMap,
    ) -> Option<KeymapMatch> {
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

        if !keymap.modes.is_empty() && !keymap.modes.contains(mode) {
            return None;
        }

        if let Some(condition) = &keymap.when {
            if !LAPCE_STATE.check_condition(condition) {
                return None;
            }
        }
        match_result
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
}
