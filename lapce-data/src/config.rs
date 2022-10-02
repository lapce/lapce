use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use druid::{
    piet::{PietText, Text, TextLayout, TextLayoutBuilder},
    Color, ExtEventSink, FontFamily, Size, Target,
};
use indexmap::IndexMap;
pub use lapce_proxy::APPLICATION_NAME;
use lapce_proxy::{directory::Directory, plugin::wasi::find_all_volts};
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use structdesc::FieldNames;
use thiserror::Error;
use toml_edit::easy as toml;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceWorkspace, LapceWorkspaceType},
};

pub const LOGO: &str = include_str!("../../extra/images/logo.svg");
const DEFAULT_SETTINGS: &str = include_str!("../../defaults/settings.toml");
const DEFAULT_LIGHT_THEME: &str = include_str!("../../defaults/light-theme.toml");
const DEFAULT_DARK_THEME: &str = include_str!("../../defaults/dark-theme.toml");
static DEFAULT_CONFIG: Lazy<config::Config> = Lazy::new(LapceConfig::default_config);
static DEFAULT_LAPCE_CONFIG: Lazy<LapceConfig> =
    Lazy::new(LapceConfig::default_lapce_config);

pub struct LapceTheme {}

impl LapceTheme {
    pub const LAPCE_WARN: &str = "lapce.warn";
    pub const LAPCE_ERROR: &str = "lapce.error";
    pub const LAPCE_ACTIVE_TAB: &str = "lapce.active_tab";
    pub const LAPCE_INACTIVE_TAB: &str = "lapce.inactive_tab";
    pub const LAPCE_DROPDOWN_SHADOW: &str = "lapce.dropdown_shadow";
    pub const LAPCE_BORDER: &str = "lapce.border";
    pub const LAPCE_SCROLL_BAR: &str = "lapce.scroll_bar";

    pub const EDITOR_BACKGROUND: &str = "editor.background";
    pub const EDITOR_FOREGROUND: &str = "editor.foreground";
    pub const EDITOR_DIM: &str = "editor.dim";
    pub const EDITOR_FOCUS: &str = "editor.focus";
    pub const EDITOR_CARET: &str = "editor.caret";
    pub const EDITOR_SELECTION: &str = "editor.selection";
    pub const EDITOR_CURRENT_LINE: &str = "editor.current_line";
    pub const EDITOR_LINK: &str = "editor.link";
    pub const EDITOR_VISIBLE_WHITESPACE: &str = "editor.visible_whitespace";

    pub const INLAY_HINT_FOREGROUND: &str = "inlay_hint.foreground";
    pub const INLAY_HINT_BACKGROUND: &str = "inlay_hint.background";

    pub const ERROR_LENS_ERROR_FOREGROUND: &str = "error_lens.error.foreground";
    pub const ERROR_LENS_ERROR_BACKGROUND: &str = "error_lens.error.background";
    pub const ERROR_LENS_WARNING_FOREGROUND: &str = "error_lens.warning.foreground";
    pub const ERROR_LENS_WARNING_BACKGROUND: &str = "error_lens.warning.background";
    pub const ERROR_LENS_OTHER_FOREGROUND: &str = "error_lens.other.foreground";
    pub const ERROR_LENS_OTHER_BACKGROUND: &str = "error_lens.other.background";

    pub const SOURCE_CONTROL_ADDED: &str = "source_control.added";
    pub const SOURCE_CONTROL_REMOVED: &str = "source_control.removed";
    pub const SOURCE_CONTROL_MODIFIED: &str = "source_control.modified";

    pub const TERMINAL_CURSOR: &str = "terminal.cursor";
    pub const TERMINAL_BACKGROUND: &str = "terminal.background";
    pub const TERMINAL_FOREGROUND: &str = "terminal.foreground";
    pub const TERMINAL_RED: &str = "terminal.red";
    pub const TERMINAL_BLUE: &str = "terminal.blue";
    pub const TERMINAL_GREEN: &str = "terminal.green";
    pub const TERMINAL_YELLOW: &str = "terminal.yellow";
    pub const TERMINAL_BLACK: &str = "terminal.black";
    pub const TERMINAL_WHITE: &str = "terminal.white";
    pub const TERMINAL_CYAN: &str = "terminal.cyan";
    pub const TERMINAL_MAGENTA: &str = "terminal.magenta";

