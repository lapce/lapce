use std::{io::Write, path::PathBuf};

use anyhow::Result;
use directories::ProjectDirs;
use druid::{
    piet::{PietText, Text, TextLayout, TextLayoutBuilder},
    theme, Color, Env, ExtEventSink, FontDescriptor, FontFamily, Key, Size, Target,
};
use hashbrown::HashMap;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use structdesc::FieldNames;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::hex_to_color,
    state::{LapceWorkspace, LapceWorkspaceType},
};

const default_settings: &'static str = include_str!("../../defaults/settings.toml");
const default_light_theme: &'static str =
    include_str!("../../defaults/light-theme.toml");
const default_dark_theme: &'static str =
    include_str!("../../defaults/dark-theme.toml");
pub const LOGO: &'static str = include_str!("../../extra/images/logo.svg");

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

    pub const PANEL_BACKGROUND: &'static str = "panel.background";
    pub const PANEL_CURRENT: &'static str = "panel.current";

    pub const STATUS_BACKGROUND: &'static str = "status.background";
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
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct EditorConfig {
    #[field_names(desc = "Set the font family")]
    pub font_family: String,
    #[field_names(desc = "Set the font size")]
    pub font_size: usize,
    #[field_names(desc = "Set the line height")]
    pub line_height: usize,
    #[field_names(desc = "Set the tab width")]
    pub tab_width: usize,
    #[field_names(desc = "If opened editors are shown in a tab")]
    pub show_tab: bool,
}

