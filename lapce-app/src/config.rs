use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use floem::peniko::Color;
use lapce_core::directory::Directory;
use lapce_proxy::plugin::wasi::find_all_volts;
use lapce_rpc::plugin::VoltID;
use lsp_types::{CompletionItemKind, SymbolKind};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Deserialize;

use crate::workspace::{LapceWorkspace, LapceWorkspaceType};

use self::{
    color::LapceColor,
    color_theme::{ColorThemeConfig, ThemeColor, ThemeColorPreference},
    core::CoreConfig,
    editor::EditorConfig,
    icon::LapceIcons,
    icon_theme::IconThemeConfig,
    svg::SvgStore,
    terminal::TerminalConfig,
    ui::UIConfig,
};

pub mod color;
mod color_theme;
mod core;
mod editor;
pub mod icon;
mod icon_theme;
pub mod svg;
mod terminal;
mod ui;
mod watcher;

pub const LOGO: &str = include_str!("../../extra/images/logo.svg");
const DEFAULT_SETTINGS: &str = include_str!("../../defaults/settings.toml");
const DEFAULT_LIGHT_THEME: &str = include_str!("../../defaults/light-theme.toml");
const DEFAULT_DARK_THEME: &str = include_str!("../../defaults/dark-theme.toml");
const DEFAULT_ICON_THEME: &str = include_str!("../../defaults/icon-theme.toml");

static DEFAULT_CONFIG: Lazy<config::Config> = Lazy::new(LapceConfig::default_config);
static DEFAULT_LAPCE_CONFIG: Lazy<LapceConfig> =
    Lazy::new(LapceConfig::default_lapce_config);

#[derive(Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct LapceConfig {
    #[serde(skip)]
    pub id: u64,
    pub core: CoreConfig,
    pub ui: UIConfig,
    pub editor: EditorConfig,
    pub terminal: TerminalConfig,
    pub color_theme: ColorThemeConfig,
    pub icon_theme: IconThemeConfig,
    #[serde(flatten)]
    pub plugins: HashMap<String, HashMap<String, serde_json::Value>>,
    #[serde(skip)]
    pub default_color_theme: ColorThemeConfig,
    #[serde(skip)]
    pub default_icon_theme: IconThemeConfig,
    #[serde(skip)]
    pub color: ThemeColor,
    #[serde(skip)]
    pub available_color_themes: HashMap<String, (String, config::Config)>,
    #[serde(skip)]
    pub available_icon_themes:
        HashMap<String, (String, config::Config, Option<PathBuf>)>,
    // #[serde(skip)]
    // tab_layout_info: Arc<RwLock<HashMap<(FontFamily, usize), f64>>>,
    #[serde(skip)]
    svg_store: Arc<RwLock<SvgStore>>,
    /// A list of the themes that are available. This is primarily for populating
    /// the theme picker, and serves as a cache.
    #[serde(skip)]
    color_theme_list: Vec<String>,
    #[serde(skip)]
    icon_theme_list: Vec<String>,
}

impl LapceConfig {
    pub fn load(workspace: &LapceWorkspace, disabled_volts: &[VoltID]) -> Self {
        let config = Self::merge_config(workspace, None, None);
        let mut lapce_config: LapceConfig = config
            .try_deserialize()
            .unwrap_or_else(|_| DEFAULT_LAPCE_CONFIG.clone());

        lapce_config.available_color_themes =
            Self::load_color_themes(disabled_volts);
        lapce_config.available_icon_themes = Self::load_icon_themes(disabled_volts);
        lapce_config.resolve_theme(workspace);

        lapce_config.color_theme_list = lapce_config
            .available_color_themes
            .values()
            .map(|(name, _)| name.clone())
            .collect();
        lapce_config.color_theme_list.sort();

        lapce_config.icon_theme_list = lapce_config
            .available_icon_themes
            .values()
            .map(|(name, _, _)| name.clone())
            .collect();
        lapce_config.icon_theme_list.sort();

        lapce_config
    }