    pub const TERMINAL_BRIGHT_RED: &str = "terminal.bright_red";
    pub const TERMINAL_BRIGHT_BLUE: &str = "terminal.bright_blue";
    pub const TERMINAL_BRIGHT_GREEN: &str = "terminal.bright_green";
    pub const TERMINAL_BRIGHT_YELLOW: &str = "terminal.bright_yellow";
    pub const TERMINAL_BRIGHT_BLACK: &str = "terminal.bright_black";
    pub const TERMINAL_BRIGHT_WHITE: &str = "terminal.bright_white";
    pub const TERMINAL_BRIGHT_CYAN: &str = "terminal.bright_cyan";
    pub const TERMINAL_BRIGHT_MAGENTA: &str = "terminal.bright_magenta";

    pub const PALETTE_BACKGROUND: &str = "palette.background";
    pub const PALETTE_CURRENT: &str = "palette.current";

    pub const COMPLETION_BACKGROUND: &str = "completion.background";
    pub const COMPLETION_CURRENT: &str = "completion.current";

    pub const HOVER_BACKGROUND: &str = "hover.background";

    pub const ACTIVITY_BACKGROUND: &str = "activity.background";
    pub const ACTIVITY_CURRENT: &str = "activity.current";

    pub const PANEL_BACKGROUND: &str = "panel.background";
    pub const PANEL_CURRENT: &str = "panel.current";
    pub const PANEL_HOVERED: &str = "panel.hovered";

    pub const STATUS_BACKGROUND: &str = "status.background";
    pub const STATUS_MODAL_NORMAL: &str = "status.modal.normal";
    pub const STATUS_MODAL_INSERT: &str = "status.modal.insert";
    pub const STATUS_MODAL_VISUAL: &str = "status.modal.visual";
    pub const STATUS_MODAL_TERMINAL: &str = "status.modal.terminal";

    pub const PALETTE_INPUT_LINE_HEIGHT: druid::Key<f64> =
        druid::Key::new("lapce.palette_input_line_height");
    pub const PALETTE_INPUT_LINE_PADDING: druid::Key<f64> =
        druid::Key::new("lapce.palette_input_line_padding");
    pub const INPUT_LINE_HEIGHT: druid::Key<f64> =
        druid::Key::new("lapce.input_line_height");
    pub const INPUT_LINE_PADDING: druid::Key<f64> =
        druid::Key::new("lapce.input_line_padding");
    pub const INPUT_FONT_SIZE: druid::Key<u64> =
        druid::Key::new("lapce.input_font_size");

    pub const MARKDOWN_BLOCKQUOTE: &'static str = "markdown.blockquote";
}

#[derive(Error, Debug)]
pub enum LoadThemeError {
    #[error("themes folder not found, possibly it could not be created")]
    ThemesFolderNotFound,
    #[error("theme file ({theme_name}.toml) was not found in {themes_folder:?}")]
    FileNotFound {
        themes_folder: PathBuf,
        theme_name: String,
    },
    #[error("There was an error reading the theme file")]
    Read(std::io::Error),
}

