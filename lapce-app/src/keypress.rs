pub mod condition;
mod key;
pub mod keymap;
mod loader;
mod press;

use std::{path::PathBuf, rc::Rc, str::FromStr, time::SystemTime};

use anyhow::Result;
use floem::{
    keyboard::{Key, KeyEvent, KeyEventExtModifierSupplement, Modifiers, NamedKey},
    pointer::{MouseButton, PointerButton, PointerInputEvent},
    reactive::{RwSignal, Scope, SignalUpdate, SignalWith},
};
use indexmap::IndexMap;
use itertools::Itertools;
use lapce_core::mode::{Mode, Modes};

pub use self::press::KeyPress;
use self::{
    key::KeyInput,
    keymap::{KeyMap, KeyMapPress},
    loader::KeyMapLoader,
};
use crate::{
    command::{CommandExecuted, CommandKind, LapceCommand, lapce_internal_commands},
    config::LapceConfig,
    keypress::{
        condition::{CheckCondition, Condition},
        keymap::KeymapMatch,
    },
    tracing::*,
};

const DEFAULT_KEYMAPS_COMMON: &str =
    include_str!("../../defaults/keymaps-common.toml");
const DEFAULT_KEYMAPS_MACOS: &str =
    include_str!("../../defaults/keymaps-macos.toml");
const DEFAULT_KEYMAPS_NONMACOS: &str =
    include_str!("../../defaults/keymaps-nonmacos.toml");

pub trait KeyPressFocus: std::fmt::Debug {
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
impl KeyPressFocus for () {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, _condition: Condition) -> bool {
        false
    }

    fn run_command(
        &self,
        _command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
    ) -> CommandExecuted {
        CommandExecuted::No
    }

    fn expect_char(&self) -> bool {
        false
    }

    fn focus_only(&self) -> bool {
        false
    }

    fn receive_char(&self, _c: &str) {}
}
impl KeyPressFocus for Box<dyn KeyPressFocus> {
    fn get_mode(&self) -> Mode {
        (**self).get_mode()
    }

    fn check_condition(&self, condition: Condition) -> bool {
        (**self).check_condition(condition)
    }

    fn run_command(
        &self,
        command: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        (**self).run_command(command, count, mods)
    }

    fn expect_char(&self) -> bool {
        (**self).expect_char()
    }

    fn focus_only(&self) -> bool {
        (**self).focus_only()
    }

    fn receive_char(&self, c: &str) {
        (**self).receive_char(c)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EventRef<'a> {
    Keyboard(&'a floem::keyboard::KeyEvent),
    Pointer(&'a floem::pointer::PointerInputEvent),
}

impl<'a> From<&'a KeyEvent> for EventRef<'a> {
    fn from(ev: &'a KeyEvent) -> Self {
        Self::Keyboard(ev)
    }
}

impl<'a> From<&'a PointerInputEvent> for EventRef<'a> {
    fn from(ev: &'a PointerInputEvent) -> Self {
        Self::Pointer(ev)
    }
}

pub struct KeyPressHandle {
    pub handled: bool,
    pub keypress: KeyPress,
    pub keymatch: KeymapMatch,
}

#[derive(Clone, Debug)]
pub struct KeyPressData {
    count: RwSignal<Option<usize>>,
    pending_keypress: RwSignal<(Vec<KeyPress>, Option<SystemTime>)>,
    pub commands: Rc<IndexMap<String, LapceCommand>>,
    pub keymaps: Rc<IndexMap<Vec<KeyMapPress>, Vec<KeyMap>>>,
    pub command_keymaps: Rc<IndexMap<String, Vec<KeyMap>>>,
    pub commands_with_keymap: Rc<Vec<KeyMap>>,
    pub commands_without_keymap: Rc<Vec<LapceCommand>>,
}

impl KeyPressData {
    pub fn new(cx: Scope, config: &LapceConfig) -> Self {
        let (keymaps, command_keymaps) =
            Self::get_keymaps(config).unwrap_or((IndexMap::new(), IndexMap::new()));
        let mut keypress = Self {
            count: cx.create_rw_signal(None),
            pending_keypress: cx.create_rw_signal((Vec::new(), None)),
            keymaps: Rc::new(keymaps),
            command_keymaps: Rc::new(command_keymaps),
            commands: Rc::new(lapce_internal_commands()),
            commands_with_keymap: Rc::new(Vec::new()),
            commands_without_keymap: Rc::new(Vec::new()),
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
            if self
                .command_keymaps
                .get(cmd.kind.str())
                .map(|x| x.is_empty())
                .unwrap_or(true)
            {
                commands_without_keymap.push(cmd.clone());
            }
        }

        self.commands_with_keymap = Rc::new(commands_with_keymap);
        self.commands_without_keymap = Rc::new(commands_without_keymap);
    }

