use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use druid::piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder};
use druid::{Command, KbKey};
use druid::{
    Env, EventCtx, ExtEventSink, FontFamily, KeyEvent, Modifiers, PaintCtx, Point,
    Rect, RenderContext, Size, Target,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use indexmap::IndexMap;
use itertools::Itertools;
use toml;

use crate::command::{
    lapce_internal_commands, CommandExecuted, CommandTarget, LapceCommandNew,
    LapceUICommand, LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND,
};
use crate::config::{Config, LapceTheme};
use crate::{command::LapceCommand, state::Mode};

const DEFAULT_KEYMAPS_COMMON: &str =
    include_str!("../../../defaults/keymaps-common.toml");
const DEFAULT_KEYMAPS_WINDOWS: &str =
    include_str!("../../../defaults/keymaps-windows.toml");
const DEFAULT_KEYMAPS_MACOS: &str =
    include_str!("../../../defaults/keymaps-macos.toml");
const DEFAULT_KEYMAPS_LINUX: &str =
    include_str!("../../../defaults/keymaps-linux.toml");

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

impl Display for KeyPress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mods.ctrl() {
            let _ = f.write_str("Ctrl+");
        }
        if self.mods.alt() {
            let _ = f.write_str("Alt+");
        }
        if self.mods.meta() {
            let _ = f.write_str("Meta+");
        }
        if self.mods.shift() {
            let _ = f.write_str("Shift+");
        }
        f.write_str(&self.key.to_string())
    }
}

impl KeyPress {
    pub fn is_char(&self) -> bool {
        let mut mods = self.mods;
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            if let druid::KbKey::Character(_c) = &self.key {
                return true;
            }
        }
        false
    }

    pub fn paint(
        &self,
        ctx: &mut PaintCtx,
        origin: Point,
        config: &Config,
    ) -> (Point, Vec<(Option<Rect>, PietTextLayout, Point)>) {
        let mut origin = origin;
        let mut keys = Vec::new();
        if self.mods.ctrl() {
            keys.push("Ctrl".to_string());
        }
        if self.mods.alt() {
            keys.push("Alt".to_string());
        }
        if self.mods.meta() {
            let keyname = match std::env::consts::OS {
                "macos" => "Cmd",
                "windows" => "Win",
                _ => "Meta",
            };
            keys.push(keyname.to_string());
        }
        if self.mods.shift() {
            keys.push("Shift".to_string());
        }
        match &self.key {
            druid::keyboard_types::Key::Character(c) => {
                if *c == c.to_uppercase()
                    && c.to_lowercase() != c.to_uppercase()
                    && !self.mods.shift()
                {
                    keys.push("Shift".to_string());
                }
                keys.push(c.to_uppercase());
            }
            _ => {
                keys.push(self.key.to_string());
            }
        }

        let mut items = Vec::new();
        let keys_len = keys.len();
        for (i, key) in keys.iter().enumerate() {
            let (rect, text_layout, text_layout_pos) =
                paint_key(ctx, key, origin, config);
            origin += (rect.width() + 5.0, 0.0);

            items.push((Some(rect), text_layout, text_layout_pos));

            if i < keys_len - 1 {
                let text_layout = ctx
                    .text()
                    .new_text_layout("+")
                    .font(FontFamily::SYSTEM_UI, 13.0)
                    .text_color(
                        config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                let text_size = text_layout.size();
                let text_layout_pos = origin + (0.0, -(text_size.height / 2.0));
                items.push((None, text_layout, text_layout_pos));
                origin += (text_size.width + 5.0, 0.0);
            }
        }

        (origin, items)
    }
}

pub fn paint_key(
    ctx: &mut PaintCtx,
    text: &str,
    origin: Point,
    config: &Config,
) -> (Rect, PietTextLayout, Point) {
    let text_layout = ctx
        .text()
        .new_text_layout(text.to_string())
        .font(FontFamily::SYSTEM_UI, 13.0)
        .text_color(
            config
                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                .clone(),
        )
        .build()
        .unwrap();
    let text_size = text_layout.size();
    let text_layout_point = origin + (5.0, -(text_size.height / 2.0));
    let rect = Size::new(text_size.width, 0.0)
        .to_rect()
        .with_origin(origin + (5.0, 0.0))
        .inflate(5.0, text_size.height / 2.0 + 4.0);
    (rect, text_layout, text_layout_point)
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Vec<Mode>,
    pub when: Option<String>,
    pub command: String,
}

pub enum Alignment {
    Left,
    Center,
    Right,
}