pub trait GetConfig {
    fn get_config(&self) -> &LapceConfig;
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct CoreConfig {
    #[field_names(desc = "Enable modal editing (Vim like)")]
    pub modal: bool,
    #[field_names(desc = "Set the color theme of Lapce")]
    pub color_theme: String,
    #[field_names(
        desc = "Enable customised titlebar and disable OS native one (Linux, BSD, Windows)"
    )]
    pub custom_titlebar: bool,
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct EditorConfig {
    #[field_names(desc = "Set the editor font family")]
    pub font_family: String,
    #[field_names(desc = "Set the editor font size")]
    pub font_size: usize,
    #[field_names(desc = "Set the font size in the code lens")]
    pub code_lens_font_size: usize,
    #[field_names(
        desc = "Set the editor line height. If less than 5.0, line height will be a multiple of the font size."
    )]
    line_height: f64,
    #[field_names(desc = "Set the tab width")]
    pub tab_width: usize,
    #[field_names(desc = "If opened editors are shown in a tab")]
    pub show_tab: bool,
    #[field_names(desc = "If navigation breadcrumbs are shown for the file")]
    pub show_bread_crumbs: bool,
    #[field_names(desc = "If the editor can scroll beyond the last line")]
    pub scroll_beyond_last_line: bool,
    #[field_names(
        desc = "Show code context like functions and classes at the top of editor when scroll"
    )]
    pub sticky_header: bool,
    #[field_names(
        desc = "If the editor should show the documentation of the current completion item"
    )]
    pub completion_show_documentation: bool,
    #[field_names(
        desc = "Whether the editor should disable automatic closing of matching pairs"
    )]
    pub auto_closing_matching_pairs: bool,
    #[field_names(
        desc = "How long (in ms) it should take before the hover information appears"
    )]
    pub hover_delay: u64,
    #[field_names(
        desc = "If modal mode should have relative line numbers (though, not in insert mode)"
    )]
    pub modal_mode_relative_line_numbers: bool,
    #[field_names(
        desc = "Whether it should format the document on save (if there is an available formatter)"
    )]
    pub format_on_save: bool,
    #[field_names(desc = "If inlay hints should be displayed")]
    pub enable_inlay_hints: bool,
    #[field_names(
        desc = "Set the inlay hint font family. If empty, it uses the editor font family."
    )]
    pub inlay_hint_font_family: String,
    #[field_names(
        desc = "Set the inlay hint font size. If less than 5 or greater than editor font size, it uses the editor font size."
    )]
    pub inlay_hint_font_size: usize,
    #[field_names(desc = "If diagnostics should be displayed inline")]
    pub enable_error_lens: bool,
    #[field_names(
        desc = "Whether error lens should go to the end of view line, or only to the end of the diagnostic"
    )]
    pub error_lens_end_of_line: bool,
    #[field_names(
        desc = "Set error lens font family. If empty, it uses the inlay hint font family."
    )]
    pub error_lens_font_family: String,
    #[field_names(
        desc = "Set the error lens font size. If 0 it uses the inlay hint font size."
    )]
    pub error_lens_font_size: usize,
    #[field_names(
        desc = "Set the cursor blink interval (in milliseconds). Set to 0 to completely disable."
    )]
    pub blink_interval: u64, // TODO: change to u128 when upgrading config-rs to >0.11
    #[field_names(
        desc = "Whether the multiple cursor selection is case sensitive."
    )]
    pub multicursor_case_sensitive: bool,
    #[field_names(
        desc = "Whether the multiple cursor selection only selects whole words."
    )]
    pub multicursor_whole_words: bool,
    #[field_names(
        desc = "How the editor should render whitespace characters.\nOptions: none, all, boundary, trailing."
    )]
    pub render_whitespace: String,
    #[field_names(
        desc = "Set the auto save delay (in milliseconds), Set to 0 to completely disable"
    )]
    pub autosave_interval: u64,
}

impl EditorConfig {
    pub fn line_height(&self) -> usize {
        const SCALE_OR_SIZE_LIMIT: f64 = 5.0;

        let line_height = if self.line_height < SCALE_OR_SIZE_LIMIT {
            self.line_height * self.font_size as f64
        } else {
            self.line_height
        };

        // Prevent overlapping lines
        (line_height.round() as usize).max(self.font_size)
    }

    pub fn font_family(&self) -> FontFamily {
        if self.font_family.is_empty() {
            FontFamily::SYSTEM_UI
        } else {
            FontFamily::new_unchecked(self.font_family.clone())
        }
    }

    pub fn inlay_hint_font_family(&self) -> FontFamily {
        if self.inlay_hint_font_family.is_empty() {
            self.font_family()
        } else {
            FontFamily::new_unchecked(self.inlay_hint_font_family.clone())
        }
    }

    pub fn inlay_hint_font_size(&self) -> usize {
        if self.inlay_hint_font_size < 5
            || self.inlay_hint_font_size > self.font_size
        {
            self.font_size
        } else {
            self.inlay_hint_font_size
        }
    }

    pub fn error_lens_font_family(&self) -> FontFamily {
        if self.error_lens_font_family.is_empty() {
            self.inlay_hint_font_family()
        } else {
            FontFamily::new_unchecked(self.error_lens_font_family.clone())
        }
    }

    pub fn error_lens_font_size(&self) -> usize {
        if self.error_lens_font_size == 0 {
            self.inlay_hint_font_size()
        } else {
            self.error_lens_font_size
        }
    }
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct UIConfig {
    #[field_names(
        desc = "Set the UI font family. If empty, it uses system default."
    )]
    font_family: String,

    #[field_names(desc = "Set the UI base font size")]
    font_size: usize,

    #[field_names(
        desc = "Set the header height for panel header and editor tab header"
    )]
    header_height: usize,

    #[field_names(desc = "Set the height for status line")]
    status_height: usize,

    #[field_names(desc = "Set the minimum width for editor tab")]
    tab_min_width: usize,

    #[field_names(desc = "Set the width for scroll bar")]
    scroll_width: usize,

    #[field_names(desc = "Controls the width of drop shadow in the UI")]
    drop_shadow_width: usize,

    #[field_names(desc = "Controls the width of the preview editor")]
    preview_editor_width: usize,

    #[field_names(
        desc = "Set the hover font family. If empty, it uses the UI font family"
    )]
    hover_font_family: String,
    #[field_names(desc = "Set the hover font size. If 0, uses the UI font size")]
    hover_font_size: usize,
}

