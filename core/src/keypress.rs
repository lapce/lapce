use std::fmt::Display;
use std::path::PathBuf;
use std::slice::SliceIndex;
use std::str::FromStr;
use std::{collections::HashMap, io::Read};
use std::{fs::File, sync::Arc};

use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use druid::piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder};
use druid::{
    Color, Data, Env, EventCtx, ExtEventSink, FontFamily, KeyEvent, Modifiers,
    PaintCtx, Point, Rect, RenderContext, Size, Target, WidgetId, WindowId,
};
use druid::{Command, KbKey};
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

impl Display for KeyPress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mods.ctrl() {
            f.write_str("Ctrl+");
        }
        if self.mods.alt() {
            f.write_str("Alt+");
        }
        if self.mods.meta() {
            f.write_str("Meta+");
        }
        if self.mods.shift() {
            f.write_str("Shift+");
        }
        f.write_str(&self.key.to_string())
    }
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

    pub fn paint(
        &self,
        ctx: &mut PaintCtx,
        origin: Point,
        config: &Config,
    ) -> (Point, Vec<(Option<Rect>, PietTextLayout, Point)>) {
        let mut origin = origin.clone();
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
                if c.to_string() == c.to_uppercase()
                    && c.to_lowercase() != c.to_uppercase()
                {
                    if !self.mods.shift() {
                        keys.push("Shift".to_string());
                    }
                }
                keys.push(c.to_uppercase());
            }
            _ => {
                keys.push(self.key.to_string());
            }
        }

        let old_origin = origin.clone();

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
                    .new_text_layout("+".to_string())
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