impl KeyMap {
    pub fn paint(
        &self,
        ctx: &mut PaintCtx,
        origin: Point,
        align: Alignment,
        config: &Config,
    ) {
        let old_origin = origin;

        let mut origin = origin;
        let mut items = Vec::new();
        for keypress in self.key.iter() {
            let (new_origin, mut new_items) = keypress.paint(ctx, origin, config);
            origin = new_origin + (10.0, 0.0);
            items.append(&mut new_items);
        }

        let x_shift = match align {
            Alignment::Left => 0.0,
            Alignment::Center => (origin.x - old_origin.x) / 2.0,
            Alignment::Right => (origin.x - old_origin.x),
        };

        for (rect, text_layout, text_layout_pos) in items {
            if let Some(mut rect) = rect {
                rect.x0 -= x_shift;
                rect.x1 -= x_shift;
                ctx.stroke(
                    rect,
                    config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                    1.0,
                );
            }
            ctx.draw_text(&text_layout, text_layout_pos - (x_shift, 0.0));
        }
    }
}

pub trait KeyPressFocus {
    fn get_mode(&self) -> Mode;
    fn check_condition(&self, condition: &str) -> bool;
    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
        env: &Env,
    ) -> CommandExecuted;
    fn expect_char(&self) -> bool {
        false
    }
    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str);
}

#[derive(Clone)]
pub struct KeyPressData {
    pending_keypress: Vec<KeyPress>,
    pub commands: Arc<IndexMap<String, LapceCommandNew>>,
    pub keymaps: Arc<IndexMap<Vec<KeyPress>, Vec<KeyMap>>>,
    pub command_keymaps: Arc<IndexMap<String, Vec<KeyMap>>>,

    pub commands_with_keymap: Arc<Vec<KeyMap>>,
    pub commands_without_keymap: Arc<Vec<LapceCommandNew>>,
    pub filtered_commands_with_keymap: Arc<Vec<KeyMap>>,
    pub filtered_commands_without_keymap: Arc<Vec<LapceCommandNew>>,
    pub filter_pattern: String,

    count: Option<usize>,

    event_sink: Arc<ExtEventSink>,
}

impl KeyPressData {
    pub fn new(config: &Config, event_sink: ExtEventSink) -> Self {
        let (keymaps, command_keymaps) =
            Self::get_keymaps(config).unwrap_or((IndexMap::new(), IndexMap::new()));
        let mut keypress = Self {
            pending_keypress: Vec::new(),
            commands: Arc::new(lapce_internal_commands()),
            keymaps: Arc::new(keymaps),
            command_keymaps: Arc::new(command_keymaps),
            commands_with_keymap: Arc::new(Vec::new()),
            commands_without_keymap: Arc::new(Vec::new()),
            filter_pattern: "".to_string(),
            filtered_commands_with_keymap: Arc::new(Vec::new()),
            filtered_commands_without_keymap: Arc::new(Vec::new()),
            count: None,
            event_sink: Arc::new(event_sink),
        };
        keypress.load_commands();
        keypress
    }

    pub fn update_keymaps(&mut self, config: &Config) {
        if let Ok((new_keymaps, new_command_keymaps)) = Self::get_keymaps(config) {
            self.keymaps = Arc::new(new_keymaps);
            self.command_keymaps = Arc::new(new_command_keymaps);
            self.load_commands();
        }
    }

    fn load_commands(&mut self) {
        let mut commands_with_keymap = Vec::new();
        let mut commands_without_keymap = Vec::new();
        for (_, keymaps) in self.command_keymaps.iter() {
            for keymap in keymaps.iter() {
                if let Some(_cmd) = self.commands.get(&keymap.command) {
                    commands_with_keymap.push(keymap.clone());
                }
            }
        }

        for (_, cmd) in self.commands.iter() {
            if !self.command_keymaps.contains_key(&cmd.cmd) {
                commands_without_keymap.push(cmd.clone());
            }
        }

        self.commands_with_keymap = Arc::new(commands_with_keymap);
        self.commands_without_keymap = Arc::new(commands_without_keymap);
        if !self.filter_pattern.is_empty() {
            self.filter_commands(&self.filter_pattern.clone());
        }
    }

