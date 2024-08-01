use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    slice,
    sync::Arc,
};

use anyhow::{anyhow, Result};
use directory::Directory;
use floem::peniko::Color;
use itertools::Itertools;
use lapce_proxy::plugin::wasi::find_all_volts;
use lapce_rpc::plugin::VoltID;
use lsp_types::{CompletionItemKind, SymbolKind};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Deserialize;
use strum::VariantNames;
use tracing::{trace, TraceLevel};

use self::{
    color::LapceColor,
    color_theme::{ColorThemeConfig, ThemeColor, ThemeColorPreference},
    core::CoreConfig,
    editor::{EditorConfig, WrapStyle, SCALE_OR_SIZE_LIMIT},
    icon::LapceIcons,
    icon_theme::{file::FileIconThemeConfig, ui::UIIconThemeConfig},
    remote::RemoteConfig,
    svg::SvgStore,
    terminal::TerminalConfig,
    ui::UIConfig,
};
use crate::workspace::{LapceWorkspace, LapceWorkspaceType};

pub mod color;
pub mod color_theme;
pub mod core;
pub mod editor;
pub mod icon;
pub mod icon_theme;
pub mod remote;
pub mod svg;
pub mod terminal;
pub mod ui;
pub mod watcher;

pub const LOGO: &str = include_str!("../../extra/images/logo.svg");
const DEFAULT_SETTINGS: &str = include_str!("../../defaults/settings.toml");
const DEFAULT_LIGHT_THEME: &str = include_str!("../../defaults/light-theme.toml");
const DEFAULT_DARK_THEME: &str = include_str!("../../defaults/dark-theme.toml");
const DEFAULT_FILE_ICON_THEME: &str =
    include_str!("../../defaults/icon-theme-file.toml");
const DEFAULT_UI_ICON_THEME: &str =
    include_str!("../../defaults/icon-theme-ui.toml");

static DEFAULT_CONFIG: Lazy<config::Config> = Lazy::new(LapceConfig::default_config);

static DEFAULT_DARK_THEME_CONFIG: Lazy<config::Config> = Lazy::new(|| {
    config::Config::builder()
        .add_source(config::File::from_str(
            DEFAULT_DARK_THEME,
            config::FileFormat::Toml,
        ))
        .build()
        .unwrap()
});

/// Default icon theme for icons describing files
static DEFAULT_FILE_ICON_THEME_CONFIG: Lazy<config::Config> = Lazy::new(|| {
    config::Config::builder()
        .add_source(config::File::from_str(
            DEFAULT_FILE_ICON_THEME,
            config::FileFormat::Toml,
        ))
        .build()
        .unwrap()
});
static DEFAULT_FILE_ICON_THEME_ICON_CONFIG: Lazy<FileIconThemeConfig> = Lazy::new(
    || {
        DEFAULT_FILE_ICON_THEME_CONFIG.get::<FileIconThemeConfig>("icon-theme")
    .expect("Failed to load default file icon theme. This is likely due to a missing or misnamed field in file-icon-theme.toml")
    },
);

/// Default icon theme for icons used in Lapce UI
static DEFAULT_UI_ICON_THEME_CONFIG: Lazy<config::Config> = Lazy::new(|| {
    config::Config::builder()
        .add_source(config::File::from_str(
            DEFAULT_UI_ICON_THEME,
            config::FileFormat::Toml,
        ))
        .build()
        .unwrap()
});
static DEFAULT_UI_ICON_THEME_ICON_CONFIG: Lazy<UIIconThemeConfig> = Lazy::new(
    || {
        DEFAULT_UI_ICON_THEME_CONFIG.get::<UIIconThemeConfig>("icon-theme")
    .expect("Failed to load default ui icon theme. This is likely due to a missing or misnamed field in ui-icon-theme.toml")
    },
);