    fn handle_count<T: KeyPressFocus + ?Sized>(
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

        if let KeyInput::Keyboard {
            logical: Key::Character(c),
            ..
        } = &keypress.key
        {
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

    fn run_command<T: KeyPressFocus + ?Sized>(
        &self,
        command: &str,
        count: Option<usize>,
        mods: Modifiers,
        focus: &T,
    ) -> CommandExecuted {
        if let Some(cmd) = self.commands.get(command) {
            focus.run_command(cmd, count, mods)
        } else {
            CommandExecuted::No
        }
    }

    pub fn keypress<'a>(event: impl Into<EventRef<'a>>) -> Option<KeyPress> {
        let event = event.into();

        let keypress = match event {
            EventRef::Keyboard(ev) => KeyPress {
                key: KeyInput::Keyboard {
                    logical: ev.key.logical_key.to_owned(),
                    physical: ev.key.physical_key,
                    key_without_modifiers: ev.key.key_without_modifiers(),
                    location: ev.key.location,
                    repeat: ev.key.repeat,
                },
                mods: Self::get_key_modifiers(ev),
            },
            EventRef::Pointer(ev) => KeyPress {
                key: KeyInput::Pointer(ev.button),
                mods: ev.modifiers,
            },
        };
        Some(keypress)
    }

    pub fn key_down<'a, T: KeyPressFocus + ?Sized>(
        &self,
        event: impl Into<EventRef<'a>>,
        focus: &T,
    ) -> KeyPressHandle {
        let keypress = match Self::keypress(event) {
            Some(keypress) => keypress,
            None => {
                return KeyPressHandle {
                    handled: false,
                    keymatch: KeymapMatch::None,
                    keypress: KeyPress {
                        key: KeyInput::Pointer(PointerButton::Mouse(
                            MouseButton::Primary,
                        )),
                        mods: Modifiers::empty(),
                    },
                };
            }
        };

        if self.handle_count(focus, &keypress) {
            return KeyPressHandle {
                handled: true,
                keymatch: KeymapMatch::None,
                keypress,
            };
        }

        self.pending_keypress
            .update(|(pending_keypress, last_time)| {
                let last_time = last_time.replace(SystemTime::now());
                if let Some(last_time_val) = last_time {
                    if last_time_val
                        .elapsed()
                        .map(|x| x.as_millis() > 1000)
                        .unwrap_or_default()
                    {
                        pending_keypress.clear();
                    }
                }
                pending_keypress.push(keypress.clone());
            });

        let keymatch =
            self.pending_keypress
                .with_untracked(|(pending_keypress, _)| {
                    self.match_keymap(pending_keypress, focus)
                });
        self.handle_keymatch(focus, keymatch, keypress)
    }

    pub fn handle_keymatch<T: KeyPressFocus + ?Sized>(
        &self,
        focus: &T,
        keymatch: KeymapMatch,
        keypress: KeyPress,
    ) -> KeyPressHandle {
        let mods = keypress.mods;
        match &keymatch {
            KeymapMatch::Full(command) => {
                self.pending_keypress
                    .update(|(pending_keypress, last_time)| {
                        last_time.take();
                        pending_keypress.clear();
                    });
                let count = self.count.try_update(|count| count.take()).unwrap();
                let handled = self.run_command(command, count, mods, focus)
                    == CommandExecuted::Yes;
                return KeyPressHandle {
                    handled,
                    keymatch,
                    keypress,
                };
            }
            KeymapMatch::Multiple(commands) => {
                self.pending_keypress
                    .update(|(pending_keypress, last_time)| {
                        last_time.take();
                        pending_keypress.clear();
                    });
                let count = self.count.try_update(|count| count.take()).unwrap();
                for command in commands {
                    let handled = self.run_command(command, count, mods, focus)
                        == CommandExecuted::Yes;
                    if handled {
                        return KeyPressHandle {
                            handled,
                            keymatch,
                            keypress,
                        };
                    }
                }

                return KeyPressHandle {
                    handled: false,
                    keymatch,
                    keypress,
                };
            }
            KeymapMatch::Prefix => {
                // Here pending_keypress contains only a prefix of some keymap, so let's keep
                // collecting key presses.
                return KeyPressHandle {
                    handled: true,
                    keymatch,
                    keypress,
                };
            }
            KeymapMatch::None => {
                self.pending_keypress
                    .update(|(pending_keypress, last_time)| {
                        pending_keypress.clear();
                        last_time.take();
                    });
                if focus.get_mode() == Mode::Insert {
                    let old_keypress = keypress.clone();
                    let mut keypress = keypress.clone();
                    keypress.mods.set(Modifiers::SHIFT, false);
                    if let KeymapMatch::Full(command) =
                        self.match_keymap(&[keypress], focus)
                    {
                        if let Some(cmd) = self.commands.get(&command) {
                            if let CommandKind::Move(_) = cmd.kind {
                                let handled = focus.run_command(cmd, None, mods)
                                    == CommandExecuted::Yes;
                                return KeyPressHandle {
                                    handled,
                                    keymatch,
                                    keypress: old_keypress,
                                };
                            }
                        }
                    }
                }
            }
        }

        let mut mods = keypress.mods;

        #[cfg(target_os = "macos")]
        {
            mods.set(Modifiers::SHIFT, false);
            mods.set(Modifiers::ALT, false);
        }
        #[cfg(not(target_os = "macos"))]
        {
            mods.set(Modifiers::SHIFT, false);
            mods.set(Modifiers::ALTGR, false);
        }
        if mods.is_empty() {
            if let KeyInput::Keyboard { logical, .. } = &keypress.key {
                if let Key::Character(c) = logical {
                    focus.receive_char(c);
                    self.count.set(None);
                    return KeyPressHandle {
                        handled: true,
                        keymatch,
                        keypress,
                    };
                } else if let Key::Named(NamedKey::Space) = logical {
                    focus.receive_char(" ");
                    self.count.set(None);
                    return KeyPressHandle {
                        handled: true,
                        keymatch,
                        keypress,
                    };
                }
            }
        }

        KeyPressHandle {
            handled: false,
            keymatch,
            keypress,
        }
    }