    fn run_command<T: KeyPressFocus>(
        &self,
        ctx: &mut EventCtx,
        command: &str,
        count: Option<usize>,
        mods: Modifiers,
        focus: &mut T,
        env: &Env,
    ) -> CommandExecuted {
        if let Some(cmd) = self.commands.get(command) {
            if let CommandTarget::Focus = cmd.target {
                if let Ok(cmd) = LapceCommand::from_str(command) {
                    focus.run_command(ctx, &cmd, count, mods, env)
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

        if let druid::KbKey::Character(c) = &keypress.key {
            if let Ok(n) = c.parse::<usize>() {
                if self.count.is_some() || n > 0 {
                    self.count = Some(self.count.unwrap_or(0) * 10 + n);
                    return true;
                }
            }
        }

        false
    }

    pub fn keypress(key_event: &KeyEvent) -> Option<KeyPress> {
        match key_event.key {
            druid::KbKey::Shift
            | KbKey::Meta
            | KbKey::Super
            | KbKey::Alt
            | KbKey::Control => return None,
            _ => (),
        }
        if key_event.key == druid::KbKey::Shift {
            let mut mods = key_event.mods;
            mods.set(Modifiers::SHIFT, false);
            if mods.is_empty() {
                return None;
            }
        }
        let mut mods = key_event.mods;
        if let druid::KbKey::Character(_c) = &key_event.key {
            mods.set(Modifiers::SHIFT, false);
        }

        let keypress = KeyPress {
            key: key_event.key.clone(),
            mods,
        };
        Some(keypress)
    }

    pub fn key_down<T: KeyPressFocus>(
        &mut self,
        ctx: &mut EventCtx,
        key_event: &KeyEvent,
        focus: &mut T,
        env: &Env,
    ) -> bool {
        if key_event.key == druid::KbKey::Shift {
            let mut mods = key_event.mods;
            mods.set(Modifiers::SHIFT, false);
            if mods.is_empty() {
                return false;
            }
        }
        let mut mods = key_event.mods;
        if let druid::KbKey::Character(_) = &key_event.key {
            mods.set(Modifiers::SHIFT, false);
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

        let keymatch = self.match_keymap(&keypresses, focus);
        match keymatch {
            KeymapMatch::Full(command) => {
                let count = self.count.take();
                self.run_command(ctx, &command, count, mods, focus, env);
                self.pending_keypress = Vec::new();
                return true;
            }
            KeymapMatch::Multiple(commands) => {
                self.pending_keypress = Vec::new();
                let count = self.count.take();
                for command in commands {
                    if self.run_command(ctx, &command, count, mods, focus, env)
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
                if focus.get_mode() == Mode::Insert {
                    let mut keypress = keypress.clone();
                    keypress.mods.set(Modifiers::SHIFT, false);
                    if let KeymapMatch::Full(command) =
                        self.match_keymap(&[keypress], focus)
                    {
                        if let Ok(cmd) = LapceCommand::from_str(&command) {
                            if cmd.move_command(None).is_some() {
                                focus.run_command(ctx, &cmd, None, mods, env);
                                return true;
                            }
                        }
                    }
                }
            }
        }

        if mode != Mode::Insert
            && mode != Mode::Terminal
            && !focus.expect_char()
            && self.handle_count(focus, &keypress)
        {
            return false;
        }

        self.count = None;

        let mut mods = keypress.mods;
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            if let druid::KbKey::Character(c) = &key_event.key {
                focus.receive_char(ctx, c);
                return true;
            }
        }
        false
    }

    fn match_keymap<T: KeyPressFocus>(
        &self,
        keypresses: &[KeyPress],
        check: &T,
    ) -> KeymapMatch {
        let matches = self
            .keymaps
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
                        if !keymap.modes.is_empty()
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
            .unwrap_or_else(Vec::new);
        let keymatch = if matches.is_empty() {
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
        keymatch
    }

    fn check_one_condition<T: KeyPressFocus>(
        &self,
        condition: &str,
        check: &T,
    ) -> bool {
        let condition = condition.trim();
        let (reverse, condition) =
            if let Some(stripped) = condition.strip_prefix('!') {
                (true, stripped)
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
                self.check_one_condition(condition, check)
            } else {
                self.check_one_condition(&condition[..or_indics[0].0], check)
                    || self.check_condition(&condition[or_indics[0].0 + 2..], check)
            }
        } else if or_indics.is_empty() {
            self.check_one_condition(&condition[..and_indics[0].0], check)
                && self.check_condition(&condition[and_indics[0].0 + 2..], check)
        } else if or_indics[0].0 < and_indics[0].0 {
            self.check_one_condition(&condition[..or_indics[0].0], check)
                || self.check_condition(&condition[or_indics[0].0 + 2..], check)
        } else {
            self.check_one_condition(&condition[..and_indics[0].0], check)
                && self.check_condition(&condition[and_indics[0].0 + 2..], check)
        }
    }
}

pub struct KeyMapLoader {
    keymaps: IndexMap<Vec<KeyPress>, Vec<KeyMap>>,
    command_keymaps: IndexMap<String, Vec<KeyMap>>,
}

impl KeyMapLoader {
    pub fn new() -> Self {
        Self {
            keymaps: Default::default(),
            command_keymaps: Default::default(),
        }
    }

    pub fn load_from_str<'a>(
        &'a mut self,
        s: &str,
        modal: bool,
    ) -> Result<&'a mut Self> {
        let toml_keymaps: toml::Value = toml::from_str(s)?;
        let toml_keymaps = toml_keymaps
            .get("keymaps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("no keymaps"))?;

        for toml_keymap in toml_keymaps {
            let keymap = match Self::get_keymap(toml_keymap, modal) {
                Ok(keymap) => keymap,
                Err(err) => {
                    log::error!("Could not parse keymap: {err}");
                    continue;
                }
            };

            let (command, bind) = match keymap.command.strip_prefix('-') {
                Some(cmd) => (cmd.to_string(), false),
                None => (keymap.command.clone(), true),
            };

            let current_keymaps = self.command_keymaps.entry(command).or_default();
            if bind {
                current_keymaps.push(keymap.clone());
                for i in 1..keymap.key.len() + 1 {
                    let key = keymap.key[..i].to_vec();
                    self.keymaps.entry(key).or_default().push(keymap.clone());
                }
            } else {
                let is_keymap = |k: &KeyMap| -> bool {
                    k.when == keymap.when
                        && k.modes == keymap.modes
                        && k.key == keymap.key
                };
                if let Some(index) = current_keymaps.iter().position(is_keymap) {
                    current_keymaps.remove(index);
                }
                for i in 1..keymap.key.len() + 1 {
                    if let Some(keymaps) = self.keymaps.get_mut(&keymap.key[..i]) {
                        if let Some(index) = keymaps.iter().position(is_keymap) {
                            keymaps.remove(index);
                        }
                    }
                }
            }
        }

        Ok(self)
    }

    pub fn finalize(
        self,
    ) -> (
        IndexMap<Vec<KeyPress>, Vec<KeyMap>>,
        IndexMap<String, Vec<KeyMap>>,
    ) {
        let Self {
            keymaps: map,
            command_keymaps: command_map,
        } = self;

        (map, command_map)
    }
}

impl KeyPressData {
    fn get_file_array() -> Option<toml::value::Array> {
        let path = Self::file()?;
        let content = std::fs::read(&path).ok()?;
        let toml_value: toml::Value = toml::from_slice(&content).ok()?;
        let table = toml_value.as_table()?;
        let array = table.get("keymaps")?.as_array()?.clone();
        Some(array)
    }

    pub fn filter_commands(&mut self, pattern: &str) {
        self.filter_pattern = pattern.to_string();
        let pattern = pattern.to_string();
        let commands_with_keymap = self.commands_with_keymap.clone();
        let commands_without_keymap = self.commands_without_keymap.clone();
        let commands = self.commands.clone();
        let event_sink = self.event_sink.clone();

        std::thread::spawn(move || {
            let matcher = SkimMatcherV2::default().ignore_case();

            let filtered_commands_with_keymap: Vec<KeyMap> = commands_with_keymap
                .iter()
                .filter_map(|i| {
                    let cmd = commands.get(&i.command).unwrap();
                    let text =
                        cmd.palette_desc.clone().unwrap_or_else(|| cmd.cmd.clone());
                    if let Some((score, _indices)) =
                        matcher.fuzzy_indices(&text, &pattern)
                    {
                        Some((i, score))
                    } else {
                        None
                    }
                })
                .sorted_by_key(|(_i, score)| -*score)
                .map(|(i, _)| i.clone())
                .collect();

            let filtered_commands_without_keymap: Vec<LapceCommandNew> =
                commands_without_keymap
                    .iter()
                    .filter_map(|i| {
                        let text =
                            i.palette_desc.clone().unwrap_or_else(|| i.cmd.clone());
                        if let Some((score, _indices)) =
                            matcher.fuzzy_indices(&text, &pattern)
                        {
                            Some((i, score))
                        } else {
                            None
                        }
                    })
                    .sorted_by_key(|(_i, score)| -*score)
                    .map(|(i, _)| i.clone())
                    .collect();

            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::FilterKeymaps(
                    pattern,
                    Arc::new(filtered_commands_with_keymap),
                    Arc::new(filtered_commands_without_keymap),
                ),
                Target::Auto,
            );
        });
    }

    pub fn update_file(keymap: &KeyMap, keys: &[KeyPress]) -> Option<()> {
        let mut array =
            Self::get_file_array().unwrap_or_else(toml::value::Array::new);
        if let Some(index) = array.iter().position(|value| {
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
        }) {
            if !keys.is_empty() {
                array[index].as_table_mut()?.insert(
                    "key".to_string(),
                    toml::Value::String(
                        keys.iter().map(|k| k.to_string()).join(" "),
                    ),
                );
            } else {
                array.remove(index);
            };
        } else {
            let mut table = toml::value::Table::new();
            table.insert(
                "command".to_string(),
                toml::Value::String(keymap.command.clone()),
            );
            if !keymap.modes.is_empty() {
                table.insert(
                    "mode".to_string(),
                    toml::Value::String(
                        keymap.modes.iter().map(|m| m.short()).join(""),
                    ),
                );
            }
            if let Some(when) = keymap.when.as_ref() {
                table.insert(
                    "when".to_string(),
                    toml::Value::String(when.to_string()),
                );
            }

            if !keys.is_empty() {
                table.insert(
                    "key".to_string(),
                    toml::Value::String(
                        keys.iter().map(|k| k.to_string()).join(" "),
                    ),
                );
                array.push(toml::Value::Table(table.clone()));
            }

            if !keymap.key.is_empty() {
                table.insert(
                    "key".to_string(),
                    toml::Value::String(
                        keymap.key.iter().map(|k| k.to_string()).join(" "),
                    ),
                );
                table.insert(
                    "command".to_string(),
                    toml::Value::String(format!("-{}", keymap.command)),
                );
                array.push(toml::Value::Table(table.clone()));
            }
        }

        let mut table = toml::value::Table::new();
        table.insert("keymaps".to_string(), toml::Value::Array(array));
        let value = toml::Value::Table(table);

        let path = Self::file()?;
        std::fs::write(&path, toml::to_string(&value).ok()?.as_bytes()).ok()?;
        None
    }

    pub fn file() -> Option<PathBuf> {
        let path = Config::dir().map(|d| {
            d.join(if !cfg!(debug_assertions) {
                "keymaps.toml"
            } else {
                "debug-keymaps.toml"
            })
        })?;

        if let Some(dir) = path.parent() {
            if !dir.exists() {
                let _ = std::fs::create_dir_all(dir);
            }
        }

        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }

        Some(path)
    }

    fn get_keymaps(
        config: &Config,
    ) -> Result<(
        IndexMap<Vec<KeyPress>, Vec<KeyMap>>,
        IndexMap<String, Vec<KeyMap>>,
    )> {
        let is_modal = config.lapce.modal;

        let mut loader = KeyMapLoader::new();

        if let Err(err) = loader.load_from_str(DEFAULT_KEYMAPS_COMMON, is_modal) {
            log::error!("Failed to load common defaults: {err}");
        }

        let os_keymaps = if std::env::consts::OS == "macos" {
            DEFAULT_KEYMAPS_MACOS
        } else if std::env::consts::OS == "linux" {
            DEFAULT_KEYMAPS_LINUX
        } else {
            DEFAULT_KEYMAPS_WINDOWS
        };

        if let Err(err) = loader.load_from_str(os_keymaps, is_modal) {
            log::error!("Failed to load OS defaults: {err}");
        }

        if let Some(path) = Self::file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Err(err) = loader.load_from_str(&content, is_modal) {
                    log::error!("Failed to load from {path:?}: {err}");
                }
            }
        }

