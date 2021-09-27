use std::collections::HashMap;

use druid::{theme, Color, Env, FontDescriptor, FontFamily, Key};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{data::hex_to_color, state::LapceWorkspace};

const default_settings: &'static str = include_str!("../../defaults/settings.toml");
const default_theme: &'static str = include_str!("../../defaults/light-theme.toml");

pub struct LapceTheme {}

impl LapceTheme {
    pub const EDITOR_BACKGROUND: &'static str = "editor.background";
    pub const EDITOR_FOREGROUND: &'static str = "editor.foreground";
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
        let mut files = vec!["/Users/Lulu/.lapce/settings.toml".to_string()];
        if let Some(workspace) = workspace {
            match workspace.kind {
                crate::state::LapceWorkspaceType::Local => {
                    let mut path = workspace.path.clone();
                    path.push("./.lapce/settings.toml");
                    files.push(path.to_str().unwrap().to_string());
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

        let theme_colors: HashMap<String, String> =
            toml::from_str(&default_theme).unwrap();
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

        config.theme = theme;

        let themes = HashMap::new();
        config.themes = themes;

        println!("{:?}", config);

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
