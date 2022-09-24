use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use druid::{
    piet::{PietText, Text, TextLayout, TextLayoutBuilder},
    Color, ExtEventSink, FontFamily, Size, Target,
};
use indexmap::IndexMap;
use lapce_proxy::{directory::Directory, plugin::wasi::find_all_volts};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use structdesc::FieldNames;
use thiserror::Error;
use toml_edit::easy as toml;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceWorkspace, LapceWorkspaceType},
};

pub use lapce_proxy::APPLICATION_NAME;

const DEFAULT_SETTINGS: &str = include_str!("../../defaults/settings.toml");
const DEFAULT_LIGHT_THEME: &str = include_str!("../../defaults/light-theme.toml");
const DEFAULT_DARK_THEME: &str = include_str!("../../defaults/dark-theme.toml");
pub const LOGO: &str = include_str!("../../extra/images/logo.svg");

pub struct LapceTheme {}

impl LapceTheme {
    pub const LAPCE_WARN: &'static str = "lapce.warn";
    pub const LAPCE_ERROR: &'static str = "lapce.error";
    pub const LAPCE_ACTIVE_TAB: &'static str = "lapce.active_tab";
    pub const LAPCE_INACTIVE_TAB: &'static str = "lapce.inactive_tab";
    pub const LAPCE_DROPDOWN_SHADOW: &'static str = "lapce.dropdown_shadow";
    pub const LAPCE_BORDER: &'static str = "lapce.border";
    pub const LAPCE_SCROLL_BAR: &'static str = "lapce.scroll_bar";

    pub const EDITOR_BACKGROUND: &'static str = "editor.background";
    pub const EDITOR_FOREGROUND: &'static str = "editor.foreground";
    pub const EDITOR_DIM: &'static str = "editor.dim";
    pub const EDITOR_FOCUS: &'static str = "editor.focus";
    pub const EDITOR_CARET: &'static str = "editor.caret";
    pub const EDITOR_SELECTION: &'static str = "editor.selection";
    pub const EDITOR_CURRENT_LINE: &'static str = "editor.current_line";
    pub const EDITOR_LINK: &'static str = "editor.link";

    pub const INLAY_HINT_FOREGROUND: &'static str = "inlay_hint.foreground";
    pub const INLAY_HINT_BACKGROUND: &'static str = "inlay_hint.background";

    pub const ERROR_LENS_ERROR_FOREGROUND: &'static str =
        "error_lens.error.foreground";
    pub const ERROR_LENS_ERROR_BACKGROUND: &'static str =
        "error_lens.error.background";
    pub const ERROR_LENS_WARNING_FOREGROUND: &'static str =
        "error_lens.warning.foreground";
    pub const ERROR_LENS_WARNING_BACKGROUND: &'static str =
        "error_lens.warning.background";
    pub const ERROR_LENS_OTHER_FOREGROUND: &'static str =
        "error_lens.other.foreground";
    pub const ERROR_LENS_OTHER_BACKGROUND: &'static str =
        "error_lens.other.background";

    pub const SOURCE_CONTROL_ADDED: &'static str = "source_control.added";
    pub const SOURCE_CONTROL_REMOVED: &'static str = "source_control.removed";
    pub const SOURCE_CONTROL_MODIFIED: &'static str = "source_control.modified";

    pub const TERMINAL_CURSOR: &'static str = "terminal.cursor";
    pub const TERMINAL_BACKGROUND: &'static str = "terminal.background";
    pub const TERMINAL_FOREGROUND: &'static str = "terminal.foreground";
    pub const TERMINAL_RED: &'static str = "terminal.red";
    pub const TERMINAL_BLUE: &'static str = "terminal.blue";
    pub const TERMINAL_GREEN: &'static str = "terminal.green";
    pub const TERMINAL_YELLOW: &'static str = "terminal.yellow";
    pub const TERMINAL_BLACK: &'static str = "terminal.black";
    pub const TERMINAL_WHITE: &'static str = "terminal.white";
    pub const TERMINAL_CYAN: &'static str = "terminal.cyan";
    pub const TERMINAL_MAGENTA: &'static str = "terminal.magenta";