impl UIConfig {
    pub fn font_family(&self) -> FontFamily {
        if self.font_family.is_empty() {
            FontFamily::SYSTEM_UI
        } else {
            FontFamily::new_unchecked(self.font_family.as_str())
        }
    }

    pub fn font_size(&self) -> usize {
        self.font_size.max(6).min(32)
    }

    pub fn header_height(&self) -> usize {
        let font_size = self.font_size();
        self.header_height.max(font_size)
    }

    pub fn status_height(&self) -> usize {
        let font_size = self.font_size();
        self.status_height.max(font_size)
    }

    pub fn tab_min_width(&self) -> usize {
        self.tab_min_width
    }

    pub fn scroll_width(&self) -> usize {
        self.scroll_width
    }

    pub fn drop_shadow_width(&self) -> usize {
        self.drop_shadow_width
    }

    pub fn preview_editor_width(&self) -> usize {
        self.preview_editor_width
    }

    pub fn hover_font_family(&self) -> FontFamily {
        if self.hover_font_family.is_empty() {
            self.font_family()
        } else {
            FontFamily::new_unchecked(self.hover_font_family.as_str())
        }
    }

    pub fn hover_font_size(&self) -> usize {
        if self.hover_font_size == 0 {
            self.font_size()
        } else {
            self.hover_font_size
        }
    }
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct TerminalConfig {
    #[field_names(
        desc = "Set the terminal font family. If empty, it uses editor font family."
    )]
    pub font_family: String,
    #[field_names(
        desc = "Set the terminal font size, If 0, it uses editor font size."
    )]
    pub font_size: usize,
    #[field_names(
        desc = "Set the terminal line height, If 0, it uses editor line height"
    )]
    pub line_height: usize,
    #[field_names(desc = "Set the terminal Shell")]
    pub shell: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub base: ThemeBaseConfig,
    pub syntax: IndexMap<String, String>,
    pub ui: IndexMap<String, String>,
}

impl ThemeConfig {
    fn resolve_color(
        colors: &IndexMap<String, String>,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        colors
            .iter()
            .map(|(name, hex)| {
                let color = if let Some(stripped) = hex.strip_prefix('$') {
                    base.get(stripped).cloned()
                } else {
                    Color::from_hex_str(hex).ok()
                };

                let color = color
                    .or_else(|| {
                        default.and_then(|default| default.get(name).cloned())
                    })
                    .unwrap_or(Color::rgb8(0, 0, 0));

                (name.to_string(), color)
            })
            .collect()
    }

    fn resolve_ui_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.ui, base, default)
    }

    fn resolve_syntax_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.syntax, base, default)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ThemeBaseConfig {
    pub white: String,
    pub black: String,
    pub grey: String,
    pub blue: String,
    pub red: String,
    pub yellow: String,
    pub orange: String,
    pub green: String,
    pub purple: String,
    pub cyan: String,
    pub magenta: String,
}