        Ok(loader.finalize())
    }
}

impl KeyPress {
    /// Convert a piece of text representing a key in a keymaps file to the actual key
    /// Returns `None` if it was not able to find a key with that name
    fn map_str_to_key(key_part: &str) -> Option<druid::KbKey> {
        // Checks if it is a character key
        fn is_key_string(s: &str) -> bool {
            s.chars().all(|c| !c.is_control())
                && s.chars().skip(1).all(|c| !c.is_ascii())
        }

        // Import into scope to reduce noise
        use druid::keyboard_types::Key::*;
        Some(match key_part.to_lowercase().as_str() {
            s if is_key_string(s) => Character(key_part.to_string()),
            "unidentified" => Unidentified,
            "alt" => Alt,
            "altgraph" => AltGraph,
            "capslock" => CapsLock,
            "control" => Control,
            "fn" => Fn,
            "fnlock" => FnLock,
            "meta" => Meta,
            "numlock" => NumLock,
            "scrolllock" => ScrollLock,
            "shift" => Shift,
            "symbol" => Symbol,
            "symbollock" => SymbolLock,
            "hyper" => Hyper,
            "super" => Super,
            "enter" => Enter,
            "tab" => Tab,
            "arrowdown" => ArrowDown,
            "arrowleft" => ArrowLeft,
            "arrowright" => ArrowRight,
            "arrowup" => ArrowUp,
            "end" => End,
            "home" => Home,
            "pagedown" => PageDown,
            "pageup" => PageUp,
            "backspace" => Backspace,
            "clear" => Clear,
            "copy" => Copy,
            "crsel" => CrSel,
            "cut" => Cut,
            "delete" => Delete,
            "eraseeof" => EraseEof,
            "exsel" => ExSel,
            "insert" => Insert,
            "paste" => Paste,
            "redo" => Redo,
            "undo" => Undo,
            "accept" => Accept,
            "again" => Again,
            "attn" => Attn,
            "cancel" => Cancel,
            "contextmenu" => ContextMenu,
            "escape" => Escape,
            "execute" => Execute,
            "find" => Find,
            "help" => Help,
            "pause" => Pause,
            "play" => Play,
            "props" => Props,
            "select" => Select,
            "zoomin" => ZoomIn,
            "zoomout" => ZoomOut,
            "brightnessdown" => BrightnessDown,
            "brightnessup" => BrightnessUp,
            "eject" => Eject,
            "logoff" => LogOff,
            "power" => Power,
            "poweroff" => PowerOff,
            "printscreen" => PrintScreen,
            "hibernate" => Hibernate,
            "standby" => Standby,
            "wakeup" => WakeUp,
            "allcandidates" => AllCandidates,
            "alphanumeric" => Alphanumeric,
            "codeinput" => CodeInput,
            "compose" => Compose,
            "convert" => Convert,
            "dead" => Dead,
            "finalmode" => FinalMode,
            "groupfirst" => GroupFirst,
            "grouplast" => GroupLast,
            "groupnext" => GroupNext,
            "groupprevious" => GroupPrevious,
            "modechange" => ModeChange,
            "nextcandidate" => NextCandidate,
            "nonconvert" => NonConvert,
            "previouscandidate" => PreviousCandidate,
            "process" => Process,
            "singlecandidate" => SingleCandidate,
            "hangulmode" => HangulMode,
            "hanjamode" => HanjaMode,
            "junjamode" => JunjaMode,
            "eisu" => Eisu,
            "hankaku" => Hankaku,
            "hiragana" => Hiragana,
            "hiraganakatakana" => HiraganaKatakana,
            "kanamode" => KanaMode,
            "kanjimode" => KanjiMode,
            "katakana" => Katakana,
            "romaji" => Romaji,
            "zenkaku" => Zenkaku,
            "zenkakuhankaku" => ZenkakuHankaku,
            "f1" => F1,
            "f2" => F2,
            "f3" => F3,
            "f4" => F4,
            "f5" => F5,
            "f6" => F6,
            "f7" => F7,
            "f8" => F8,
            "f9" => F9,
            "f10" => F10,
            "f11" => F11,
            "f12" => F12,
            "soft1" => Soft1,
            "soft2" => Soft2,
            "soft3" => Soft3,
            "soft4" => Soft4,
            "channeldown" => ChannelDown,
            "channelup" => ChannelUp,
            "close" => Close,
            "mailforward" => MailForward,
            "mailreply" => MailReply,
            "mailsend" => MailSend,
            "mediaclose" => MediaClose,
            "mediafastforward" => MediaFastForward,
            "mediapause" => MediaPause,
            "mediaplay" => MediaPlay,
            "mediaplaypause" => MediaPlayPause,
            "mediarecord" => MediaRecord,
            "mediarewind" => MediaRewind,
            "mediastop" => MediaStop,
            "mediatracknext" => MediaTrackNext,
            "mediatrackprevious" => MediaTrackPrevious,
            "new" => New,
            "open" => Open,
            "print" => Print,
            "save" => Save,
            "spellcheck" => SpellCheck,
            "key11" => Key11,
            "key12" => Key12,
            "audiobalanceleft" => AudioBalanceLeft,
            "audiobalanceright" => AudioBalanceRight,
            "audiobassboostdown" => AudioBassBoostDown,
            "audiobassboosttoggle" => AudioBassBoostToggle,
            "audiobassboostup" => AudioBassBoostUp,
            "audiofaderfront" => AudioFaderFront,
            "audiofaderrear" => AudioFaderRear,
            "audiosurroundmodenext" => AudioSurroundModeNext,
            "audiotrebledown" => AudioTrebleDown,
            "audiotrebleup" => AudioTrebleUp,
            "audiovolumedown" => AudioVolumeDown,
            "audiovolumeup" => AudioVolumeUp,
            "audiovolumemute" => AudioVolumeMute,
            "microphonetoggle" => MicrophoneToggle,
            "microphonevolumedown" => MicrophoneVolumeDown,
            "microphonevolumeup" => MicrophoneVolumeUp,
            "microphonevolumemute" => MicrophoneVolumeMute,
            "speechcorrectionlist" => SpeechCorrectionList,
            "speechinputtoggle" => SpeechInputToggle,
            "launchapplication1" => LaunchApplication1,
            "launchapplication2" => LaunchApplication2,
            "launchcalendar" => LaunchCalendar,
            "launchcontacts" => LaunchContacts,
            "launchmail" => LaunchMail,
            "launchmediaplayer" => LaunchMediaPlayer,
            "launchmusicplayer" => LaunchMusicPlayer,
            "launchphone" => LaunchPhone,
            "launchscreensaver" => LaunchScreenSaver,
            "launchspreadsheet" => LaunchSpreadsheet,
            "launchwebbrowser" => LaunchWebBrowser,
            "launchwebcam" => LaunchWebCam,
            "launchwordprocessor" => LaunchWordProcessor,
            "browserback" => BrowserBack,
            "browserfavorites" => BrowserFavorites,
            "browserforward" => BrowserForward,
            "browserhome" => BrowserHome,
            "browserrefresh" => BrowserRefresh,
            "browsersearch" => BrowserSearch,
            "browserstop" => BrowserStop,
            "appswitch" => AppSwitch,
            "call" => Call,
            "camera" => Camera,
            "camerafocus" => CameraFocus,
            "endcall" => EndCall,
            "goback" => GoBack,
            "gohome" => GoHome,
            "headsethook" => HeadsetHook,
            "lastnumberredial" => LastNumberRedial,
            "notification" => Notification,
            "mannermode" => MannerMode,
            "voicedial" => VoiceDial,
            "tv" => TV,
            "tv3dmode" => TV3DMode,
            "tvantennacable" => TVAntennaCable,
            "tvaudiodescription" => TVAudioDescription,
            "tvaudiodescriptionmixdown" => TVAudioDescriptionMixDown,
            "tvaudiodescriptionmixup" => TVAudioDescriptionMixUp,
            "tvcontentsmenu" => TVContentsMenu,
            "tvdataservice" => TVDataService,
            "tvinput" => TVInput,
            "tvinputcomponent1" => TVInputComponent1,
            "tvinputcomponent2" => TVInputComponent2,
            "tvinputcomposite1" => TVInputComposite1,
            "tvinputcomposite2" => TVInputComposite2,
            "tvinputhdmi1" => TVInputHDMI1,
            "tvinputhdmi2" => TVInputHDMI2,
            "tvinputhdmi3" => TVInputHDMI3,
            "tvinputhdmi4" => TVInputHDMI4,
            "tvinputvga1" => TVInputVGA1,
            "tvmediacontext" => TVMediaContext,
            "tvnetwork" => TVNetwork,
            "tvnumberentry" => TVNumberEntry,
            "tvpower" => TVPower,
            "tvradioservice" => TVRadioService,
            "tvsatellite" => TVSatellite,
            "tvsatellitebs" => TVSatelliteBS,
            "tvsatellitecs" => TVSatelliteCS,
            "tvsatellitetoggle" => TVSatelliteToggle,
            "tvterrestrialanalog" => TVTerrestrialAnalog,
            "tvterrestrialdigital" => TVTerrestrialDigital,
            "tvtimer" => TVTimer,
            "avrinput" => AVRInput,
            "avrpower" => AVRPower,
            "colorf0red" => ColorF0Red,
            "colorf1green" => ColorF1Green,
            "colorf2yellow" => ColorF2Yellow,
            "colorf3blue" => ColorF3Blue,
            "colorf4grey" => ColorF4Grey,
            "colorf5brown" => ColorF5Brown,
            "closedcaptiontoggle" => ClosedCaptionToggle,
            "dimmer" => Dimmer,
            "displayswap" => DisplaySwap,
            "dvr" => DVR,
            "exit" => Exit,
            "favoriteclear0" => FavoriteClear0,
            "favoriteclear1" => FavoriteClear1,
            "favoriteclear2" => FavoriteClear2,
            "favoriteclear3" => FavoriteClear3,
            "favoriterecall0" => FavoriteRecall0,
            "favoriterecall1" => FavoriteRecall1,
            "favoriterecall2" => FavoriteRecall2,
            "favoriterecall3" => FavoriteRecall3,
            "favoritestore0" => FavoriteStore0,
            "favoritestore1" => FavoriteStore1,
            "favoritestore2" => FavoriteStore2,
            "favoritestore3" => FavoriteStore3,
            "guide" => Guide,
            "guidenextday" => GuideNextDay,
            "guidepreviousday" => GuidePreviousDay,
            "info" => Info,
            "instantreplay" => InstantReplay,
            "link" => Link,
            "listprogram" => ListProgram,
            "livecontent" => LiveContent,
            "lock" => Lock,
            "mediaapps" => MediaApps,
            "mediaaudiotrack" => MediaAudioTrack,
            "medialast" => MediaLast,
            "mediaskipbackward" => MediaSkipBackward,
            "mediaskipforward" => MediaSkipForward,
            "mediastepbackward" => MediaStepBackward,
            "mediastepforward" => MediaStepForward,
            "mediatopmenu" => MediaTopMenu,
            "navigatein" => NavigateIn,
            "navigatenext" => NavigateNext,
            "navigateout" => NavigateOut,
            "navigateprevious" => NavigatePrevious,
            "nextfavoritechannel" => NextFavoriteChannel,
            "nextuserprofile" => NextUserProfile,
            "ondemand" => OnDemand,
            "pairing" => Pairing,
            "pinpdown" => PinPDown,
            "pinpmove" => PinPMove,
            "pinptoggle" => PinPToggle,
            "pinpup" => PinPUp,
            "playspeeddown" => PlaySpeedDown,
            "playspeedreset" => PlaySpeedReset,
            "playspeedup" => PlaySpeedUp,
            "randomtoggle" => RandomToggle,
            "rclowbattery" => RcLowBattery,
            "recordspeednext" => RecordSpeedNext,
            "rfbypass" => RfBypass,
            "scanchannelstoggle" => ScanChannelsToggle,
            "screenmodenext" => ScreenModeNext,
            "settings" => Settings,
            "splitscreentoggle" => SplitScreenToggle,
            "stbinput" => STBInput,
            "stbpower" => STBPower,
            "subtitle" => Subtitle,
            "teletext" => Teletext,
            "videomodenext" => VideoModeNext,
            "wink" => Wink,
            "zoomtoggle" => ZoomToggle,

            // Custom key name mappings
            "esc" => Escape,
            "space" => Character(" ".to_string()),
            "bs" => Backspace,
            "up" => ArrowUp,
            "down" => ArrowDown,
            "right" => ArrowRight,
            "left" => ArrowLeft,
            "del" => Delete,

            _ => return None,
        })
    }
}

