use std::collections::HashMap;

use anyhow::Result;
use directories::ProjectDirs;
use druid::{
    piet::{PietText, Text, TextLayout, TextLayoutBuilder},
    theme, Color, Env, FontDescriptor, FontFamily, Key,
};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{data::hex_to_color, state::LapceWorkspace};

const default_settings: &'static str = include_str!("../../defaults/settings.toml");
const default_light_theme: &'static str =
    include_str!("../../defaults/light-theme.toml");
const default_dark_theme: &'static str =
    include_str!("../../defaults/dark-theme.toml");

pub struct LapceTheme {}

impl LapceTheme {
    pub const LAPCE_WARN: &'static str = "lapce.warn";
    pub const LAPCE_ERROR: &'static str = "lapce.error";
    pub const LAPCE_ACTIVE_TAB: &'static str = "lapce.active_tab";
    pub const LAPCE_INACTIVE_TAB: &'static str = "lapce.inactive_tab";
    pub const LAPCE_DROPDOWN_SHADOW: &'static str = "lapce.dropdown_shadow";
    pub const LAPCE_BORDER: &'static str = "lapce.border";

    pub const EDITOR_BACKGROUND: &'static str = "editor.background";
    pub const EDITOR_FOREGROUND: &'static str = "editor.foreground";
    pub const EDITOR_DIM: &'static str = "editor.dim";
    pub const EDITOR_CARET: &'static str = "editor.caret";
    pub const EDITOR_SELECTION: &'static str = "editor.selection";
    pub const EDITOR_CURRENT_LINE: &'static str = "editor.current_line";

    pub const PALETTE_BACKGROUND: &'static str = "palette.background";
    pub const PALETTE_CURRENT: &'static str = "palette.current";

    pub const COMPLETION_BACKGROUND: &'static str = "completion.background";
    pub const COMPLETION_CURRENT: &'static str = "completion.current";

    pub const PANEL_BACKGROUND: &'static str = "panel.background";
    pub const PANEL_CURRENT: &'static str = "panel.current";

    pub const STATUS_BACKGROUND: &'static str = "status.background";
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LapceConfig {
    pub color_theme: String,
    pub icon_theme: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EditorConfig {
    pub font_family: String,
    pub font_size: usize,
    pub line_height: usize,
}

impl EditorConfig {
    pub fn font_family(&self) -> FontFamily {
        FontFamily::new_unchecked(self.font_family.clone())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub lapce: LapceConfig,
    pub editor: EditorConfig,
    #[serde(skip)]
    pub theme: HashMap<String, Color>,
    #[serde(skip)]
    pub themes: HashMap<String, HashMap<String, Color>>,
}

impl Config {
    pub fn load(workspace: Option<LapceWorkspace>) -> Self {
        let mut settings_string = default_settings.to_string();
        let mut files = vec![];

        if let Some(proj_dirs) = ProjectDirs::from("", "", "Lapce") {
            let path = proj_dirs.config_dir().join("settings.toml");
            files.push(path);
        }

        if let Some(workspace) = workspace {
            match workspace.kind {
                crate::state::LapceWorkspaceType::Local => {
                    let path = workspace.path.join("./.lapce/settings.toml");
                    files.push(path);
                }
                crate::state::LapceWorkspaceType::RemoteSSH(_, _) => {}
            }
        }
        for f in files {
            if let Ok(content) = std::fs::read_to_string(f) {
                if content != "" {
                    settings_string += &content;
                }
            }
        }

        let config: toml::Value = toml::from_str(&settings_string).unwrap();
        let mut config: Config = config.try_into().unwrap();

        config.theme = get_theme(default_light_theme).unwrap();

        let mut themes = HashMap::new();
        themes.insert(
            "Lapce Light".to_string(),
            get_theme(default_light_theme).unwrap(),
        );
        themes.insert(
            "Lapce Dark".to_string(),
            get_theme(default_dark_theme).unwrap(),
        );
        config.themes = themes;

        config
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

    pub fn reload_env(&self, env: &mut Env) {
        env.set(theme::SCROLLBAR_RADIUS, 0.0);
        env.set(theme::SCROLLBAR_EDGE_WIDTH, 0.0);
        env.set(theme::SCROLLBAR_WIDTH, 10.0);
        env.set(theme::SCROLLBAR_PAD, 0.0);
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
}

fn get_theme(content: &str) -> Result<HashMap<String, Color>> {
    let theme_colors: HashMap<String, String> = toml::from_str(content)?;
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