    pub const TERMINAL_BRIGHT_RED: &'static str = "terminal.bright_red";
    pub const TERMINAL_BRIGHT_BLUE: &'static str = "terminal.bright_blue";
    pub const TERMINAL_BRIGHT_GREEN: &'static str = "terminal.bright_green";
    pub const TERMINAL_BRIGHT_YELLOW: &'static str = "terminal.bright_yellow";
    pub const TERMINAL_BRIGHT_BLACK: &'static str = "terminal.bright_black";
    pub const TERMINAL_BRIGHT_WHITE: &'static str = "terminal.bright_white";
    pub const TERMINAL_BRIGHT_CYAN: &'static str = "terminal.bright_cyan";
    pub const TERMINAL_BRIGHT_MAGENTA: &'static str = "terminal.bright_magenta";

    pub const PALETTE_BACKGROUND: &'static str = "palette.background";
    pub const PALETTE_CURRENT: &'static str = "palette.current";

    pub const COMPLETION_BACKGROUND: &'static str = "completion.background";
    pub const COMPLETION_CURRENT: &'static str = "completion.current";

    pub const HOVER_BACKGROUND: &'static str = "hover.background";

    pub const ACTIVITY_BACKGROUND: &'static str = "activity.background";
    pub const ACTIVITY_CURRENT: &'static str = "activity.current";

    pub const PANEL_BACKGROUND: &'static str = "panel.background";
    pub const PANEL_CURRENT: &'static str = "panel.current";
    pub const PANEL_HOVERED: &'static str = "panel.hovered";