impl KeyPress {
    pub fn parse(key: &str) -> Vec<Self> {
        key.split(' ')
            .filter_map(|k| {
                let (modifiers, key) = match k.rsplit_once('+') {
                    Some(pair) => pair,
                    None => ("", k),
                };

                let key = match Self::map_str_to_key(key) {
                    Some(key) => key,
                    None => {
                        // Skip past unrecognized key definitions
                        log::warn!("Unrecognized key: {key}");
                        return None;
                    }
                };

                let mut mods = Modifiers::default();
                for part in modifiers.split('+') {
                    match part.to_lowercase().as_ref() {
                        "ctrl" => mods.set(Modifiers::CONTROL, true),
                        "meta" => mods.set(Modifiers::META, true),
                        "shift" => mods.set(Modifiers::SHIFT, true),
                        "alt" => mods.set(Modifiers::ALT, true),
                        _ => (),
                    }
                }

                Some(KeyPress { mods, key })
            })
            .collect()
    }
}

impl KeyMapLoader {
    fn get_keymap(toml_keymap: &toml::Value, modal: bool) -> Result<KeyMap> {
        let key = toml_keymap
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("no key in keymap"))?;

        let modes = get_modes(toml_keymap);
        if !modal && !modes.is_empty() && !modes.contains(&Mode::Insert) {
            return Err(anyhow!(""));
        }