impl ThemeBaseConfig {
    pub fn resolve(&self, default: Option<&ThemeBaseColor>) -> ThemeBaseColor {
        let default = default.cloned().unwrap_or_default();
        ThemeBaseColor {
            white: Color::from_hex_str(&self.white).unwrap_or(default.white),
            black: Color::from_hex_str(&self.black).unwrap_or(default.black),
            grey: Color::from_hex_str(&self.grey).unwrap_or(default.grey),
            blue: Color::from_hex_str(&self.blue).unwrap_or(default.blue),
            red: Color::from_hex_str(&self.red).unwrap_or(default.red),
            yellow: Color::from_hex_str(&self.yellow).unwrap_or(default.yellow),
            orange: Color::from_hex_str(&self.orange).unwrap_or(default.orange),
            green: Color::from_hex_str(&self.green).unwrap_or(default.green),
            purple: Color::from_hex_str(&self.purple).unwrap_or(default.purple),
            cyan: Color::from_hex_str(&self.cyan).unwrap_or(default.cyan),
            magenta: Color::from_hex_str(&self.magenta).unwrap_or(default.magenta),
        }
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        Some(match name {
            "white" => &self.white,
            "black" => &self.black,
            "grey" => &self.grey,
            "blue" => &self.blue,
            "red" => &self.red,
            "yellow" => &self.yellow,
            "orange" => &self.orange,
            "green" => &self.green,
            "purple" => &self.purple,
            "cyan" => &self.cyan,
            "magenta" => &self.magenta,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ThemeColor {
    pub base: ThemeBaseColor,
    pub syntax: HashMap<String, Color>,
    pub ui: HashMap<String, Color>,
}

#[derive(Debug, Clone)]
pub struct ThemeBaseColor {
    pub white: Color,
    pub black: Color,
    pub grey: Color,
    pub blue: Color,
    pub red: Color,
    pub yellow: Color,
    pub orange: Color,
    pub green: Color,
    pub purple: Color,
    pub cyan: Color,
    pub magenta: Color,
}

impl Default for ThemeBaseColor {
    fn default() -> Self {
        Self {
            white: Color::rgb8(0, 0, 0),
            black: Color::rgb8(0, 0, 0),
            grey: Color::rgb8(0, 0, 0),
            blue: Color::rgb8(0, 0, 0),
            red: Color::rgb8(0, 0, 0),
            yellow: Color::rgb8(0, 0, 0),
            orange: Color::rgb8(0, 0, 0),
            green: Color::rgb8(0, 0, 0),
            purple: Color::rgb8(0, 0, 0),
            cyan: Color::rgb8(0, 0, 0),
            magenta: Color::rgb8(0, 0, 0),
        }
    }
}

impl ThemeBaseColor {
    fn get(&self, name: &str) -> Option<&Color> {
        Some(match name {
            "white" => &self.white,
            "black" => &self.black,
            "grey" => &self.grey,
            "blue" => &self.blue,
            "red" => &self.red,
            "yellow" => &self.yellow,
            "orange" => &self.orange,
            "green" => &self.green,
            "purple" => &self.purple,
            "cyan" => &self.cyan,
            "magenta" => &self.magenta,
            _ => return None,
        })
    }

    pub fn keys(&self) -> [&'static str; 11] {
        [
            "white", "black", "grey", "blue", "red", "yellow", "orange", "green",
            "purple", "cyan", "magenta",
        ]
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LapceConfig {
    #[serde(skip)]
    pub id: u64,
    pub core: CoreConfig,
    pub ui: UIConfig,
    pub editor: EditorConfig,
    pub terminal: TerminalConfig,
    pub theme: ThemeConfig,
    #[serde(flatten)]
    pub plugins: HashMap<String, serde_json::Value>,
    #[serde(skip)]
    pub default_theme: ThemeConfig,
    #[serde(skip)]
    pub color: ThemeColor,
    #[serde(skip)]
    pub available_themes: HashMap<String, (String, config::Config)>,
    #[serde(skip)]
    tab_layout_info: Arc<RwLock<HashMap<(FontFamily, usize), f64>>>,
}

pub struct ConfigWatcher {
    event_sink: ExtEventSink,
    delay_handler: Arc<Mutex<Option<()>>>,
}

impl ConfigWatcher {
    pub fn new(event_sink: ExtEventSink) -> Self {
        Self {
            event_sink,
            delay_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl notify::EventHandler for ConfigWatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        if let Ok(event) = event {
            match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    *self.delay_handler.lock() = Some(());
                    let delay_handler = self.delay_handler.clone();
                    let event_sink = self.event_sink.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        if delay_handler.lock().take().is_some() {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::ReloadConfig,
                                Target::Auto,
                            );
                        }
                    });
                }
                _ => (),
            }
        }
    }
}

impl LapceConfig {
    pub fn load(workspace: &LapceWorkspace, disabled_volts: &[String]) -> Self {
        let config = Self::merge_config(workspace, None);
        let mut lapce_config: LapceConfig = config
            .try_deserialize()
            .unwrap_or_else(|_| DEFAULT_LAPCE_CONFIG.clone());
        let available_themes = Self::load_themes(disabled_volts);
        lapce_config.available_themes = available_themes;
        lapce_config.resolve_theme(workspace);
        lapce_config
    }

    fn resolve_theme(&mut self, workspace: &LapceWorkspace) {
        let mut default_lapce_config = DEFAULT_LAPCE_CONFIG.clone();
        if let Some((_, theme_config)) = self
            .available_themes
            .get(&self.core.color_theme.to_lowercase())
        {
            if let Ok(mut theme_lapce_config) = config::Config::builder()
                .add_source(DEFAULT_CONFIG.clone())
                .add_source(theme_config.clone())
                .build()
                .and_then(|theme| theme.try_deserialize::<LapceConfig>())
            {
                theme_lapce_config.resolve_colors(Some(&default_lapce_config));
                default_lapce_config = theme_lapce_config;
            }
            if let Ok(new) =
                Self::merge_config(workspace, Some(theme_config.clone()))
                    .try_deserialize::<LapceConfig>()
            {
                self.core = new.core;
                self.ui = new.ui;
                self.editor = new.editor;
                self.terminal = new.terminal;
                self.theme = new.theme;
                self.plugins = new.plugins;
            }
        }
        self.resolve_colors(Some(&default_lapce_config));
        self.default_theme = default_lapce_config.theme.clone();
        self.update_id();
    }

    fn merge_config(
        workspace: &LapceWorkspace,
        theme_config: Option<config::Config>,
    ) -> config::Config {
        let mut config = DEFAULT_CONFIG.clone();
        if let Some(theme) = theme_config {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(path) = Self::settings_file() {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(config::File::from(path.as_path()).required(false))
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        match workspace.kind {
            LapceWorkspaceType::Local => {
                if let Some(path) = workspace.path.as_ref() {
                    let path = path.join("./.lapce/settings.toml");
                    config = config::Config::builder()
                        .add_source(config.clone())
                        .add_source(
                            config::File::from(path.as_path()).required(false),
                        )
                        .build()
                        .unwrap_or_else(|_| config.clone());
                }
            }
            LapceWorkspaceType::RemoteSSH(_, _) => {}
            LapceWorkspaceType::RemoteWSL => {}
        }

        config
    }

    fn resolve_colors(&mut self, default_config: Option<&LapceConfig>) {
        self.color.base = self
            .theme
            .base
            .resolve(default_config.map(|c| &c.color.base));
        self.color.ui = self
            .theme
            .resolve_ui_color(&self.color.base, default_config.map(|c| &c.color.ui));
        self.color.syntax = self.theme.resolve_syntax_color(
            &self.color.base,
            default_config.map(|c| &c.color.syntax),
        );
    }

    fn load_themes(
        disabled_volts: &[String],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes = Self::load_local_themes().unwrap_or_default();
        if let Some(plugin_themes) = Self::load_plugin_themes(disabled_volts) {
            for (key, theme) in plugin_themes.into_iter() {
                themes.insert(key, theme);
            }
        }

        let (name, theme) = Self::load_theme_from_str(DEFAULT_LIGHT_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));
        let (name, theme) = Self::load_theme_from_str(DEFAULT_DARK_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));

        themes
    }

    fn load_local_themes() -> Option<HashMap<String, (String, config::Config)>> {
        let themes_folder = Directory::themes_directory()?;
        let themes: HashMap<String, (String, config::Config)> =
            std::fs::read_dir(themes_folder)
                .ok()?
                .filter_map(|entry| {
                    entry.ok().and_then(|entry| Self::load_theme(&entry.path()))
                })
                .collect();
        Some(themes)
    }

    fn load_plugin_themes(
        disabled_volts: &[String],
    ) -> Option<HashMap<String, (String, config::Config)>> {
        let mut themes: HashMap<String, (String, config::Config)> = HashMap::new();
        for meta in find_all_volts() {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Some((key, theme)) =
                        Self::load_theme(&PathBuf::from(theme_path))
                    {
                        themes.insert(key, theme);
                    }
                }
            }
        }
        Some(themes)
    }

