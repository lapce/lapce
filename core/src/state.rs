use crate::{
    buffer::Buffer,
    buffer::BufferId,
    buffer::BufferUIState,
    command::LapceUICommand,
    command::LAPCE_UI_COMMAND,
    command::{LapceCommand, LAPCE_COMMAND},
    editor::EditorSplitState,
    editor::EditorUIState,
    editor::HighlightTextLayout,
    explorer::FileExplorerState,
    keypress::KeyPressState,
    language::TreeSitter,
    palette::PaletteState,
    plugin::PluginCatalog,
};
use anyhow::{anyhow, Result};
use druid::{
    Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Modifiers, Target, WidgetId,
};
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::{
    collections::HashMap, fs::File, io::Read, path::PathBuf, str::FromStr,
    sync::Arc, thread,
};
use toml;

lazy_static! {
    pub static ref LAPCE_STATE: LapceState = LapceState::new();
}

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
pub struct LapceUIState {
    pub focus: LapceFocus,
    pub buffers: Arc<HashMap<BufferId, Arc<BufferUIState>>>,
    pub editors: Arc<HashMap<WidgetId, EditorUIState>>,
}

impl Data for LapceUIState {
    fn same(&self, other: &Self) -> bool {
        self.focus == other.focus
            && self.buffers.same(&other.buffers)
            && self.editors.same(&other.editors)
    }
}

impl LapceUIState {
    pub fn new() -> LapceUIState {
        let active = LAPCE_STATE.editor_split.lock().active;
        let editor_ui_state = EditorUIState::new();
        let mut editors = HashMap::new();
        editors.insert(active, editor_ui_state);
        LapceUIState {
            buffers: Arc::new(HashMap::new()),
            focus: LapceFocus::Editor,
            editors: Arc::new(editors),
        }
    }

    pub fn get_buffer_mut(&mut self, buffer_id: &BufferId) -> &mut BufferUIState {
        Arc::make_mut(Arc::make_mut(&mut self.buffers).get_mut(buffer_id).unwrap())
    }

    pub fn get_buffer(&self, buffer_id: &BufferId) -> &BufferUIState {
        self.buffers.get(buffer_id).unwrap()
    }

    pub fn new_editor(&mut self, editor_id: &WidgetId) {
        let editor_ui_state = EditorUIState::new();
        Arc::make_mut(&mut self.editors).insert(editor_id.clone(), editor_ui_state);
    }

    pub fn get_editor_mut(&mut self, view_id: &WidgetId) -> &mut EditorUIState {
        Arc::make_mut(&mut self.editors).get_mut(view_id).unwrap()
    }

    pub fn get_editor(&self, view_id: &WidgetId) -> &EditorUIState {
        self.editors.get(view_id).unwrap()
    }
}