        Ok(KeyMap {
            key: KeyPress::parse(key),
            modes: get_modes(toml_keymap),
            when: toml_keymap
                .get("when")
                .and_then(|w| w.as_str())
                .map(|w| w.to_string()),
            command: toml_keymap
                .get("command")
                .and_then(|c| c.as_str())
                .map(|w| w.trim().to_string())
                .unwrap_or_else(|| "".to_string()),
        })
    }
}

fn get_modes(toml_keymap: &toml::Value) -> Vec<Mode> {
    let mut modes = toml_keymap
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
        .unwrap_or_else(Vec::new);
    modes.sort();
    modes
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
    { key = "End", command = "line_end", when = "n" },
    { key = "I", command = "insert_first_non_blank", when = "n" },
]
        "###;
        let mut loader = KeyMapLoader::new();
        loader.load_from_str(keymaps, true).unwrap();

        let (keymaps, _) = loader.finalize();

        // Lower case modifiers
        let keypress = KeyPress::parse("ctrl+w");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 4);

        let keypress = KeyPress::parse("ctrl+w l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 2);

        let keypress = KeyPress::parse("ctrl+w h");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("ctrl+w l l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("end");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        // Upper case modifiers
        let keypress = KeyPress::parse("Ctrl+w");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 4);

        let keypress = KeyPress::parse("Ctrl+w l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 2);

        let keypress = KeyPress::parse("Ctrl+w h");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("Ctrl+w l l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("End");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        // No modifier
        let keypress = KeyPress::parse("I");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);
    }
}

pub struct DefaultKeyPressHandler {}

impl KeyPressFocus for DefaultKeyPressHandler {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, _condition: &str) -> bool {
        false
    }

    fn run_command(
        &mut self,
        _ctx: &mut EventCtx,
        _command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}
}