    fn load_theme_from_str(s: &str) -> Option<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()
            .ok()?;
        let table = config.get_table("theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, config))
    }

    fn load_theme(path: &Path) -> Option<(String, (String, config::Config))> {
        if !path.is_file() {
            return None;
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .ok()?;
        let table = config.get_table("theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name.to_lowercase(), (name, config)))
    }

    fn default_config() -> config::Config {
        config::Config::builder()
            .add_source(config::File::from_str(
                DEFAULT_SETTINGS,
                config::FileFormat::Toml,
            ))
            .build()
            .unwrap()
    }

    fn default_lapce_config() -> LapceConfig {
        let mut default_lapce_config: LapceConfig =
            DEFAULT_CONFIG.clone().try_deserialize().unwrap();
        default_lapce_config.resolve_colors(None);
        default_lapce_config
    }

    pub fn export_theme(&self) -> String {
        let mut table = toml::value::Table::new();
        let mut theme = self.theme.clone();
        theme.name = "".to_string();
        theme.syntax.sort_keys();
        theme.ui.sort_keys();
        table.insert("theme".to_string(), toml::Value::try_from(&theme).unwrap());
        table.insert("ui".to_string(), toml::Value::try_from(&self.ui).unwrap());
        let value = toml::Value::Table(table);
        toml::to_string_pretty(&value).unwrap()
    }

    pub fn keymaps_file() -> Option<PathBuf> {
        let path = Directory::config_directory()?.join("keymaps.toml");

        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }

        Some(path)
    }

    pub fn log_file() -> Option<PathBuf> {
        let time = chrono::Local::now().format("%Y%m%d-%H%M%S");

        let file_name = format!("{time}.log");

        let path = Directory::logs_directory()?.join(file_name);

        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }

        Some(path)
    }

    pub fn settings_file() -> Option<PathBuf> {
        let path = Directory::config_directory()?.join("settings.toml");

        if !path.exists() {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }

        Some(path)
    }