    fn merge_config(
        workspace: &LapceWorkspace,
        color_theme_config: Option<config::Config>,
        icon_theme_config: Option<config::Config>,
    ) -> config::Config {
        let mut config = DEFAULT_CONFIG.clone();
        if let Some(theme) = color_theme_config {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(theme) = icon_theme_config {
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
            LapceWorkspaceType::RemoteSSH(_) => {}
            LapceWorkspaceType::RemoteWSL => {}
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

    fn default_lapce_config() -> LapceConfig {
        let mut default_lapce_config: LapceConfig =
            DEFAULT_CONFIG.clone().try_deserialize().unwrap();
        default_lapce_config.resolve_colors(None);
        default_lapce_config
    }

    fn resolve_theme(&mut self, workspace: &LapceWorkspace) {
        let mut default_lapce_config = DEFAULT_LAPCE_CONFIG.clone();
        if let Some((_, color_theme_config)) = self
            .available_color_themes
            .get(&self.core.color_theme.to_lowercase())
        {
            if let Ok(mut theme_lapce_config) = config::Config::builder()
                .add_source(DEFAULT_CONFIG.clone())
                .add_source(color_theme_config.clone())
                .build()
                .and_then(|theme| theme.try_deserialize::<LapceConfig>())
            {
                theme_lapce_config.resolve_colors(Some(&default_lapce_config));
                default_lapce_config = theme_lapce_config;
            }
        }

        let color_theme_config = self
            .available_color_themes
            .get(&self.core.color_theme.to_lowercase())
            .map(|(_, config)| config);

        let icon_theme_config = self
            .available_icon_themes
            .get(&self.core.icon_theme.to_lowercase())
            .map(|(_, config, _)| config);

        let icon_theme_path = self
            .available_icon_themes
            .get(&self.core.icon_theme.to_lowercase())
            .map(|(_, _, path)| path);

        if color_theme_config.is_some() || icon_theme_config.is_some() {
            if let Ok(new) = Self::merge_config(
                workspace,
                color_theme_config.cloned(),
                icon_theme_config.cloned(),
            )
            .try_deserialize::<LapceConfig>()
            {
                self.core = new.core;
                self.ui = new.ui;
                self.editor = new.editor;
                self.terminal = new.terminal;
                self.color_theme = new.color_theme;
                self.icon_theme = new.icon_theme;
                if let Some(icon_theme_path) = icon_theme_path {
                    self.icon_theme.path =
                        icon_theme_path.clone().unwrap_or_default();
                }
                self.plugins = new.plugins;
            }
        }
        self.resolve_colors(Some(&default_lapce_config));
        self.default_color_theme = default_lapce_config.color_theme.clone();
        self.default_icon_theme = default_lapce_config.icon_theme.clone();
        self.update_id();
    }

    fn load_color_themes(
        disabled_volts: &[VoltID],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes = Self::load_local_themes().unwrap_or_default();

        for (key, theme) in Self::load_plugin_color_themes(disabled_volts) {
            themes.insert(key, theme);
        }

        let (name, theme) =
            Self::load_color_theme_from_str(DEFAULT_LIGHT_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));
        let (name, theme) =
            Self::load_color_theme_from_str(DEFAULT_DARK_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));

        themes
    }

    /// Get the color by the name from the current theme if it exists
    /// Otherwise, get the color from the base them
    /// # Panics
    /// If the color was not able to be found in either theme, which may be indicative that
    /// it is misspelled or needs to be added to the base-theme.
    pub fn get_color(&self, name: &str) -> &Color {
        self.color
            .ui
            .get(name)
            .unwrap_or_else(|| panic!("Key not found: {name}"))
    }

    /// Retrieve a color value whose key starts with "style."
    pub fn get_style_color(&self, name: &str) -> Option<&Color> {
        self.color.syntax.get(name)
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

        self.get_style_color(theme_str).cloned()
    }

    fn resolve_colors(&mut self, default_config: Option<&LapceConfig>) {
        self.color.base = self
            .color_theme
            .base
            .resolve(default_config.map(|c| &c.color.base));
        self.color.ui = self
            .color_theme
            .resolve_ui_color(&self.color.base, default_config.map(|c| &c.color.ui));
        self.color.syntax = self.color_theme.resolve_syntax_color(
            &self.color.base,
            default_config.map(|c| &c.color.syntax),
        );

        let fg = self.get_color(LapceColor::EDITOR_FOREGROUND);
        let bg = self.get_color(LapceColor::EDITOR_BACKGROUND);
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

    fn load_local_themes() -> Option<HashMap<String, (String, config::Config)>> {
        let themes_folder = Directory::themes_directory()?;
        let themes: HashMap<String, (String, config::Config)> =
            std::fs::read_dir(themes_folder)
                .ok()?
                .filter_map(|entry| {
                    entry
                        .ok()
                        .and_then(|entry| Self::load_color_theme(&entry.path()))
                })
                .collect();
        Some(themes)
    }

    fn load_color_theme(path: &Path) -> Option<(String, (String, config::Config))> {
        if !path.is_file() {
            return None;
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .ok()?;
        let table = config.get_table("color-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name.to_lowercase(), (name, config)))
    }

    fn load_color_theme_from_str(s: &str) -> Option<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()
            .ok()?;
        let table = config.get_table("color-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, config))
    }

    fn load_icon_themes(
        disabled_volts: &[VoltID],
    ) -> HashMap<String, (String, config::Config, Option<PathBuf>)> {
        let mut themes = HashMap::new();

        for (key, (name, theme, path)) in
            Self::load_plugin_icon_themes(disabled_volts)
        {
            themes.insert(key, (name, theme, Some(path)));
        }

        let (name, theme) =
            Self::load_icon_theme_from_str(DEFAULT_ICON_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme, None));

        themes
    }

    fn load_icon_theme_from_str(s: &str) -> Option<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()
            .ok()?;
        let table = config.get_table("icon-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, config))
    }

    fn load_plugin_color_themes(
        disabled_volts: &[VoltID],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes: HashMap<String, (String, config::Config)> = HashMap::new();
        for meta in find_all_volts() {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.color_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Some((key, theme)) =
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
    ) -> HashMap<String, (String, config::Config, PathBuf)> {
        let mut themes: HashMap<String, (String, config::Config, PathBuf)> =
            HashMap::new();
        for meta in find_all_volts() {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.icon_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Some((key, theme)) =
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
    ) -> Option<(String, (String, config::Config, PathBuf))> {
        if !path.is_file() {
            return None;
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .ok()?;
        let table = config.get_table("icon-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((
            name.to_lowercase(),
            (name, config, path.parent().unwrap().to_path_buf()),
        ))
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

    pub fn ui_svg(&self, icon: &'static str) -> String {
        let svg = self.icon_theme.ui.get(icon).and_then(|path| {
            let path = self.icon_theme.path.join(path);
            self.svg_store.write().get_svg_on_disk(&path)
        });

        svg.unwrap_or_else(|| {
            let name = self.default_icon_theme.ui.get(icon).unwrap();
            self.svg_store.write().get_default_svg(name)
        })
    }

    pub fn file_svg(&self, path: &Path) -> (String, Option<&Color>) {
        let svg = self
            .icon_theme
            .resolve_path_to_icon(path)
            .and_then(|p| self.svg_store.write().get_svg_on_disk(&p));
        if let Some(svg) = svg {
            let color = if self.icon_theme.use_editor_color.unwrap_or(false) {
                Some(self.get_color(LapceColor::LAPCE_ICON_ACTIVE))
            } else {
                None
            };
            (svg, color)
        } else {
            (
                self.ui_svg(LapceIcons::FILE),
                Some(self.get_color(LapceColor::LAPCE_ICON_ACTIVE)),
            )
        }
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
}
