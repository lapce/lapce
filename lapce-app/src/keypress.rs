pub mod condition;
mod key;
pub mod keymap;
mod loader;
mod press;

use std::{path::PathBuf, rc::Rc, str::FromStr};

use anyhow::Result;
use floem::{
    glazier::{KbKey, KeyEvent, Modifiers, PointerEvent},
    reactive::{RwSignal, Scope},
};
use indexmap::IndexMap;
use itertools::Itertools;
use lapce_core::mode::{Mode, Modes};
use tracing::{debug, error};

use self::{key::Key, keymap::KeyMap, loader::KeyMapLoader};
use crate::{
    command::{
        lapce_internal_commands, CommandExecuted, CommandKind, LapceCommand,
        LapceWorkbenchCommand,
    },
    config::LapceConfig,
    keypress::{
        condition::{CheckCondition, Condition},
        keymap::KeymapMatch,
    },
    listener::Listener,
};

pub use self::press::KeyPress;

const DEFAULT_KEYMAPS_COMMON: &str =
    include_str!("../../defaults/keymaps-common.toml");
const DEFAULT_KEYMAPS_MACOS: &str =
    include_str!("../../defaults/keymaps-macos.toml");
const DEFAULT_KEYMAPS_NONMACOS: &str =
    include_str!("../../defaults/keymaps-nonmacos.toml");

pub trait KeyPressFocus {
    fn get_mode(&self) -> Mode;

    fn check_condition(&self, condition: Condition) -> bool;

    fn run_command(
        &self,
        command: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted;

    fn expect_char(&self) -> bool {
        false
    }

    fn focus_only(&self) -> bool {
        false
    }

    fn receive_char(&self, c: &str);
}

#[derive(Clone, Copy, Debug)]
pub enum EventRef<'a> {
    Keyboard(&'a floem::glazier::KeyEvent),
    Pointer(&'a floem::glazier::PointerEvent),
}

impl<'a> From<&'a KeyEvent> for EventRef<'a> {
    fn from(ev: &'a KeyEvent) -> Self {
        Self::Keyboard(ev)
    }
}

impl<'a> From<&'a PointerEvent> for EventRef<'a> {
    fn from(ev: &'a PointerEvent) -> Self {
        Self::Pointer(ev)
    }
}

#[derive(Clone)]
pub struct KeyPressData {
    count: RwSignal<Option<usize>>,
    pending_keypress: RwSignal<Vec<KeyPress>>,
    workbench_cmd: Listener<LapceWorkbenchCommand>,
    pub commands: Rc<IndexMap<String, LapceCommand>>,
    pub keymaps: Rc<IndexMap<Vec<KeyPress>, Vec<KeyMap>>>,
    pub command_keymaps: Rc<IndexMap<String, Vec<KeyMap>>>,
    pub commands_with_keymap: Rc<Vec<KeyMap>>,
    pub commands_without_keymap: Rc<Vec<LapceCommand>>,
}

impl KeyPressData {
    pub fn new(
        cx: Scope,
        config: &LapceConfig,
        workbench_cmd: Listener<LapceWorkbenchCommand>,
    ) -> Self {
        let (keymaps, command_keymaps) =
            Self::get_keymaps(config).unwrap_or((IndexMap::new(), IndexMap::new()));
        let mut keypress = Self {
            count: cx.create_rw_signal(None),
            pending_keypress: cx.create_rw_signal(Vec::new()),
            keymaps: Rc::new(keymaps),
            command_keymaps: Rc::new(command_keymaps),
            commands: Rc::new(lapce_internal_commands()),
            commands_with_keymap: Rc::new(Vec::new()),
            commands_without_keymap: Rc::new(Vec::new()),
            workbench_cmd,
        };
        keypress.load_commands();
        keypress
    }

    pub fn update_keymaps(&mut self, config: &LapceConfig) {
        if let Ok((new_keymaps, new_command_keymaps)) = Self::get_keymaps(config) {
            self.keymaps = Rc::new(new_keymaps);
            self.command_keymaps = Rc::new(new_command_keymaps);
            self.load_commands();
        }
    }

