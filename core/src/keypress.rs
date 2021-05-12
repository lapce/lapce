use std::str::FromStr;
use std::{collections::HashMap, io::Read};
use std::{fs::File, sync::Arc};

use anyhow::{anyhow, Result};
use druid::KbKey;
use druid::{
    Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Modifiers, Target, WidgetId,
    WindowId,
};
use toml;

use crate::{
    command::LapceCommand,
    state::{LapceFocus, LapceTabState, LapceUIState, Mode, LAPCE_APP_STATE},
};

#[derive(PartialEq)]
enum KeymapMatch {
    Full(String),
    Prefix,
    None,
}

#[derive(PartialEq, Eq, Hash, Default, Clone, Debug)]
pub struct KeyPress {
    pub key: druid::keyboard_types::Key,
    pub mods: Modifiers,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Vec<Mode>,
    pub when: Option<String>,
    pub command: String,
}

pub struct KeyPressState {
    window_id: WindowId,
    tab_id: WidgetId,
    pending_keypress: Vec<KeyPress>,
    count: Option<usize>,
    keymaps: Vec<KeyMap>,
}

pub trait KeyPressFocus {
    fn get_mode(&self) -> Mode;
    fn check_condition(&self, condition: &str) -> bool;
    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
    );
    fn insert(&self, c: &str);
}

#[derive(Clone, Debug)]
pub struct KeyPressData {
    pending_keypress: Vec<KeyPress>,
    keymaps: im::HashMap<Vec<KeyPress>, Vec<KeyMap>>,
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