    fn get_key_modifiers(key_event: &KeyEvent) -> Modifiers {
        let mut mods = key_event.modifiers;

        match &key_event.key.logical_key {
            Key::Named(NamedKey::Shift) => mods.set(Modifiers::SHIFT, false),
            Key::Named(NamedKey::Alt) => mods.set(Modifiers::ALT, false),
            Key::Named(NamedKey::Meta) => mods.set(Modifiers::META, false),
            Key::Named(NamedKey::Control) => mods.set(Modifiers::CONTROL, false),
            Key::Named(NamedKey::AltGraph) => mods.set(Modifiers::ALTGR, false),
            _ => (),
        }

        mods
    }

    fn match_keymap<T: KeyPressFocus + ?Sized>(
        &self,
        keypresses: &[KeyPress],
        check: &T,
    ) -> KeymapMatch {
        let keypresses: Vec<KeyMapPress> =
            keypresses.iter().filter_map(|k| k.keymap_press()).collect();
        let matches: Vec<_> = self
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
            .unwrap_or_default();

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

    fn check_condition<T: KeyPressFocus + ?Sized>(
        condition: &str,
        check: &T,
    ) -> bool {
        fn check_one_condition<T: KeyPressFocus + ?Sized>(
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
        IndexMap<Vec<KeyMapPress>, Vec<KeyMap>>,
        IndexMap<String, Vec<KeyMap>>,
    )> {
        let is_modal = config.core.modal;

        let mut loader = KeyMapLoader::new();

        if let Err(err) = loader.load_from_str(DEFAULT_KEYMAPS_COMMON, is_modal) {
            trace!(TraceLevel::ERROR, "Failed to load common defaults: {err}");
        }

        let os_keymaps = if std::env::consts::OS == "macos" {
            DEFAULT_KEYMAPS_MACOS
        } else {
            DEFAULT_KEYMAPS_NONMACOS
        };

        if let Err(err) = loader.load_from_str(os_keymaps, is_modal) {
            trace!(TraceLevel::ERROR, "Failed to load OS defaults: {err}");
        }

        if let Some(path) = Self::file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Err(err) = loader.load_from_str(&content, is_modal) {
                    trace!(TraceLevel::WARN, "Failed to load from {path:?}: {err}");
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

    pub fn update_file(keymap: &KeyMap, keys: &[KeyMapPress]) -> Option<()> {
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
                        .map(KeyMapPress::parse)
        });

        if let Some(index) = index {
            if !keys.is_empty() {
                array.get_mut(index)?.insert(
                    "key",
                    toml_edit::value(toml_edit::Value::from(keys.iter().join(" "))),
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
                    toml_edit::value(toml_edit::Value::from(keys.iter().join(" "))),
                );
                array.push(table.clone());
            }

            if !keymap.key.is_empty() {
                table.insert(
                    "key",
                    toml_edit::value(toml_edit::Value::from(
                        keymap.key.iter().join(" "),
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