    fn load_commands(&mut self) {
        let mut commands_with_keymap = Vec::new();
        let mut commands_without_keymap = Vec::new();
        for (_, keymaps) in self.command_keymaps.iter() {
            for keymap in keymaps.iter() {
                if self.commands.get(&keymap.command).is_some() {
                    commands_with_keymap.push(keymap.clone());
                }
            }
        }

        for (_, cmd) in self.commands.iter() {
            if !self.command_keymaps.contains_key(cmd.kind.str()) {
                commands_without_keymap.push(cmd.clone());
            }
        }

        self.commands_with_keymap = Rc::new(commands_with_keymap);
        self.commands_without_keymap = Rc::new(commands_without_keymap);
    }

    fn handle_count<T: KeyPressFocus>(
        &self,
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

        if let Key::Keyboard(KbKey::Character(c)) = &keypress.key {
            if let Ok(n) = c.parse::<usize>() {
                if self.count.with_untracked(|count| count.is_some()) || n > 0 {
                    self.count
                        .update(|count| *count = Some(count.unwrap_or(0) * 10 + n));
                    return true;
                }
            }
        }

        false
    }

    fn run_command<T: KeyPressFocus>(
        &self,
        command: &str,
        count: Option<usize>,
        mods: Modifiers,
        focus: &T,
    ) -> CommandExecuted {
        if let Some(cmd) = self.commands.get(command) {
            match &cmd.kind {
                CommandKind::Workbench(cmd) => {
                    self.workbench_cmd.send(cmd.clone());
                    CommandExecuted::Yes
                }
                CommandKind::Move(_)
                | CommandKind::Edit(_)
                | CommandKind::Focus(_)
                | CommandKind::MotionMode(_)
                | CommandKind::MultiSelection(_) => {
                    focus.run_command(cmd, count, mods)
                }
            }
        } else {
            CommandExecuted::No
        }
    }