    fn get_file_table() -> Option<toml_edit::Document> {
        let path = Self::settings_file()?;
        let content = std::fs::read_to_string(path).ok()?;
        let document: toml_edit::Document = content.parse().ok()?;
        Some(document)
    }

    pub fn reset_setting(parent: &str, key: &str) -> Option<()> {
        let mut main_table = Self::get_file_table().unwrap_or_default();

        // Find the container table
        let mut table = main_table.as_table_mut();
        for key in parent.split('.') {
            if !table.contains_key(key) {
                table.insert(
                    key,
                    toml_edit::Item::Table(toml_edit::Table::default()),
                );
            }
            table = table.get_mut(key)?.as_table_mut()?;
        }

        table.remove(key);

        // Store
        let path = Self::settings_file()?;
        std::fs::write(&path, main_table.to_string().as_bytes()).ok()?;

        Some(())
    }

    pub fn update_file(
        parent: &str,
        key: &str,
        value: toml_edit::Value,
    ) -> Option<()> {
        let mut main_table = Self::get_file_table().unwrap_or_default();

        // Find the container table
        let mut table = main_table.as_table_mut();
        for key in parent.split('.') {
            if !table.contains_key(key) {
                table.insert(
                    key,
                    toml_edit::Item::Table(toml_edit::Table::default()),
                );
            }
            table = table.get_mut(key)?.as_table_mut()?;
        }

        // Update key
        table.insert(key, toml_edit::Item::Value(value));

        // Store
        let path = Self::settings_file()?;
        std::fs::write(&path, main_table.to_string().as_bytes()).ok()?;

        Some(())
    }