impl EditorConfig {
    pub fn font_family(&self) -> FontFamily {
        FontFamily::new_unchecked(self.font_family.clone())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    pub lapce: LapceConfig,
    pub editor: EditorConfig,
    #[serde(skip)]
    pub theme: HashMap<String, Color>,
    #[serde(skip)]
    pub themes: HashMap<String, HashMap<String, Color>>,
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
                    self.event_sink.submit_command(
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
        let mut settings = config::Config::default().with_merged(
            config::File::from_str(default_settings, config::FileFormat::Toml),
        )?;

        if let Some(path) = Self::settings_file() {
            settings.merge(config::File::from(path.as_path()).required(false));
        }

        match workspace.kind {
            crate::state::LapceWorkspaceType::Local => {
                if let Some(path) = workspace.path.as_ref() {
                    let path = path.join("./.lapce/settings.toml");
                    settings
                        .merge(config::File::from(path.as_path()).required(false));
                }
            }
            crate::state::LapceWorkspaceType::RemoteSSH(_, _) => {}
        }

        let mut config: Config = settings.try_into()?;

        config.theme = get_theme(default_light_theme)?;

        let mut themes = HashMap::new();
        themes.insert("Lapce Light".to_string(), get_theme(default_light_theme)?);
        themes.insert("Lapce Dark".to_string(), get_theme(default_dark_theme)?);
        config.themes = themes;

        Ok(config)
    }

    pub fn dir() -> Option<PathBuf> {
        ProjectDirs::from("", "", "Lapce").map(|d| PathBuf::from(d.config_dir()))
    }

    pub fn log_file() -> Option<PathBuf> {
        let path = Self::dir().map(|d| {
            d.join(if !cfg!(debug_assertions) {
                "lapce.log"
            } else {
                "debug-lapce.log"
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

    pub fn settings_file() -> Option<PathBuf> {
        let path = Self::dir().map(|d| {
            d.join(if !cfg!(debug_assertions) {
                "settings.toml"
            } else {
                "debug-settings.toml"
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

    fn get_file_table() -> Option<toml::value::Table> {
        let path = Self::settings_file()?;
        let content = std::fs::read(&path).ok()?;
        let toml_value: toml::Value = toml::from_slice(&content).ok()?;
        let table = toml_value.as_table()?.clone();
        Some(table)
    }

    pub fn update_file(key: &str, value: toml::Value) -> Option<()> {
        let mut main_table =
            Self::get_file_table().unwrap_or_else(|| toml::value::Table::new());
        let mut table = &mut main_table;
        let parts: Vec<&str> = key.split(".").collect();
        let n = parts.len();
        for (i, key) in parts.into_iter().enumerate() {
            if i == n - 1 {
                table.insert(key.to_string(), value.clone());
            } else {
                if !table.contains_key(key) {
                    table.insert(
                        key.to_string(),
                        toml::Value::Table(toml::value::Table::new()),
                    );
                }
                table = table.get_mut(key)?.as_table_mut()?;
            }
        }

        let path = Self::settings_file()?;
        std::fs::write(&path, toml::to_string(&main_table).ok()?.as_bytes()).ok()?;
        None
    }

    pub fn set_theme(&mut self, theme: &str, preview: bool) -> Option<()> {
        self.lapce.color_theme = theme.to_string();
        if !preview {
            Config::update_file(
                "lapce.color-theme",
                toml::Value::String(theme.to_string()),
            )?;
        }
        None
    }

    pub fn get_color_unchecked(&self, name: &str) -> &Color {
        let theme = self
            .themes
            .get(&self.lapce.color_theme)
            .unwrap_or(&self.theme);
        theme.get(name).unwrap()
    }

    pub fn get_color(&self, name: &str) -> Option<&Color> {
        let theme = self
            .themes
            .get(&self.lapce.color_theme)
            .unwrap_or(&self.theme);
        theme.get(name)
    }

    pub fn editor_text_width(&self, text: &mut PietText, c: &str) -> f64 {
        let text_layout = text
            .new_text_layout(c.to_string())
            .font(self.editor.font_family(), self.editor.font_size as f64)
            .build()
            .unwrap();
        text_layout.size().width
    }

    pub fn editor_text_size(&self, text: &mut PietText, c: &str) -> Size {
        let text_layout = text
            .new_text_layout(c.to_string())
            .font(self.editor.font_family(), self.editor.font_size as f64)
            .build()
            .unwrap();
        text_layout.size()
    }

    pub fn reload_env(&self, env: &mut Env) {
        env.set(theme::SCROLLBAR_RADIUS, 0.0);
        env.set(theme::SCROLLBAR_EDGE_WIDTH, 0.0);
        env.set(theme::SCROLLBAR_WIDTH, 15.0);
        env.set(theme::SCROLLBAR_PAD, 0.0);
        env.set(theme::SCROLLBAR_MAX_OPACITY, 1.0);
        env.set(
            theme::SCROLLBAR_COLOR,
            Color::from_hex_str("#949494").unwrap(),
        );

        // env.set(key, value);

        //  let theme = &self.theme;
        //  if let Some(line_highlight) = theme.get("line_highlight") {
        //      env.set(
        //          LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
        //          line_highlight.clone(),
        //      );
        //  };
        //  if let Some(caret) = theme.get("caret") {
        //      env.set(LapceTheme::EDITOR_CURSOR_COLOR, caret.clone());
        //  };
        //  if let Some(foreground) = theme.get("foreground") {
        //      env.set(LapceTheme::EDITOR_FOREGROUND, foreground.clone());
        //  };
        //  if let Some(background) = theme.get("background") {
        //      env.set(LapceTheme::EDITOR_BACKGROUND, background.clone());
        //  };
        //  if let Some(selection) = theme.get("selection") {
        //      env.set(LapceTheme::EDITOR_SELECTION_COLOR, selection.clone());
        //  };
        //  if let Some(color) = theme.get("comment") {
        //      env.set(LapceTheme::EDITOR_COMMENT, color.clone());
        //  };
        //  if let Some(color) = theme.get("error") {
        //      env.set(LapceTheme::EDITOR_ERROR, color.clone());
        //  };
        //  if let Some(color) = theme.get("warn") {
        //      env.set(LapceTheme::EDITOR_WARN, color.clone());
        //  };
        //  env.set(LapceTheme::EDITOR_LINE_HEIGHT, 25.0);
        //  env.set(LapceTheme::PALETTE_BACKGROUND, Color::rgb8(125, 125, 125));
        //  env.set(LapceTheme::PALETTE_INPUT_FOREROUND, Color::rgb8(0, 0, 0));
        //  env.set(
        //      LapceTheme::PALETTE_INPUT_BACKGROUND,
        //      Color::rgb8(255, 255, 255),
        //  );
        //  env.set(LapceTheme::PALETTE_INPUT_BORDER, Color::rgb8(0, 0, 0));
        //  env.set(LapceTheme::LIST_BACKGROUND, Color::rgb8(234, 234, 235));
        //  env.set(LapceTheme::LIST_CURRENT, Color::rgb8(219, 219, 220));
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
                            let mut parts = s[6..].split("@");
                            let user = parts.next()?.to_string();
                            let host = parts.next()?.to_string();
                            LapceWorkspaceType::RemoteSSH(user, host)
                        }
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
        let proj_dirs = ProjectDirs::from("", "", "Lapce")?;
        std::fs::create_dir_all(proj_dirs.config_dir());
        let path = proj_dirs.config_dir().join("workspaces.toml");
        {
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path);
        }
        Some(path)
    }
}

fn get_theme(content: &str) -> Result<HashMap<String, Color>> {
    let theme_colors: std::collections::HashMap<String, String> =
        toml::from_str(content)?;
    let mut theme = HashMap::new();
    for (k, v) in theme_colors.iter() {
        if v.starts_with("$") {
            let var_name = &v[1..];
            if let Some(hex) = theme_colors.get(var_name) {
                if let Ok(color) = hex_to_color(hex) {
                    theme.insert(k.clone(), color);
                }
            }
        } else {
            if let Ok(color) = hex_to_color(v) {
                theme.insert(k.clone(), color);
            }
        }
    }
    Ok(theme)
}