    fn run_command<T: KeyPressFocus>(
        &self,
        ctx: &mut EventCtx,
        command: &str,
        count: Option<usize>,
        focus: &mut T,
    ) -> Result<()> {
        let cmd = LapceCommand::from_str(command)?;
        focus.run_command(ctx, &cmd, count);
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
    ) {
        if key_event.key == KbKey::Shift {
            return;
        }
        let mut mods = key_event.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        let keypress = KeyPress {
            key: key_event.key.clone(),
            mods,
        };

        let mode = focus.get_mode();
        if self.handle_count(&mode, &keypress) {
            return;
        }

        let mut keypresses = self.pending_keypress.clone();
        keypresses.push(keypress.clone());

        let matches = self.match_keymap(&keypresses, focus);
        let keymatch = if matches.len() == 0 {
            KeymapMatch::None
        } else if matches.len() == 1 && matches[0].key == keypresses {
            KeymapMatch::Full(matches[0].command.clone())
        } else {
            KeymapMatch::Prefix
        };
        match keymatch {
            KeymapMatch::Full(command) => {
                let count = self.count.take();
                self.run_command(ctx, &command, count, focus);
                self.pending_keypress = Vec::new();
                return;
            }
            KeymapMatch::Prefix => {
                self.pending_keypress.push(keypress);
                return;
            }
            KeymapMatch::None => {}
        }

        self.pending_keypress = Vec::new();
        self.count = None;

        if mods.is_empty() {
            match &key_event.key {
                druid::keyboard_types::Key::Character(c) => {
                    focus.insert(c);
                }
                _ => (),
            }
        }
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

    fn check_condition<T: KeyPressFocus>(&self, condition: &str, check: &T) -> bool {
        let or_indics: Vec<_> = condition.match_indices("||").collect();
        let and_indics: Vec<_> = condition.match_indices("&&").collect();
        if and_indics.is_empty() {
            if or_indics.is_empty() {
                return check.check_condition(condition);
            } else {
                return check.check_condition(&condition[..or_indics[0].0])
                    || self
                        .check_condition(&condition[or_indics[0].0 + 2..], check);
            }
        } else {
            if or_indics.is_empty() {
                return check.check_condition(&condition[..and_indics[0].0])
                    && self
                        .check_condition(&condition[and_indics[0].0 + 2..], check);
            } else {
                if or_indics[0].0 < and_indics[0].0 {
                    return check.check_condition(&condition[..or_indics[0].0])
                        || self.check_condition(
                            &condition[or_indics[0].0 + 2..],
                            check,
                        );
                } else {
                    return check.check_condition(&condition[..and_indics[0].0])
                        && self.check_condition(
                            &condition[and_indics[0].0 + 2..],
                            check,
                        );
                }
            }
        }
    }

    fn keymaps_from_str(
        s: &[u8],
    ) -> Result<im::HashMap<Vec<KeyPress>, Vec<KeyMap>>> {
        let toml_keymaps: toml::Value = toml::from_slice(s)?;
        let toml_keymaps = toml_keymaps
            .get("keymaps")
            .and_then(|v| v.as_array())
            .ok_or(anyhow!("no keymaps"))?;

        let mut keymaps: im::HashMap<Vec<KeyPress>, Vec<KeyMap>> =
            im::HashMap::new();
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

    fn get_keymaps() -> Result<im::HashMap<Vec<KeyPress>, Vec<KeyMap>>> {
        let mut f = File::open("/Users/Lulu/lapce/.lapce/keymaps.toml")?;
        let mut content = vec![];
        f.read_to_end(&mut content)?;
        Self::keymaps_from_str(&content)
    }

    fn get_keypress(key: &str) -> Vec<KeyPress> {
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
                        _ => druid::keyboard_types::Key::Character(part.to_string()),
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

impl KeyPressState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> KeyPressState {
        KeyPressState {
            window_id,
            tab_id,
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

    fn get_keypress(key: &str) -> Vec<KeyPress> {
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
                        _ => druid::keyboard_types::Key::Character(part.to_string()),
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

    pub fn key_down(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        key_event: &KeyEvent,
        env: &Env,
    ) {
        if key_event.key == KbKey::Shift {
            return;
        }
        let mut mods = key_event.mods.clone();
        mods.set(Modifiers::SHIFT, false);
        let keypress = KeyPress {
            key: key_event.key.clone(),
            mods,
        };

        let mode = LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .get_mode();
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
                    KeymapMatch::Full(_) => {
                        if full_match_keymap.is_none() {
                            full_match_keymap = Some(keymap.clone());
                        }
                    }
                    KeymapMatch::Prefix => {
                        self.pending_keypress.push(keypress.clone());
                        return;
                    }
                    KeymapMatch::None => {}
                }
            }
        }

        let pending_keypresses = self.pending_keypress.clone();
        self.pending_keypress = Vec::new();

        if let Some(keymap) = full_match_keymap {
            LAPCE_APP_STATE
                .get_tab_state(&self.window_id, &self.tab_id)
                .run_command(ctx, ui_state, self.take_count(), &keymap.command, env);
            return;
        }

        if pending_keypresses.len() > 0 {
            let mut full_match_keymap = None;
            for keymap in self.keymaps.iter() {
                if let Some(match_result) =
                    self.match_keymap_new(&mode, &pending_keypresses, keymap)
                {
                    if match_result == KeymapMatch::Full("".to_string()) {
                        if full_match_keymap.is_none() {
                            full_match_keymap = Some(keymap.clone());
                        }
                    }
                }
            }
            if let Some(keymap) = full_match_keymap {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .run_command(
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
                    LAPCE_APP_STATE
                        .get_tab_state(&self.window_id, &self.tab_id)
                        .insert(ctx, ui_state, c, env);
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
            Some(KeymapMatch::Full(keymap.command.clone()))
        } else {
            None
        };

        if !keymap.modes.is_empty() && !keymap.modes.contains(mode) {
            return None;
        }

        if let Some(condition) = &keymap.when {
            if !LAPCE_APP_STATE
                .get_tab_state(&self.window_id, &self.tab_id)
                .check_condition(condition)
            {
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
        let keymaps = KeyPressData::keymaps_from_str(keymaps.as_bytes()).unwrap();
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