    pub fn keypress<'a>(event: impl Into<EventRef<'a>>) -> Option<KeyPress> {
        let event = event.into();
        debug!("{event:?}");

        let keypress = match event {
            EventRef::Keyboard(ev)
                if ev.key == KbKey::Shift && ev.mods.is_empty() =>
            {
                return None;
            }
            EventRef::Keyboard(ev) => KeyPress {
                key: Key::Keyboard(ev.key.clone()),
                // We are removing Shift modifier since the character is already upper case.
                mods: Self::get_key_modifiers(ev),
            },
            EventRef::Pointer(ev) => KeyPress {
                key: Key::Pointer(ev.button),
                mods: ev.modifiers,
            },
        };
        Some(keypress)
    }

    pub fn key_down<'a, T: KeyPressFocus>(
        &self,
        event: impl Into<EventRef<'a>>,
        focus: &T,
    ) -> bool {
        let keypress = match Self::keypress(event) {
            Some(keypress) => keypress,
            None => return false,
        };
        let mods = keypress.mods;

        let mode = focus.get_mode();
        if self.handle_count(focus, &keypress) {
            return false;
        }

        self.pending_keypress.update(|pending_keypress| {
            pending_keypress.push(keypress.clone());
        });

        let keymatch = self.pending_keypress.with_untracked(|pending_keypress| {
            self.match_keymap(pending_keypress, focus)
        });
        match keymatch {
            KeymapMatch::Full(command) => {
                self.pending_keypress.update(|pending_keypress| {
                    pending_keypress.clear();
                });
                let count = self.count.try_update(|count| count.take()).unwrap();
                self.run_command(&command, count, mods, focus);
                return true;
            }
            KeymapMatch::Multiple(commands) => {
                self.pending_keypress.update(|pending_keypress| {
                    pending_keypress.clear();
                });
                let count = self.count.try_update(|count| count.take()).unwrap();
                for command in commands {
                    if self.run_command(&command, count, mods, focus)
                        == CommandExecuted::Yes
                    {
                        return true;
                    }
                }

                return true;
            }
            KeymapMatch::Prefix => {
                // Here pending_keypress contains only a prefix of some keymap, so let's keep
                // collecting key presses.
                return false;
            }
            KeymapMatch::None => {
                self.pending_keypress.update(|pending_keypress| {
                    pending_keypress.clear();
                });
                if focus.get_mode() == Mode::Insert {
                    let mut keypress = keypress.clone();
                    keypress.mods.set(Modifiers::SHIFT, false);
                    if let KeymapMatch::Full(command) =
                        self.match_keymap(&[keypress], focus)
                    {
                        if let Some(cmd) = self.commands.get(&command) {
                            if let CommandKind::Move(_) = cmd.kind {
                                focus.run_command(cmd, None, mods);
                                return true;
                            }
                        }
                    }
                }
            }
        }

        if mode != Mode::Insert
            && mode != Mode::Terminal
            && self.handle_count(focus, &keypress)
        {
            return false;
        }

        self.count.set(None);

        let mut mods = keypress.mods;

        #[cfg(not(target_os = "macos"))]
        {
            mods.set(Modifiers::SHIFT, false);
            if mods.is_empty() {
                if let Key::Keyboard(KbKey::Character(c)) = &keypress.key {
                    focus.receive_char(c);
                    return true;
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            mods.set(Modifiers::SHIFT, false);
            mods.set(Modifiers::ALT, false);
            if mods.is_empty() {
                if let Key::Keyboard(KbKey::Character(c)) = &keypress.key {
                    focus.receive_char(c);
                    return true;
                }
            }
        }

        false
    }

    fn get_key_modifiers(key_event: &KeyEvent) -> Modifiers {
        // We only care about some modifiers
        let mut mods = (Modifiers::ALT
            | Modifiers::CONTROL
            | Modifiers::SHIFT
            | Modifiers::META)
            & key_event.mods;

        if mods == Modifiers::SHIFT {
            if let KbKey::Character(c) = &key_event.key {
                if !c.chars().all(|c| c.is_alphabetic()) {
                    // We remove the shift if there's only shift pressed,
                    // and the character isn't a letter
                    return Modifiers::empty();
                }
            }
        }

        match &key_event.key {
            KbKey::Shift => mods.set(Modifiers::SHIFT, false),
            KbKey::Alt => mods.set(Modifiers::ALT, false),
            KbKey::Meta => mods.set(Modifiers::META, false),
            KbKey::Control => mods.set(Modifiers::CONTROL, false),
            _ => (),
        }

        mods
    }

    fn match_keymap<T: KeyPressFocus>(
        &self,
        keypresses: &[KeyPress],
        check: &T,
    ) -> KeymapMatch {
        let keypresses: Vec<KeyPress> =
            keypresses.iter().map(KeyPress::to_lowercase).collect();
        let matches = self
            .keymaps
            .get(&keypresses)
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
                        if !keymap.modes.is_empty()
                            && !keymap.modes.contains(check.get_mode().into())
                        {
                            return false;
                        }
                        if let Some(condition) = &keymap.when {
                            if !Self::check_condition(condition, check) {
                                return false;
                            }
                        }
                        true
                    })
                    .collect()
            })
            .unwrap_or_else(Vec::new);

        if matches.is_empty() {
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
        }
    }

    fn check_condition<T: KeyPressFocus>(condition: &str, check: &T) -> bool {
        fn check_one_condition<T: KeyPressFocus>(
            condition: &str,
            check: &T,
        ) -> bool {
            let trimmed = condition.trim();
            if let Some(stripped) = trimmed.strip_prefix('!') {
                if let Ok(condition) = Condition::from_str(stripped) {
                    !check.check_condition(condition)
                } else {
                    true
                }
            } else if let Ok(condition) = Condition::from_str(trimmed) {
                check.check_condition(condition)
            } else {
                false
            }
        }

        match CheckCondition::parse_first(condition) {
            CheckCondition::Single(condition) => {
                check_one_condition(condition, check)
            }
            CheckCondition::Or(left, right) => {
                let left = check_one_condition(left, check);
                let right = Self::check_condition(right, check);

                left || right
            }
            CheckCondition::And(left, right) => {
                let left = check_one_condition(left, check);
                let right = Self::check_condition(right, check);

                left && right
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn get_keymaps(
        config: &LapceConfig,
    ) -> Result<(
        IndexMap<Vec<KeyPress>, Vec<KeyMap>>,
        IndexMap<String, Vec<KeyMap>>,
    )> {
        let is_modal = config.core.modal;

        let mut loader = KeyMapLoader::new();

        if let Err(err) = loader.load_from_str(DEFAULT_KEYMAPS_COMMON, is_modal) {
            error!("Failed to load common defaults: {err}");
        }

        let os_keymaps = if std::env::consts::OS == "macos" {
            DEFAULT_KEYMAPS_MACOS
        } else {
            DEFAULT_KEYMAPS_NONMACOS
        };

        if let Err(err) = loader.load_from_str(os_keymaps, is_modal) {
            error!("Failed to load OS defaults: {err}");
        }

        if let Some(path) = Self::file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Err(err) = loader.load_from_str(&content, is_modal) {
                    error!("Failed to load from {path:?}: {err}");
                }
            }
        }

        Ok(loader.finalize())
    }

    pub fn file() -> Option<PathBuf> {
        LapceConfig::keymaps_file()
    }

    fn get_file_array() -> Option<toml_edit::ArrayOfTables> {
        let path = Self::file()?;
        let content = std::fs::read_to_string(path).ok()?;
        let document: toml_edit::Document = content.parse().ok()?;
        document
            .as_table()
            .get("keymaps")?
            .as_array_of_tables()
            .cloned()
    }

    pub fn update_file(keymap: &KeyMap, keys: &[KeyPress]) -> Option<()> {
        let mut array = Self::get_file_array().unwrap_or_default();
        let index = array.iter().position(|value| {
            Some(keymap.command.as_str())
                == value.get("command").and_then(|c| c.as_str())
                && keymap.when.as_deref()
                    == value.get("when").and_then(|w| w.as_str())
                && keymap.modes == get_modes(value)
                && Some(keymap.key.clone())
                    == value
                        .get("key")
                        .and_then(|v| v.as_str())
                        .map(KeyPress::parse)
        });

        if let Some(index) = index {
            if !keys.is_empty() {
                array.get_mut(index)?.insert(
                    "key",
                    toml_edit::value(toml_edit::Value::from(
                        keys.iter().map(|k| k.to_string()).join(" "),
                    )),
                );
            } else {
                array.remove(index);
            };
        } else {
            let mut table = toml_edit::Table::new();
            table.insert(
                "command",
                toml_edit::value(toml_edit::Value::from(keymap.command.clone())),
            );
            if !keymap.modes.is_empty() {
                table.insert(
                    "mode",
                    toml_edit::value(toml_edit::Value::from(
                        keymap.modes.to_string(),
                    )),
                );
            }
            if let Some(when) = keymap.when.as_ref() {
                table.insert(
                    "when",
                    toml_edit::value(toml_edit::Value::from(when.to_string())),
                );
            }

            if !keys.is_empty() {
                table.insert(
                    "key",
                    toml_edit::value(toml_edit::Value::from(
                        keys.iter().map(|k| k.to_string()).join(" "),
                    )),
                );
                array.push(table.clone());
            }

            if !keymap.key.is_empty() {
                table.insert(
                    "key",
                    toml_edit::value(toml_edit::Value::from(
                        keymap.key.iter().map(|k| k.to_string()).join(" "),
                    )),
                );
                table.insert(
                    "command",
                    toml_edit::value(toml_edit::Value::from(format!(
                        "-{}",
                        keymap.command
                    ))),
                );
                array.push(table.clone());
            }
        }

        let mut table = toml_edit::Document::new();
        table.insert("keymaps", toml_edit::Item::ArrayOfTables(array));
        let path = Self::file()?;
        std::fs::write(path, table.to_string().as_bytes()).ok()?;
        None
    }
}

fn get_modes(toml_keymap: &toml_edit::Table) -> Modes {
    toml_keymap
        .get("mode")
        .and_then(|v| v.as_str())
        .map(Modes::parse)
        .unwrap_or_else(Modes::empty)
}