/// Used for creating a `DropdownData` for a setting
#[derive(Debug, Clone)]
pub struct DropdownInfo {
    /// The currently selected item.
    pub active_index: usize,
    pub items: im::Vector<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct LapceConfig {
    #[serde(skip)]
    pub id: u64,

    pub core: CoreConfig,
    pub editor: EditorConfig,
    pub remote: RemoteConfig,
    pub terminal: TerminalConfig,
    pub ui: UIConfig,

    /// Current colour theme config
    #[serde(default)]
    pub color_theme: ColorThemeConfig,
    /// Current file icon theme config
    #[serde(default)]
    pub file_icon_theme: FileIconThemeConfig,
    /// Current UI icon theme config
    #[serde(default)]
    pub ui_icon_theme: UIIconThemeConfig,
    /// Installed plugins
    #[serde(flatten)]
    pub plugins: HashMap<String, HashMap<String, serde_json::Value>>,
    /// Current colour scheme
    #[serde(skip)]
    pub color: ThemeColor,
    /// List of available color themes
    #[serde(skip)]
    pub available_color_themes: HashMap<String, (String, config::Config)>,
    /// List of available file icon themes
    #[serde(skip)]
    pub available_file_icon_themes:
        HashMap<String, (String, config::Config, Option<PathBuf>)>,
    /// List of available UI icon themes
    #[serde(skip)]
    pub available_ui_icon_themes:
        HashMap<String, (String, config::Config, Option<PathBuf>)>,
    // #[serde(skip)]
    // tab_layout_info: Arc<RwLock<HashMap<(FontFamily, usize), f64>>>,
    #[serde(skip)]
    svg_store: Arc<RwLock<SvgStore>>,
    /// A list of the themes that are available. This is primarily for populating
    /// the theme picker, and serves as a cache.
    #[serde(skip)]
    color_theme_list: im::Vector<String>,
    /// A list of the themes that are available. This is primarily for populating
    /// the theme picker, and serves as a cache.
    #[serde(skip)]
    file_icon_theme_list: im::Vector<String>,
    /// A list of the themes that are available. This is primarily for populating
    /// the theme picker, and serves as a cache.
    #[serde(skip)]
    ui_icon_theme_list: im::Vector<String>,
    /// The couple names for the wrap style
    #[serde(skip)]
    wrap_style_list: im::Vector<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LapceConfigError {
    #[error("Failed to deserialise config")]
    DeserError(String),
    #[error("Unknown error")]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("Unknown error")]
    Unknown,
}

impl LapceConfig {
    pub fn load(
        workspace: &LapceWorkspace,
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> Result<Self> {
        let config = Self::merge_config(workspace, None, None, None);
        let mut lapce_config: LapceConfig = match config.try_deserialize() {
            Ok(config) => config,
            Err(error) => {
                trace!(
                    TraceLevel::ERROR,
                    "Failed to deserialize configuration file: {error}"
                );
                LapceConfig::default_lapce_config()?.clone()
            }
        };

        lapce_config.available_color_themes =
            Self::load_color_themes(disabled_volts, extra_plugin_paths)?;
        lapce_config.available_file_icon_themes =
            Self::load_icon_themes(disabled_volts, extra_plugin_paths)?;
        lapce_config.available_ui_icon_themes =
            Self::load_icon_themes(disabled_volts, extra_plugin_paths)?;
        lapce_config.resolve_theme(workspace, Some(&lapce_config.clone()));

        lapce_config.color_theme_list = lapce_config
            .available_color_themes
            .values()
            .map(|(name, _)| name.clone())
            .sorted()
            .collect();
        lapce_config.color_theme_list.sort();

        lapce_config.file_icon_theme_list = lapce_config
            .available_file_icon_themes
            .values()
            .map(|(name, _, _)| name.clone())
            .sorted()
            .collect();
        lapce_config.file_icon_theme_list.sort();

        lapce_config.ui_icon_theme_list = lapce_config
            .available_ui_icon_themes
            .values()
            .map(|(name, _, _)| name.clone())
            .sorted()
            .collect();
        lapce_config.ui_icon_theme_list.sort();

        lapce_config.wrap_style_list = im::vector![
            WrapStyle::None.to_string(),
            WrapStyle::EditorWidth.to_string(),
            // TODO: WrapStyle::WrapColumn.to_string(),
            WrapStyle::WrapWidth.to_string()
        ];

        lapce_config.terminal.get_indexed_colors();

        Ok(lapce_config)
    }

    fn merge_config(
        workspace: &LapceWorkspace,
        color_theme_config: Option<config::Config>,
        file_icon_theme_config: Option<config::Config>,
        ui_icon_theme_config: Option<config::Config>,
    ) -> config::Config {
        let mut config = DEFAULT_CONFIG.clone();

        if let Some(theme) = color_theme_config {
            // TODO: use different color theme basis if the theme declares its color preference
            // differently
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(DEFAULT_DARK_THEME_CONFIG.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(theme) = file_icon_theme_config {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(theme) = ui_icon_theme_config {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Ok(path) = Self::settings_file() {
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
            LapceWorkspaceType::RemoteSSH(_) => {}
            LapceWorkspaceType::RemoteGH(_) => {}
            LapceWorkspaceType::RemoteCustom(_) => {}
            LapceWorkspaceType::RemoteTS(_) => {}
            #[cfg(windows)]
            LapceWorkspaceType::RemoteWSL(_) => {}
        }

        config
    }

    fn update_id(&mut self) {
        self.id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
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

    fn default_lapce_config() -> Result<LapceConfig, LapceConfigError> {
        let mut default_lapce_config: LapceConfig = match DEFAULT_CONFIG
            .clone()
            .try_deserialize()
        {
            Ok(v) => v,
            Err(e) => {
                return Err(LapceConfigError::DeserError(format!("Failed to deserialize default config, this likely indicates a missing or misnamed field in settings.toml: {e}")));
            }
        };
        default_lapce_config.color_theme = color_theme::dark::THEME.clone();
        default_lapce_config.file_icon_theme =
            DEFAULT_FILE_ICON_THEME_ICON_CONFIG.clone();
        default_lapce_config.ui_icon_theme =
            DEFAULT_UI_ICON_THEME_ICON_CONFIG.clone();
        default_lapce_config.resolve_colors(None);
        Ok(default_lapce_config)
    }

    fn resolve_theme(
        &mut self,
        workspace: &LapceWorkspace,
        default_config: Option<&LapceConfig>,
    ) {
        let color_theme_config = self
            .available_color_themes
            .get(&self.core.color_theme.to_lowercase())
            .map(|(_, config)| config)
            .unwrap_or(&DEFAULT_DARK_THEME_CONFIG);

        let file_icon_theme_config = self
            .available_file_icon_themes
            .get(&self.core.file_icon_theme.to_lowercase())
            .map(|(_, config, _)| config)
            .unwrap_or(&DEFAULT_FILE_ICON_THEME_CONFIG);

        let file_icon_theme_path = self
            .available_file_icon_themes
            .get(&self.core.file_icon_theme.to_lowercase())
            .map(|(_, _, path)| path);

        let ui_icon_theme_config = self
            .available_file_icon_themes
            .get(&self.core.ui_icon_theme.to_lowercase())
            .map(|(_, config, _)| config)
            .unwrap_or(&DEFAULT_FILE_ICON_THEME_CONFIG);

        let ui_icon_theme_path = self
            .available_file_icon_themes
            .get(&self.core.ui_icon_theme.to_lowercase())
            .map(|(_, _, path)| path);

        if let Ok(new) = Self::merge_config(
            workspace,
            Some(color_theme_config.clone()),
            Some(file_icon_theme_config.clone()),
            Some(ui_icon_theme_config.clone()),
        )
        .try_deserialize::<LapceConfig>()
        {
            self.core = new.core;
            self.ui = new.ui;
            self.editor = new.editor;
            self.terminal = new.terminal;
            self.terminal.get_indexed_colors();

            self.color_theme = new.color_theme;
            self.file_icon_theme = new.file_icon_theme;
            if let Some(icon_theme_path) = file_icon_theme_path {
                self.file_icon_theme.path =
                    icon_theme_path.clone().unwrap_or_default();
            }
            self.ui_icon_theme = new.ui_icon_theme;
            if let Some(icon_theme_path) = ui_icon_theme_path {
                self.ui_icon_theme.path =
                    icon_theme_path.clone().unwrap_or_default();
            }
            self.plugins = new.plugins;
        }
        self.resolve_colors(default_config);
        self.update_id();
    }

    fn load_color_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> Result<HashMap<String, (String, config::Config)>> {
        let mut themes = Self::load_local_themes()?;

        for (key, theme) in
            Self::load_plugin_color_themes(disabled_volts, extra_plugin_paths)
        {
            themes.insert(key, theme);
        }

        let (name, theme) = Self::load_color_theme_from_str(DEFAULT_LIGHT_THEME)?;
        themes.insert(name.to_lowercase(), (name, theme));
        let (name, theme) = Self::load_color_theme_from_str(DEFAULT_DARK_THEME)?;
        themes.insert(name.to_lowercase(), (name, theme));

        Ok(themes)
    }

    pub fn default_color_theme(&self) -> &ColorThemeConfig {
        &color_theme::dark::THEME
    }

    /// Set the active color theme.
    /// Note that this does not save the config.
    pub fn set_color_theme(&mut self, workspace: &LapceWorkspace, theme: &str) {
        self.core.color_theme = theme.to_string();
        self.resolve_theme(workspace, None);
    }

    /// Set the active file icon theme.  
    /// Note that this does not save the config.
    pub fn set_file_icon_theme(&mut self, workspace: &LapceWorkspace, theme: &str) {
        self.core.file_icon_theme = theme.to_string();
        self.resolve_theme(workspace, None);
    }

    /// Set the active UI icon theme.  
    /// Note that this does not save the config.
    pub fn set_ui_icon_theme(&mut self, workspace: &LapceWorkspace, theme: &str) {
        self.core.ui_icon_theme = theme.to_string();
        self.resolve_theme(workspace, None);
    }

    pub fn set_modal(&mut self, _workspace: &LapceWorkspace, modal: bool) {
        self.core.modal = modal;
    }

    /// Get the color by the name from the current theme if it exists
    /// Otherwise, get the color from the base them
    /// # Panics
    /// If the color was not able to be found in either theme, which may be indicative that
    /// it is misspelled or needs to be added to the base-theme.
    pub fn color(&self, name: &str) -> Color {
        match self.color.ui.get(name) {
            Some(c) => *c,
            None => {
                trace!(TraceLevel::ERROR, "Failed to find key: {name}");
                Color::HOT_PINK
            }
        }
    }

    /// Retrieve a color value whose key starts with "style."
    pub fn style_color(&self, name: &str) -> Option<Color> {
        self.color.syntax.get(name).copied()
    }

    pub fn completion_color(
        &self,
        kind: Option<CompletionItemKind>,
    ) -> Option<Color> {
        let kind = kind?;
        let theme_str = match kind {
            CompletionItemKind::METHOD => "method",
            CompletionItemKind::FUNCTION => "method",
            CompletionItemKind::ENUM => "enum",
            CompletionItemKind::ENUM_MEMBER => "enum-member",
            CompletionItemKind::CLASS => "class",
            CompletionItemKind::VARIABLE => "field",
            CompletionItemKind::STRUCT => "structure",
            CompletionItemKind::KEYWORD => "keyword",
            CompletionItemKind::CONSTANT => "constant",
            CompletionItemKind::PROPERTY => "property",
            CompletionItemKind::FIELD => "field",
            CompletionItemKind::INTERFACE => "interface",
            CompletionItemKind::SNIPPET => "snippet",
            CompletionItemKind::MODULE => "builtinType",
            _ => "string",
        };

        self.style_color(theme_str)
    }

    fn resolve_colors(&mut self, default_config: Option<&LapceConfig>) {
        self.color.base = self
            .color_theme
            .base
            .resolve(default_config.map(|c| &c.color_theme.base));
        self.color.ui = self
            .color_theme
            .resolve_ui_color(&self.color.base, default_config.map(|c| &c.color.ui));
        self.color.syntax = self.color_theme.resolve_syntax_color(
            &self.color.base,
            default_config.map(|c| &c.color.syntax),
        );

        let fg = self.color(LapceColor::EDITOR_FOREGROUND);
        let bg = self.color(LapceColor::EDITOR_BACKGROUND);
        let is_light = fg.r as u32 + fg.g as u32 + fg.b as u32
            > bg.r as u32 + bg.g as u32 + bg.b as u32;
        let high_contrast = self.color_theme.high_contrast.unwrap_or(false);
        self.color.color_preference = match (is_light, high_contrast) {
            (true, true) => ThemeColorPreference::HighContrastLight,
            (false, true) => ThemeColorPreference::HighContrastDark,
            (true, false) => ThemeColorPreference::Light,
            (false, false) => ThemeColorPreference::Dark,
        };
    }

    fn load_local_themes() -> Result<HashMap<String, (String, config::Config)>> {
        let themes_folder = Directory::themes_directory()?;
        let themes: HashMap<String, (String, config::Config)> =
            std::fs::read_dir(themes_folder)?
                .filter_map(|entry| {
                    if let Ok(entry) = entry {
                        Self::load_color_theme(&entry.path()).ok()
                    } else {
                        None
                    }
                })
                .collect();
        Ok(themes)
    }

    fn load_color_theme(path: &Path) -> Result<(String, (String, config::Config))> {
        if !path.is_file() {
            return Err(anyhow!("Provided path is not a file: {}", path.display()));
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()?;
        let table = config.get_table(color_theme::CONFIG_KEY)?;
        let name = table
            .get("name")
            .ok_or(anyhow!(
                "No key `name` available on table `{}`",
                color_theme::CONFIG_KEY
            ))?
            .to_string();
        Ok((name.to_lowercase(), (name, config)))
    }

    /// Load the given theme by its contents.  
    /// Returns `(name, theme fields)`
    fn load_color_theme_from_str(
        s: &str,
    ) -> anyhow::Result<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()?;
        let table = config.get_table(color_theme::CONFIG_KEY)?;
        let name = table
            .get("name")
            .ok_or(anyhow!(
                "No key `name` available on table `{}`",
                color_theme::CONFIG_KEY
            ))?
            .to_string();
        Ok((name, config))
    }

    #[allow(clippy::type_complexity)]
    fn load_icon_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> Result<HashMap<String, (String, config::Config, Option<PathBuf>)>> {
        let mut themes = HashMap::new();

        for (key, (name, theme, path)) in
            Self::load_plugin_icon_themes(disabled_volts, extra_plugin_paths)
        {
            themes.insert(key, (name, theme, Some(path)));
        }

        let (name, theme) = Self::load_icon_theme_from_str(DEFAULT_UI_ICON_THEME)?;
        themes.insert(name.to_lowercase(), (name, theme, None));

        Ok(themes)
    }

    fn load_icon_theme_from_str(s: &str) -> Result<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()?;
        let table = config.get_table("icon-theme")?;
        let name = table
            .get("name")
            .ok_or(anyhow!(
                "No key `name` available on table `{}`",
                icon_theme::CONFIG_KEY
            ))?
            .to_string();
        Ok((name, config))
    }

    fn load_plugin_color_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes: HashMap<String, (String, config::Config)> = HashMap::new();
        for meta in find_all_volts(extra_plugin_paths) {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.color_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Ok((key, theme)) =
                        Self::load_color_theme(&PathBuf::from(theme_path))
                    {
                        themes.insert(key, theme);
                    }
                }
            }
        }
        themes
    }

    fn load_plugin_icon_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> HashMap<String, (String, config::Config, PathBuf)> {
        let mut themes: HashMap<String, (String, config::Config, PathBuf)> =
            HashMap::new();
        for meta in find_all_volts(extra_plugin_paths) {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.icon_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Ok((key, theme)) =
                        Self::load_icon_theme(&PathBuf::from(theme_path))
                    {
                        themes.insert(key, theme);
                    }
                }
            }
        }
        themes
    }

    fn load_icon_theme(
        path: &Path,
    ) -> Result<(String, (String, config::Config, PathBuf))> {
        if !path.is_file() {
            return Err(anyhow!("Provided path is not a file: {}", path.display()));
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()?;
        let table = config.get_table("file-icon-theme")?;
        let name = table
            .get("name")
            .ok_or(anyhow!(
                "No key `name` available on table `{}`",
                icon_theme::CONFIG_KEY
            ))?
            .to_string();
        Ok((
            name.to_lowercase(),
            (
                name,
                config,
                path.parent()
                    .ok_or(anyhow!("Failed to obtain parent path"))?
                    .to_path_buf(),
            ),
        ))
    }

    pub fn export_theme(&self) -> Result<String> {
        let mut table = toml::value::Table::new();
        let mut theme = self.color_theme.clone();
        theme.name = String::from("User Theme");
        table.insert(
            color_theme::CONFIG_KEY.to_string(),
            toml::Value::try_from(&theme)?,
        );
        let value = toml::Value::Table(table);
        Ok(toml::to_string_pretty(&value)?)
    }

    pub fn settings_file() -> Result<PathBuf> {
        let path = Directory::config_directory(None)?.join("settings.toml");

        if !path.exists() {
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
        }

        Ok(path)
    }

    pub fn keymaps_file() -> Result<PathBuf> {
        let path = Directory::config_directory(None)?.join("keymaps.toml");

        if !path.exists() {
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
        }

        Ok(path)
    }

    pub fn ui_svg(&self, icon: &'static str) -> String {
        let svg = self.ui_icon_theme.icons.get(icon).and_then(|path| {
            let path = self.ui_icon_theme.path.join(path);
            self.svg_store.write().get_svg_on_disk(&path)
        });

        svg.unwrap_or_else(|| {
            let name = DEFAULT_UI_ICON_THEME_ICON_CONFIG.icons.get(icon).unwrap();
            self.svg_store.write().get_default_ui_svg(name)
        })
    }

    pub fn files_svg(&self, paths: &[&Path]) -> (String, Option<Color>) {
        let svg = self
            .file_icon_theme
            .resolve_path_to_icon(paths)
            .and_then(|p| self.svg_store.write().get_svg_on_disk(&p));

        if let Some(svg) = svg {
            let color = if self.file_icon_theme.use_editor_color.unwrap_or(false) {
                Some(self.color(LapceColor::LAPCE_ICON_ACTIVE))
            } else {
                None
            };
            (svg, color)
        } else {
            (
                self.ui_svg(LapceIcons::FILE),
                Some(self.color(LapceColor::LAPCE_ICON_ACTIVE)),
            )
        }
    }

    pub fn file_svg(&self, path: &Path) -> (String, Option<Color>) {
        self.files_svg(slice::from_ref(&path))
    }

    pub fn symbol_svg(&self, kind: &SymbolKind) -> Option<String> {
        let kind_str = match *kind {
            SymbolKind::ARRAY => LapceIcons::SYMBOL_KIND_ARRAY,
            SymbolKind::BOOLEAN => LapceIcons::SYMBOL_KIND_BOOLEAN,
            SymbolKind::CLASS => LapceIcons::SYMBOL_KIND_CLASS,
            SymbolKind::CONSTANT => LapceIcons::SYMBOL_KIND_CONSTANT,
            SymbolKind::ENUM_MEMBER => LapceIcons::SYMBOL_KIND_ENUM_MEMBER,
            SymbolKind::ENUM => LapceIcons::SYMBOL_KIND_ENUM,
            SymbolKind::EVENT => LapceIcons::SYMBOL_KIND_EVENT,
            SymbolKind::FIELD => LapceIcons::SYMBOL_KIND_FIELD,
            SymbolKind::FILE => LapceIcons::SYMBOL_KIND_FILE,
            SymbolKind::INTERFACE => LapceIcons::SYMBOL_KIND_INTERFACE,
            SymbolKind::KEY => LapceIcons::SYMBOL_KIND_KEY,
            SymbolKind::FUNCTION => LapceIcons::SYMBOL_KIND_FUNCTION,
            SymbolKind::METHOD => LapceIcons::SYMBOL_KIND_METHOD,
            SymbolKind::OBJECT => LapceIcons::SYMBOL_KIND_OBJECT,
            SymbolKind::NAMESPACE => LapceIcons::SYMBOL_KIND_NAMESPACE,
            SymbolKind::NUMBER => LapceIcons::SYMBOL_KIND_NUMBER,
            SymbolKind::OPERATOR => LapceIcons::SYMBOL_KIND_OPERATOR,
            SymbolKind::TYPE_PARAMETER => LapceIcons::SYMBOL_KIND_TYPE_PARAMETER,
            SymbolKind::PROPERTY => LapceIcons::SYMBOL_KIND_PROPERTY,
            SymbolKind::STRING => LapceIcons::SYMBOL_KIND_STRING,
            SymbolKind::STRUCT => LapceIcons::SYMBOL_KIND_STRUCT,
            SymbolKind::VARIABLE => LapceIcons::SYMBOL_KIND_VARIABLE,
            _ => return None,
        };

        Some(self.ui_svg(kind_str))
    }

    pub fn logo_svg(&self) -> String {
        self.svg_store.read().logo_svg()
    }

    /// List of the color themes that are available by their display names.
    pub fn color_theme_list(&self) -> im::Vector<String> {
        self.color_theme_list.clone()
    }

    /// List of the icon themes that are available by their display names.
    pub fn file_icon_theme_list(&self) -> im::Vector<String> {
        self.file_icon_theme_list.clone()
    }

    /// List of the icon themes that are available by their display names.
    pub fn ui_icon_theme_list(&self) -> im::Vector<String> {
        self.ui_icon_theme_list.clone()
    }

    pub fn terminal_font_family(&self) -> &str {
        if self.terminal.font_family.is_empty() {
            self.editor.font_family.as_str()
        } else {
            self.terminal.font_family.as_str()
        }
    }

    pub fn terminal_font_size(&self) -> usize {
        if self.terminal.font_size > 0 {
            self.terminal.font_size
        } else {
            self.editor.font_size()
        }
    }

    pub fn terminal_line_height(&self) -> usize {
        let font_size = self.terminal_font_size();

        if self.terminal.line_height > 0.0 {
            let line_height = if self.terminal.line_height < SCALE_OR_SIZE_LIMIT {
                self.terminal.line_height * font_size as f64
            } else {
                self.terminal.line_height
            };

            // Prevent overlapping lines
            (line_height.round() as usize).max(font_size)
        } else {
            self.editor.line_height()
        }
    }

    pub fn terminal_get_color(
        &self,
        color: &alacritty_terminal::vte::ansi::Color,
        colors: &alacritty_terminal::term::color::Colors,
    ) -> Color {
        match color {
            alacritty_terminal::vte::ansi::Color::Named(color) => {
                self.terminal_get_named_color(color)
            }
            alacritty_terminal::vte::ansi::Color::Spec(rgb) => {
                Color::rgb8(rgb.r, rgb.g, rgb.b)
            }
            alacritty_terminal::vte::ansi::Color::Indexed(index) => {
                if let Some(rgb) = colors[*index as usize] {
                    return Color::rgb8(rgb.r, rgb.g, rgb.b);
                }
                const NAMED_COLORS: [alacritty_terminal::vte::ansi::NamedColor; 16] = [
                    alacritty_terminal::vte::ansi::NamedColor::Black,
                    alacritty_terminal::vte::ansi::NamedColor::Red,
                    alacritty_terminal::vte::ansi::NamedColor::Green,
                    alacritty_terminal::vte::ansi::NamedColor::Yellow,
                    alacritty_terminal::vte::ansi::NamedColor::Blue,
                    alacritty_terminal::vte::ansi::NamedColor::Magenta,
                    alacritty_terminal::vte::ansi::NamedColor::Cyan,
                    alacritty_terminal::vte::ansi::NamedColor::White,
                    alacritty_terminal::vte::ansi::NamedColor::BrightBlack,
                    alacritty_terminal::vte::ansi::NamedColor::BrightRed,
                    alacritty_terminal::vte::ansi::NamedColor::BrightGreen,
                    alacritty_terminal::vte::ansi::NamedColor::BrightYellow,
                    alacritty_terminal::vte::ansi::NamedColor::BrightBlue,
                    alacritty_terminal::vte::ansi::NamedColor::BrightMagenta,
                    alacritty_terminal::vte::ansi::NamedColor::BrightCyan,
                    alacritty_terminal::vte::ansi::NamedColor::BrightWhite,
                ];
                if (*index as usize) < NAMED_COLORS.len() {
                    self.terminal_get_named_color(&NAMED_COLORS[*index as usize])
                } else {
                    self.terminal.indexed_colors.get(index).cloned().unwrap()
                }
            }
        }
    }

    fn terminal_get_named_color(
        &self,
        color: &alacritty_terminal::vte::ansi::NamedColor,
    ) -> Color {
        let (color, alpha) = match color {
            alacritty_terminal::vte::ansi::NamedColor::Cursor => {
                (LapceColor::TERMINAL_CURSOR, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Foreground => {
                (LapceColor::TERMINAL_FOREGROUND, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Background => {
                (LapceColor::TERMINAL_BACKGROUND, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Blue => {
                (LapceColor::TERMINAL_BLUE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Green => {
                (LapceColor::TERMINAL_GREEN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Yellow => {
                (LapceColor::TERMINAL_YELLOW, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Red => {
                (LapceColor::TERMINAL_RED, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::White => {
                (LapceColor::TERMINAL_WHITE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Black => {
                (LapceColor::TERMINAL_BLACK, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Cyan => {
                (LapceColor::TERMINAL_CYAN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Magenta => {
                (LapceColor::TERMINAL_MAGENTA, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightBlue => {
                (LapceColor::TERMINAL_BRIGHT_BLUE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightGreen => {
                (LapceColor::TERMINAL_BRIGHT_GREEN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightYellow => {
                (LapceColor::TERMINAL_BRIGHT_YELLOW, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightRed => {
                (LapceColor::TERMINAL_BRIGHT_RED, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightWhite => {
                (LapceColor::TERMINAL_BRIGHT_WHITE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightBlack => {
                (LapceColor::TERMINAL_BRIGHT_BLACK, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightCyan => {
                (LapceColor::TERMINAL_BRIGHT_CYAN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightMagenta => {
                (LapceColor::TERMINAL_BRIGHT_MAGENTA, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightForeground => {
                (LapceColor::TERMINAL_FOREGROUND, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimBlack => {
                (LapceColor::TERMINAL_BLACK, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimRed => {
                (LapceColor::TERMINAL_RED, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimGreen => {
                (LapceColor::TERMINAL_GREEN, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimYellow => {
                (LapceColor::TERMINAL_YELLOW, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimBlue => {
                (LapceColor::TERMINAL_BLUE, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimMagenta => {
                (LapceColor::TERMINAL_MAGENTA, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimCyan => {
                (LapceColor::TERMINAL_CYAN, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimWhite => {
                (LapceColor::TERMINAL_WHITE, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimForeground => {
                (LapceColor::TERMINAL_FOREGROUND, 0.66)
            }
        };
        self.color(color).with_alpha_factor(alpha)
    }

    /// Get the dropdown information for the specific setting, used for the settings UI.
    /// This should aim to efficiently return the data, because it is used to determine whether to
    /// update the dropdown items.
    pub fn get_dropdown_info(&self, kind: &str, key: &str) -> Option<DropdownInfo> {
        match (kind, key) {
            (core::CONFIG_KEY, color_theme::CONFIG_KEY) => Some(DropdownInfo {
                active_index: self
                    .color_theme_list
                    .iter()
                    .position(|s| s == &self.color_theme.name)
                    .unwrap_or(0),
                items: self.color_theme_list.clone(),
            }),
            (core::CONFIG_KEY, "file-icon-theme") => Some(DropdownInfo {
                active_index: self
                    .file_icon_theme_list
                    .iter()
                    .position(|s| s == &self.file_icon_theme.name)
                    .unwrap_or(0),
                items: self.file_icon_theme_list.clone(),
            }),
            (core::CONFIG_KEY, "ui-icon-theme") => Some(DropdownInfo {
                active_index: self
                    .ui_icon_theme_list
                    .iter()
                    .position(|s| s == &self.file_icon_theme.name)
                    .unwrap_or(0),
                items: self.ui_icon_theme_list.clone(),
            }),
            ("editor", "wrap-style") => Some(DropdownInfo {
                // TODO: it would be better to have the text not be the default kebab-case when
                // displayed in settings, but we would need to map back from the dropdown's value
                // or index.
                active_index: self
                    .wrap_style_list
                    .iter()
                    .flat_map(|w| WrapStyle::try_from_str(w))
                    .position(|w| w == self.editor.wrap_style)
                    .unwrap_or(0),
                items: self.wrap_style_list.clone(),
            }),
            ("ui", "tab-close-button") => Some(DropdownInfo {
                active_index: self.ui.tab_close_button as usize,
                items: ui::TabCloseButton::VARIANTS
                    .iter()
                    .map(|s| s.to_string())
                    .sorted()
                    .collect(),
            }),
            ("ui", "tab-separator-height") => Some(DropdownInfo {
                active_index: self.ui.tab_separator_height as usize,
                items: ui::TabSeparatorHeight::VARIANTS
                    .iter()
                    .map(|s| s.to_string())
                    .sorted()
                    .collect(),
            }),
            ("terminal", "default-profile") => Some(DropdownInfo {
                active_index: self
                    .terminal
                    .profiles
                    .iter()
                    .position(|(profile_name, _)| {
                        profile_name
                            == self
                                .terminal
                                .default_profile
                                .get(std::env::consts::OS)
                                .unwrap_or(&String::from("default"))
                    })
                    .unwrap_or(0),
                items: self.terminal.profiles.clone().into_keys().collect(),
            }),
            _ => None,
        }
    }

    fn get_file_table() -> Result<toml_edit::Document> {
        let path = Self::settings_file()?;
        let content = std::fs::read_to_string(path)?;
        let document: toml_edit::Document = content.parse()?;
        Ok(document)
    }

    pub fn reset_setting(parent: &str, key: &str) -> Result<()> {
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
            table = table
                .get_mut(key)
                .ok_or(anyhow!("No key `{key}` available on table"))?
                .as_table_mut()
                .ok_or(anyhow!("Failed to cast as mutable for `{key}`"))?;
        }

        table.remove(key);

        // Store
        let path = Self::settings_file()?;
        std::fs::write(path, main_table.to_string().as_bytes())?;

        Ok(())
    }

    /// Update the config file with the given edit.  
    /// This should be called whenever the configuration is changed, so that it is persisted.
    pub fn update_file(
        parent: &str,
        key: &str,
        value: toml_edit::Value,
    ) -> Result<()> {
        // TODO: This is a hack to fix the fact that terminal default profile is saved in a
        // different manner than other fields. As it is per-operating-system.
        // Thus we have to instead set the terminal.default-profile.{OS}
        // It would be better to not need a special hack.
        let (parent, key) = if parent == "terminal" && key == "default-profile" {
            ("terminal.default-profile", std::env::consts::OS)
        } else {
            (parent, key)
        };

        let mut main_table = Self::get_file_table()?;

        // Find the container table
        let mut table = main_table.as_table_mut();
        for key in parent.split('.') {
            if !table.contains_key(key) {
                table.insert(
                    key,
                    toml_edit::Item::Table(toml_edit::Table::default()),
                );
            }
            table = table
                .get_mut(key)
                .ok_or(anyhow!("No key `{key}` available on table"))?
                .as_table_mut()
                .ok_or(anyhow!("Failed to cast as mutable for `{key}`"))?;
        }

        // Update key
        table.insert(key, toml_edit::Item::Value(value));

        // Store
        let path = Self::settings_file()?;
        std::fs::write(path, main_table.to_string().as_bytes())?;

        Ok(())
    }
}
