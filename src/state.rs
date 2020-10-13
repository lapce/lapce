use std::{collections::HashMap, fs::File, io::Read, str::FromStr, sync::Arc};

use anyhow::{anyhow, Result};
use druid::{
    Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Modifiers, Target,
    WidgetId,
};
use toml;

use crate::{
    buffer::Buffer,
    buffer::BufferId,
    buffer::BufferUIState,
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    command::{LapceCommand, LAPCE_COMMAND},
    editor::EditorSplitState,
    explorer::FileExplorerState,
    language::TreeSitter,
    palette::PaletteState,
};

#[derive(PartialEq)]
enum KeymapMatch {
    Full,
    Prefix,
}

#[derive(Clone, PartialEq)]
pub enum LapceFocus {
    Palette,
    Editor,
    FileExplorer,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum VisualMode {
    Normal,
    Linewise,
    Blockwise,
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
    pub palette: Arc<PaletteState>,
    pending_keypress: Vec<KeyPress>,
    count: Option<usize>,
    keymaps: Vec<KeyMap>,
    pub theme: HashMap<String, Color>,
    pub last_focus: LapceFocus,
    pub focus: LapceFocus,
    // pub ui_sink: ExtEventSink,
    pub editor_split: Arc<EditorSplitState>,
    pub container: Option<WidgetId>,
    pub file_explorer: Arc<FileExplorerState>,
}

impl Data for LapceState {
    fn same(&self, other: &Self) -> bool {
        self.editor_split.same(&other.editor_split)
            && self.palette.same(&other.palette)
            && self.file_explorer.same(&other.file_explorer)
    }
}

impl LapceState {
    pub fn new() -> LapceState {
        LapceState {
            pending_keypress: Vec::new(),
            keymaps: Self::get_keymaps().unwrap_or(Vec::new()),
            theme: Self::get_theme().unwrap_or(HashMap::new()),
            count: None,
            focus: LapceFocus::Editor,
            last_focus: LapceFocus::Editor,
            palette: Arc::new(PaletteState::new()),
            editor_split: Arc::new(EditorSplitState::new()),
            file_explorer: Arc::new(FileExplorerState::new()),
            container: None,
        }
    }

    fn get_theme() -> Result<HashMap<String, Color>> {
        let mut f = File::open("/Users/Lulu/lapce/.lapce/theme.toml")?;
        let mut content = vec![];
        f.read_to_end(&mut content)?;
        let toml_theme: HashMap<String, String> = toml::from_slice(&content)?;

        let mut theme = HashMap::new();
        for (name, hex) in toml_theme.iter() {
            println!("{}", name);
            if let Ok(color) = hex_to_color(hex) {
                theme.insert(name.to_string(), color);
            }
        }
        Ok(theme)
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
        match self.focus {
            LapceFocus::Palette => Mode::Insert,
            LapceFocus::Editor => self.editor_split.get_mode(),
            LapceFocus::FileExplorer => Mode::Normal,
        }
    }

    pub fn insert(&mut self, ctx: &mut EventCtx, content: &str, env: &Env) {
        match self.focus {
            LapceFocus::Palette => {
                let palette = Arc::make_mut(&mut self.palette);
                palette.insert(ctx, content, env);
            }
            LapceFocus::Editor => {
                let editor_split = Arc::make_mut(&mut self.editor_split);
                editor_split.insert(ctx, content, env);
            }
            _ => (),
        }
    }