    fn update_id(&mut self) {
        self.id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    pub fn set_theme(
        &mut self,
        workspace: &LapceWorkspace,
        theme: &str,
        preview: bool,
    ) {
        self.core.color_theme = theme.to_string();
        self.resolve_theme(workspace);
        if !preview {
            LapceConfig::update_file(
                "core",
                "color-theme",
                toml_edit::Value::from(theme),
            );
        }
    }

    /// Get the color by the name from the current theme if it exists
    /// Otherwise, get the color from the base them
    /// # Panics
    /// If the color was not able to be found in either theme, which may be indicative that
    /// it is misspelled or needs to be added to the base-theme.
    pub fn get_color_unchecked(&self, name: &str) -> &Color {
        self.color
            .ui
            .get(name)
            .unwrap_or_else(|| panic!("Key not found: {name}"))
    }

    /// Retrieve a color value whose key starts with "style."
    pub fn get_style_color(&self, name: &str) -> Option<&Color> {
        self.color.syntax.get(name)
    }

    /// Calculate the width of the character "W" (being the widest character)
    /// in the editor's current font family at the specified font size.
    pub fn char_width(&self, text: &mut PietText, font_size: f64) -> f64 {
        Self::editor_text_size_internal(
            self.editor.font_family(),
            font_size,
            text,
            "W",
        )
        .width
    }

    pub fn terminal_font_family(&self) -> FontFamily {
        if self.terminal.font_family.is_empty() {
            self.editor.font_family()
        } else {
            FontFamily::new_unchecked(self.terminal.font_family.clone())
        }
    }

    pub fn terminal_font_size(&self) -> usize {
        if self.terminal.font_size > 0 {
            self.terminal.font_size
        } else {
            self.editor.font_size
        }
    }

    pub fn terminal_line_height(&self) -> usize {
        if self.terminal.line_height > 0 {
            self.terminal.line_height
        } else {
            self.editor.line_height()
        }
    }

    /// Calculate the width of the character "W" (being the widest character)
    /// in the editor's current font family and current font size.
    pub fn editor_char_width(&self, text: &mut PietText) -> f64 {
        self.char_width(text, self.editor.font_size as f64)
    }

    /// Calculate the width of `text_to_measure` in the editor's current font family and font size.
    pub fn editor_text_width(
        &self,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> f64 {
        Self::editor_text_size_internal(
            self.editor.font_family(),
            self.editor.font_size as f64,
            text,
            text_to_measure,
        )
        .width
    }

    /// Calculate the size of `text_to_measure` in the editor's current font family and font size.
    pub fn editor_text_size(
        &self,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> Size {
        Self::editor_text_size_internal(
            self.editor.font_family(),
            self.editor.font_size as f64,
            text,
            text_to_measure,
        )
    }

    /// Efficiently calculate the size of a piece of text, without allocating.
    /// This function should not be made public, use one of the public wrapper functions instead.
    fn editor_text_size_internal(
        font_family: FontFamily,
        font_size: f64,
        text: &mut PietText,
        text_to_measure: &str,
    ) -> Size {
        // Lie about the lifetime of `text_to_measure`.
        //
        // The method `new_text_layout` will take ownership of its parameter
        // and hold it inside the `text_layout` object. It will normally only do this efficiently
        // for `&'static str`, and not other `&'a str`. It can safely do this for static strings
        // because they are known to live for the lifetime of the program.
        //
        // `new_text_layout` can also work by taking ownership of an owned type such
        // as String, Rc, or Arc. But they all require allocation. We want to measure
        // `strs` with arbitrary lifetimes. If we 'cheat' by extending the lifetime of the
        // `text_to_measure` (in this function only) then we can safely call `new_text_layout`
        // because the `text_layout` value that is produced is local to this function and hence
        // always dropped inside this function, and hence its lifetime is always strictly less
        // than the lifetime of `text_to_measure`, irrespective of whether `text_to_measure`
        // is actually static or not.
        //
        // Note that this technique also assumes that `new_text_layout` does not stash
        // its parameter away somewhere, such as a global cache. If it did, this would
        // break and we would have to go back to calling `to_string` on the parameter.
        let static_str: &'static str =
            unsafe { std::mem::transmute(text_to_measure) };

        let text_layout = text
            .new_text_layout(static_str)
            .font(font_family, font_size)
            .build()
            .unwrap();

        text_layout.size()
    }

    pub fn update_recent_workspaces(workspaces: Vec<LapceWorkspace>) -> Option<()> {
        let path = Self::recent_workspaces_file()?;
        let mut array = toml::value::Array::new();
        for workspace in workspaces {
            if let Some(path) = workspace.path.as_ref() {
                let mut table = toml::value::Table::new();
                table.insert(
                    "kind".to_string(),
                    toml::Value::String(match workspace.kind {
                        LapceWorkspaceType::Local => "local".to_string(),
                        LapceWorkspaceType::RemoteSSH(user, host) => {
                            format!("ssh://{}@{}", user, host)
                        }
                        LapceWorkspaceType::RemoteWSL => "wsl".to_string(),
                    }),
                );
                table.insert(
                    "path".to_string(),
                    toml::Value::String(path.to_str()?.to_string()),
                );
                table.insert(
                    "last_open".to_string(),
                    toml::Value::Integer(workspace.last_open as i64),
                );
                array.push(toml::Value::Table(table));
            }
        }
        let mut table = toml::value::Table::new();
        table.insert("workspaces".to_string(), toml::Value::Array(array));
        let content = toml::to_string(&table).ok()?;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .ok()?;
        file.write_all(content.as_bytes()).ok()?;
        None
    }

    pub fn recent_workspaces() -> Option<Vec<LapceWorkspace>> {
        let path = Self::recent_workspaces_file()?;
        let content = std::fs::read_to_string(&path).ok()?;
        let value: toml::Value = toml::from_str(&content).ok()?;
        Some(
            value
                .get("workspaces")
                .and_then(|v| v.as_array())?
                .iter()
                .filter_map(|value| {
                    let path = PathBuf::from(value.get("path")?.as_str()?);
                    let kind = value.get("kind")?.as_str()?;
                    let kind = match kind {
                        s if kind.starts_with("ssh://") => {
                            let mut parts = s[6..].split('@');
                            let user = parts.next()?.to_string();
                            let host = parts.next()?.to_string();
                            LapceWorkspaceType::RemoteSSH(user, host)
                        }
                        "wsl" => LapceWorkspaceType::RemoteWSL,
                        _ => LapceWorkspaceType::Local,
                    };
                    let last_open = value
                        .get("last_open")
                        .and_then(|v| v.as_integer())
                        .unwrap_or(0) as u64;
                    let workspace = LapceWorkspace {
                        kind,
                        path: Some(path),
                        last_open,
                    };
                    Some(workspace)
                })
                .collect(),
        )
    }

    pub fn recent_workspaces_file() -> Option<PathBuf> {
        let path = Directory::config_directory()?.join("workspaces.toml");
        {
            let _ = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }
        Some(path)
    }

    pub fn tab_width(
        &self,
        text: &mut PietText,
        font_family: FontFamily,
        font_size: usize,
    ) -> f64 {
        {
            let info = self.tab_layout_info.read();
            if let Some(width) = info.get(&(font_family.clone(), font_size)) {
                return self.editor.tab_width as f64 * *width;
            };
        }

        let width = text
            .new_text_layout(" a")
            .font(font_family.clone(), font_size as f64)
            .build()
            .unwrap()
            .hit_test_text_position(1)
            .point
            .x;

        self.tab_layout_info
            .write()
            .insert((font_family, font_size), width);
        self.editor.tab_width as f64 * width
    }
}