    pub const STATUS_BACKGROUND: &'static str = "status.background";
    pub const STATUS_MODAL_NORMAL: &'static str = "status.modal.normal";
    pub const STATUS_MODAL_INSERT: &'static str = "status.modal.insert";
    pub const STATUS_MODAL_VISUAL: &'static str = "status.modal.visual";
    pub const STATUS_MODAL_TERMINAL: &'static str = "status.modal.terminal";

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
    fn get_config(&self) -> &Config;
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct LapceConfig {
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
                if let Some(stripped) = hex.strip_prefix('$') {
                    if let Some(c) = base.get(stripped) {
                        return (name.to_string(), c.clone());
                    }
                    if let Some(default) = default {
                        if let Some(c) = default.get(name) {
                            return (name.to_string(), c.clone());
                        }
                    }
                    return (name.to_string(), Color::rgb8(0, 0, 0));
                }

                if let Ok(c) = Color::from_hex_str(hex) {
                    return (name.to_string(), c);
                }
                if let Some(default) = default {
                    if let Some(c) = default.get(name) {
                        return (name.to_string(), c.clone());
                    }
                }
                (name.to_string(), Color::rgb8(0, 0, 0))
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
        ThemeBaseColor {
            white: Color::from_hex_str(&self.white).unwrap_or_else(|_| {
                default
                    .map(|d| d.white.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            black: Color::from_hex_str(&self.black).unwrap_or_else(|_| {
                default
                    .map(|d| d.black.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            grey: Color::from_hex_str(&self.grey).unwrap_or_else(|_| {
                default
                    .map(|d| d.grey.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            blue: Color::from_hex_str(&self.blue).unwrap_or_else(|_| {
                default
                    .map(|d| d.blue.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            red: Color::from_hex_str(&self.red).unwrap_or_else(|_| {
                default
                    .map(|d| d.red.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            yellow: Color::from_hex_str(&self.yellow).unwrap_or_else(|_| {
                default
                    .map(|d| d.yellow.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            orange: Color::from_hex_str(&self.orange).unwrap_or_else(|_| {
                default
                    .map(|d| d.orange.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            green: Color::from_hex_str(&self.green).unwrap_or_else(|_| {
                default
                    .map(|d| d.green.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            purple: Color::from_hex_str(&self.purple).unwrap_or_else(|_| {
                default
                    .map(|d| d.purple.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            cyan: Color::from_hex_str(&self.cyan).unwrap_or_else(|_| {
                default
                    .map(|d| d.cyan.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
            magenta: Color::from_hex_str(&self.magenta).unwrap_or_else(|_| {
                default
                    .map(|d| d.magenta.clone())
                    .unwrap_or_else(|| Color::rgb8(0, 0, 0))
            }),
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
pub struct Config {
    #[serde(skip)]
    pub id: u64,
    pub lapce: LapceConfig,
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
}

impl ConfigWatcher {
    pub fn new(event_sink: ExtEventSink) -> Self {
        Self { event_sink }
    }
}

impl notify::EventHandler for ConfigWatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        if let Ok(event) = event {
            match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    let _ = self.event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::ReloadConfig,
                        Target::Auto,
                    );
                }
                _ => (),
            }
        }
    }
}

impl Config {
    pub fn load(workspace: &LapceWorkspace) -> Result<Self> {
        let default_settings = Self::default_settings();
        let mut default_config: Config =
            default_settings.clone().try_into().unwrap();
        default_config.resolve_colors(None);

        let settings =
            Self::merge_settings(default_settings.clone(), workspace, None);
        let mut config: Config = settings.try_into()?;
        let available_themes = Self::load_themes();
        if let Some((_, theme)) =
            available_themes.get(&config.lapce.color_theme.to_lowercase())
        {
            if let Ok(theme_settings) =
                default_settings.clone().with_merged(theme.clone())
            {
                if let Ok(mut theme_config) = theme_settings.try_into::<Config>() {
                    theme_config.resolve_colors(Some(&default_config));
                    default_config = theme_config;
                }
            }
            config = Self::merge_settings(
                default_settings,
                workspace,
                Some(theme.clone()),
            )
            .try_into()?;
        }
        config.update_id();
        config.available_themes = available_themes;
        config.resolve_colors(Some(&default_config));
        config.default_theme = default_config.theme.clone();

        Ok(config)
    }

    fn merge_settings(
        mut settings: config::Config,
        workspace: &LapceWorkspace,
        theme: Option<config::Config>,
    ) -> config::Config {
        if let Some(theme) = theme {
            let _ = settings.merge(theme);
        }

        if let Some(path) = Self::settings_file() {
            let _ =
                settings.merge(config::File::from(path.as_path()).required(false));
        }

        match workspace.kind {
            LapceWorkspaceType::Local => {
                if let Some(path) = workspace.path.as_ref() {
                    let path = path.join("./.lapce/settings.toml");
                    let _ = settings
                        .merge(config::File::from(path.as_path()).required(false));
                }
            }
            LapceWorkspaceType::RemoteSSH(_, _) => {}
            LapceWorkspaceType::RemoteWSL => {}
        }

        settings
    }

    fn resolve_colors(&mut self, default_config: Option<&Config>) {
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

    fn load_themes() -> HashMap<String, (String, config::Config)> {
        let mut themes = Self::load_local_themes().unwrap_or_default();
        if let Some(plugin_themes) = Self::load_plugin_themes() {
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

    fn load_plugin_themes() -> Option<HashMap<String, (String, config::Config)>> {
        let mut themes: HashMap<String, (String, config::Config)> = HashMap::new();
        for meta in find_all_volts() {
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
        let settings = config::Config::new()
            .with_merged(config::File::from_str(s, config::FileFormat::Toml))
            .ok()?;
        let table = settings.get_table("theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, settings))
    }

    fn load_theme(path: &Path) -> Option<(String, (String, config::Config))> {
        if !path.is_file() {
            return None;
        }
        let settings = config::Config::new()
            .with_merged(config::File::from(path))
            .ok()?;
        let table = settings.get_table("theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name.to_lowercase(), (name, settings)))
    }

    fn default_settings() -> config::Config {
        config::Config::default()
            .with_merged(config::File::from_str(
                DEFAULT_SETTINGS,
                config::FileFormat::Toml,
            ))
            .unwrap()
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
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }

    pub fn set_theme(&mut self, theme: &str, preview: bool) -> bool {
        self.update_id();

        self.lapce.color_theme = theme.to_string();

        if !preview
            && Config::update_file(
                "lapce",
                "color-theme",
                toml_edit::Value::from(theme),
            )
            .is_none()
        {
            return false;
        }

        true
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