#[derive(Clone)]
pub struct LapceState {
    pub palette: Arc<Mutex<PaletteState>>,
    pub keypress: Arc<Mutex<KeyPressState>>,
    pub theme: HashMap<String, Color>,
    pub focus: Arc<Mutex<LapceFocus>>,
    pub editor_split: Arc<Mutex<EditorSplitState>>,
    pub container: Option<WidgetId>,
    pub file_explorer: Arc<Mutex<FileExplorerState>>,
    pub plugins: Arc<Mutex<PluginCatalog>>,
    pub ui_sink: Arc<Mutex<Option<ExtEventSink>>>,
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
        let mut plugins = PluginCatalog::new();
        plugins.reload_from_paths(&[PathBuf::from_str("./lsp").unwrap()]);
        plugins.start_all();
        LapceState {
            theme: Self::get_theme().unwrap_or(HashMap::new()),
            focus: Arc::new(Mutex::new(LapceFocus::Editor)),
            palette: Arc::new(Mutex::new(PaletteState::new())),
            editor_split: Arc::new(Mutex::new(EditorSplitState::new())),
            file_explorer: Arc::new(Mutex::new(FileExplorerState::new())),
            container: None,
            keypress: Arc::new(Mutex::new(KeyPressState::new())),
            plugins: Arc::new(Mutex::new(plugins)),
            ui_sink: Arc::new(Mutex::new(None)),
        }
    }

    fn get_theme() -> Result<HashMap<String, Color>> {
        let mut f = File::open("/Users/Lulu/lapce/.lapce/theme.toml")?;
        let mut content = vec![];
        f.read_to_end(&mut content)?;
        let toml_theme: HashMap<String, String> = toml::from_slice(&content)?;

        let mut theme = HashMap::new();
        for (name, hex) in toml_theme.iter() {
            if let Ok(color) = hex_to_color(hex) {
                theme.insert(name.to_string(), color);
            }
        }
        Ok(theme)
    }

    pub fn get_mode(&self) -> Mode {
        match *self.focus.lock() {
            LapceFocus::Palette => Mode::Insert,
            LapceFocus::Editor => self.editor_split.lock().get_mode(),
            LapceFocus::FileExplorer => Mode::Normal,
        }
    }

    pub fn set_ui_sink(&self, ui_event_sink: ExtEventSink) {
        *self.ui_sink.lock() = Some(ui_event_sink);
    }

    pub fn submit_ui_command(&self, comand: LapceUICommand, widget_id: WidgetId) {
        self.ui_sink.lock().as_ref().unwrap().submit_command(
            LAPCE_UI_COMMAND,
            comand,
            Target::Widget(widget_id),
        );
    }

    pub fn insert(
        &self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        content: &str,
        env: &Env,
    ) {
        match *self.focus.lock() {
            LapceFocus::Palette => {
                self.palette.lock().insert(ctx, content, env);
            }
            LapceFocus::Editor => {
                self.editor_split.lock().insert(ctx, ui_state, content, env);
            }
            _ => (),
        }
        // ctx.request_layout();
    }

    pub fn run_command(
        &self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        count: Option<usize>,
        command: &str,
        env: &Env,
    ) -> Result<()> {
        let cmd = LapceCommand::from_str(command)?;
        match cmd {
            LapceCommand::Palette => {
                *self.focus.lock() = LapceFocus::Palette;
                self.palette.lock().run();
            }
            LapceCommand::PaletteCancel => {
                *self.focus.lock() = LapceFocus::Editor;
                self.palette.lock().cancel();
            }
            LapceCommand::FileExplorer => {
                *self.focus.lock() = LapceFocus::FileExplorer;
            }
            LapceCommand::FileExplorerCancel => {
                *self.focus.lock() = LapceFocus::Editor;
            }
            _ => {
                let mut focus = self.focus.lock();
                match *focus {
                    LapceFocus::FileExplorer => {
                        *focus = self
                            .file_explorer
                            .lock()
                            .run_command(ctx, ui_state, count, cmd);
                    }
                    LapceFocus::Editor => {
                        self.editor_split
                            .lock()
                            .run_command(ctx, ui_state, count, cmd, env);
                    }
                    LapceFocus::Palette => {
                        let mut palette = self.palette.lock();
                        match cmd {
                            LapceCommand::ListSelect => {
                                palette.select(ctx, ui_state);
                                *focus = LapceFocus::Editor;
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
                                palette.delete_to_beginning_of_line(ctx, env);
                            }
                            _ => (),
                        };
                    }
                };
            }
        };
        ui_state.focus = self.focus.lock().clone();
        // ctx.request_layout();
        Ok(())
    }

    pub fn check_condition(&self, condition: &str) -> bool {
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
                    return self.check_one_condition(&condition[..or_indics[0].0])
                        || self.check_condition(&condition[or_indics[0].0 + 2..]);
                } else {
                    return self.check_one_condition(&condition[..and_indics[0].0])
                        && self.check_condition(&condition[and_indics[0].0 + 2..]);
                }
            }
        }
    }

    fn check_one_condition(&self, condition: &str) -> bool {
        let focus = self.focus.lock();
        match condition.trim() {
            "file_explorer_focus" => *focus == LapceFocus::FileExplorer,
            "palette_focus" => *focus == LapceFocus::Palette,
            "list_focus" => {
                *focus == LapceFocus::Palette
                    || *focus == LapceFocus::FileExplorer
                    || (*focus == LapceFocus::Editor
                        && self.editor_split.lock().completion.len() > 0)
            }
            "editor_operator" => {
                *focus == LapceFocus::Editor
                    && self.editor_split.lock().has_operator()
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