    pub fn handle_count(&mut self, keypress: &KeyPress) -> bool {
        if self.get_mode() == Mode::Insert {
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

    pub fn get_count(&mut self) -> Option<usize> {
        self.count.take()
    }

    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &str,
        env: &Env,
    ) {
        let count = self.get_count();
        if let Ok(cmd) = LapceCommand::from_str(command) {
            match cmd {
                LapceCommand::Palette => {
                    self.focus = LapceFocus::Palette;
                    let palette = Arc::make_mut(&mut self.palette);
                    palette.run();
                }
                LapceCommand::PaletteCancel => {
                    self.focus = LapceFocus::Editor;
                    let palette = Arc::make_mut(&mut self.palette);
                    palette.cancel();
                }
                LapceCommand::FileExplorer => {
                    self.focus = LapceFocus::FileExplorer;
                }
                LapceCommand::FileExplorerCancel => {
                    self.focus = LapceFocus::Editor;
                }
                _ => {
                    match self.focus {
                        LapceFocus::FileExplorer => {
                            let file_explorer =
                                Arc::make_mut(&mut self.file_explorer);
                            let editor_split =
                                Arc::make_mut(&mut self.editor_split);
                            self.focus = file_explorer.run_command(
                                ctx,
                                editor_split,
                                count,
                                cmd,
                            );
                        }
                        LapceFocus::Editor => {
                            let editor_split =
                                Arc::make_mut(&mut self.editor_split);
                            editor_split.run_command(ctx, count, cmd, env);
                        }
                        LapceFocus::Palette => {
                            let palette = Arc::make_mut(&mut self.palette);
                            let editor_split =
                                Arc::make_mut(&mut self.editor_split);
                            match cmd {
                                LapceCommand::ListSelect => {
                                    palette.select(ctx, editor_split);
                                    self.focus = LapceFocus::Editor;
                                }
                                LapceCommand::ListNext => {
                                    palette.change_index(ctx, 1, env);
                                }
                                LapceCommand::ListPrevious => {
                                    palette.change_index(ctx, -1, env);
                                }
                                LapceCommand::Left => {
                                    palette.move_cursor(-1);
                                }
                                LapceCommand::Right => {
                                    palette.move_cursor(1);
                                }
                                LapceCommand::DeleteBackward => {
                                    palette.delete_backward(ctx, env);
                                }
                                LapceCommand::DeleteToBeginningOfLine => {
                                    palette
                                        .delete_to_beginning_of_line(ctx, env);
                                }
                                _ => (),
                            };
                        }
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

    pub fn key_down(
        &mut self,
        ctx: &mut EventCtx,
        key_event: &KeyEvent,
        env: &Env,
    ) {
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
        let mut keypresses = self.pending_keypress.clone();
        keypresses.push(keypress.clone());
        for keymap in self.keymaps.iter() {
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
                        self.pending_keypress.push(keypress.clone());
                        return;
                    }
                }
            }
        }

        let pending_keypresses = self.pending_keypress.clone();
        self.pending_keypress = Vec::new();

        if let Some(keymap) = full_match_keymap {
            self.run_command(ctx, &keymap.command, env);
            return;
        }

        if pending_keypresses.len() > 0 {
            let mut full_match_keymap = None;
            for keymap in self.keymaps.iter() {
                if let Some(match_result) =
                    self.match_keymap_new(&pending_keypresses, keymap)
                {
                    if match_result == KeymapMatch::Full {
                        if full_match_keymap.is_none() {
                            full_match_keymap = Some(keymap.clone());
                        }
                    }
                }
            }
            if let Some(keymap) = full_match_keymap {
                self.run_command(ctx, &keymap.command, env);
                self.key_down(ctx, key_event, env);
                return;
            }
        }

        if self.get_mode() != Mode::Insert {
            self.handle_count(&keypress);
            return;
        }

        self.count = None;

        if mods.is_empty() {
            match &key_event.key {
                druid::keyboard_types::Key::Character(c) => {
                    self.insert(ctx, c, env);
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
            "file_explorer_focus" => self.focus == LapceFocus::FileExplorer,
            "palette_focus" => self.focus == LapceFocus::Palette,
            "list_focus" => {
                self.focus == LapceFocus::Palette
                    || self.focus == LapceFocus::FileExplorer
            }
            "editor_operator" => {
                self.focus == LapceFocus::Editor
                    && self.editor_split.has_operator()
            }
            _ => false,
        }
    }

    pub fn container_id(&self) -> WidgetId {
        self.container.unwrap().clone()
    }

    pub fn set_container(&mut self, container: WidgetId) {
        self.container = Some(container);
    }

    pub fn open_file(&mut self, ctx: &mut EventCtx, path: &str) {
        let editor_split = Arc::make_mut(&mut self.editor_split);
        editor_split.open_file(ctx, path);
    }

    // pub fn submit_ui_command(&self, cmd: LapceUICommand, widget_id: WidgetId) {
    //     self.ui_sink.submit_command(
    //         LAPCE_UI_COMMAND,
    //         cmd,
    //         Target::Widget(widget_id),
    //     );
    // }
}

pub fn hex_to_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches("#");
    let (r, g, b, a) = match hex.len() {
        3 => (
            format!("{}{}", &hex[0..0], &hex[0..0]),
            format!("{}{}", &hex[1..1], &hex[1..1]),
            format!("{}{}", &hex[2..2], &hex[2..2]),
            "ff".to_string(),
        ),
        6 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            "ff".to_string(),
        ),
        8 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            hex[6..8].to_string(),
        ),
        _ => return Err(anyhow!("invalid hex color")),
    };
    println!(
        "{} {} {}",
        u8::from_str_radix(&r, 16)?,
        u8::from_str_radix(&g, 16)?,
        u8::from_str_radix(&b, 16)?
    );
    Ok(Color::rgb8(
        u8::from_str_radix(&r, 16)?,
        u8::from_str_radix(&g, 16)?,
        u8::from_str_radix(&b, 16)?,
        // u8::from_str_radix(&a, 16)?,
    ))
}

#[cfg(test)]
mod tests {
    use xi_rope::Rope;

    use super::*;

    #[test]
    fn test_check_condition() {
        // let rope = Rope::from_str("abc\nabc\n").unwrap();
        // assert_eq!(rope.len(), 9);
        // assert_eq!(rope.offset_of_line(1), 1);
        // assert_eq!(rope.line_of_offset(rope.len()), 9);
    }
}