impl KeyMap {
    pub fn paint(
        &self,
        ctx: &mut PaintCtx,
        origin: Point,
        center: bool,
        config: &Config,
    ) {
        let old_origin = origin.clone();

        let mut origin = origin.clone();
        let mut items = Vec::new();
        for keypress in self.key.iter() {
            let (new_origin, mut new_items) = keypress.paint(ctx, origin, config);
            origin = new_origin + (10.0, 0.0);
            items.append(&mut new_items);
        }

        let x_shift = if center {
            (origin.x - old_origin.x) / 2.0
        } else {
            0.0
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
                if let Some(cmd) = self.commands.get(&keymap.command) {
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
        if self.filter_pattern != "" {
            self.filter_commands(&self.filter_pattern.clone());
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
            let mut mods = key_event.mods.clone();
            mods.set(Modifiers::SHIFT, false);
            if mods.is_empty() {
                return None;
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

    fn keymaps_from_str(
        s: &str,
        modal: bool,
    ) -> Result<(
        IndexMap<Vec<KeyPress>, Vec<KeyMap>>,
        IndexMap<String, Vec<KeyMap>>,
    )> {
        let toml_keymaps: toml::Value = toml::from_str(s)?;
        let toml_keymaps = toml_keymaps
            .get("keymaps")
            .and_then(|v| v.as_array())
            .ok_or(anyhow!("no keymaps"))?;

        let mut keymaps: IndexMap<Vec<KeyPress>, Vec<KeyMap>> = IndexMap::new();
        let mut command_keymaps: IndexMap<String, Vec<KeyMap>> = IndexMap::new();
        for toml_keymap in toml_keymaps {
            if let Ok(keymap) = Self::get_keymap(toml_keymap, modal) {
                let mut command = keymap.command.clone();
                let mut bind = true;
                if command.starts_with("-") {
                    command = command[1..].to_string();
                    bind = false;
                }
                if !command_keymaps.contains_key(&command) {
                    command_keymaps.insert(command.clone(), vec![]);
                }
                let current_keymaps = command_keymaps.get_mut(&command).unwrap();
                if bind {
                    current_keymaps.push(keymap.clone());
                    for i in 1..keymap.key.len() + 1 {
                        let key = keymap.key[..i].to_vec();
                        match keymaps.get_mut(&key) {
                            Some(keymaps) => keymaps.push(keymap.clone()),
                            None => {
                                keymaps.insert(key, vec![keymap.clone()]);
                            }
                        }
                    }
                } else {
                    if let Some(index) = current_keymaps.iter().position(|k| {
                        k.when == keymap.when
                            && k.modes == keymap.modes
                            && k.key == keymap.key
                    }) {
                        current_keymaps.remove(index);
                    }
                    for i in 1..keymap.key.len() + 1 {
                        let key = keymap.key[..i].to_vec();
                        if let Some(keymaps) = keymaps.get_mut(&key) {
                            if let Some(index) = keymaps.iter().position(|k| {
                                k.when == keymap.when
                                    && k.modes == keymap.modes
                                    && k.key == keymap.key
                            }) {
                                keymaps.remove(index);
                            }
                        }
                    }
                }
            }
        }

        Ok((keymaps, command_keymaps))
    }

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
                    if let Some((score, mut indices)) =
                        matcher.fuzzy_indices(&text, &pattern)
                    {
                        Some((i, score))
                    } else {
                        None
                    }
                })
                .sorted_by_key(|(i, score)| -*score)
                .map(|(i, _)| i.clone())
                .collect();

            let filtered_commands_without_keymap: Vec<LapceCommandNew> =
                commands_without_keymap
                    .iter()
                    .filter_map(|i| {
                        let text =
                            i.palette_desc.clone().unwrap_or_else(|| i.cmd.clone());
                        if let Some((score, mut indices)) =
                            matcher.fuzzy_indices(&text, &pattern)
                        {
                            Some((i, score))
                        } else {
                            None
                        }
                    })
                    .sorted_by_key(|(i, score)| -*score)
                    .map(|(i, _)| i.clone())
                    .collect();
            event_sink.submit_command(
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

    pub fn update_file(keymap: &KeyMap, keys: &Vec<KeyPress>) -> Option<()> {
        let mut array =
            Self::get_file_array().unwrap_or_else(|| toml::value::Array::new());
        if let Some(index) = array.iter().position(|value| {
            Some(keymap.command.as_str())
                == value.get("command").and_then(|c| c.as_str())
                && keymap.when.as_ref().map(|w| w.as_str())
                    == value.get("when").and_then(|w| w.as_str())
                && keymap.modes == Self::get_modes(value)
                && Some(keymap.key.clone())
                    == value
                        .get("key")
                        .and_then(|v| v.as_str())
                        .map(|s| Self::get_keypress(s))
        }) {
            if keys.len() > 0 {
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
            if keymap.modes.len() > 0 {
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

            if keys.len() > 0 {
                table.insert(
                    "key".to_string(),
                    toml::Value::String(
                        keys.iter().map(|k| k.to_string()).join(" "),
                    ),
                );
                array.push(toml::Value::Table(table.clone()));
            }

            if keymap.key.len() > 0 {
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
                std::fs::create_dir_all(dir);
            }
        }

        if !path.exists() {
            std::fs::OpenOptions::new()
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
        let mut keymaps_str = if std::env::consts::OS == "macos" {
            default_keymaps_macos
        } else if std::env::consts::OS == "linux" {
            default_keymaps_linux
        } else {
            default_keymaps_windows
        }
        .to_string();

        if let Some(path) = Self::file() {
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

        Self::keymaps_from_str(&keymaps_str, config.lapce.modal)
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

    fn get_keymap(toml_keymap: &toml::Value, modal: bool) -> Result<KeyMap> {
        let key = toml_keymap
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or(anyhow!("no key in keymap"))?;

        let modes = Self::get_modes(toml_keymap);
        if !modal {
            if modes.len() > 0 && !modes.contains(&Mode::Insert) {
                return Err(anyhow!(""));
            }
        }

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
            .unwrap_or(Vec::new());
        modes.sort();
        modes
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
        let (keymaps, _) = KeyPressData::keymaps_from_str(keymaps, true).unwrap();
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
