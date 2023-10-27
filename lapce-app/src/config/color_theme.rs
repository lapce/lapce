use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use floem::peniko::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub enum ThemeColorPreference {
    #[default]
    Light,
    Dark,
    HighContrastDark,
    HighContrastLight,
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
    pub fn get(&self, name: &str) -> Option<&Color> {
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

#[derive(Debug, Clone, Default)]
pub struct ThemeColor {
    pub color_preference: ThemeColorPreference,
    pub base: ThemeBaseColor,
    pub syntax: HashMap<String, Color>,
    pub ui: HashMap<String, Color>,
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
            white: Color::parse(&self.white).unwrap_or(default.white),
            black: Color::parse(&self.black).unwrap_or(default.black),
            grey: Color::parse(&self.grey).unwrap_or(default.grey),
            blue: Color::parse(&self.blue).unwrap_or(default.blue),
            red: Color::parse(&self.red).unwrap_or(default.red),
            yellow: Color::parse(&self.yellow).unwrap_or(default.yellow),
            orange: Color::parse(&self.orange).unwrap_or(default.orange),
            green: Color::parse(&self.green).unwrap_or(default.green),
            purple: Color::parse(&self.purple).unwrap_or(default.purple),
            cyan: Color::parse(&self.cyan).unwrap_or(default.cyan),
            magenta: Color::parse(&self.magenta).unwrap_or(default.magenta),
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

    pub fn key_values(&self) -> BTreeMap<String, String> {
        [
            ("white".to_string(), self.white.clone()),
            ("black".to_string(), self.black.clone()),
            ("grey".to_string(), self.grey.clone()),
            ("blue".to_string(), self.blue.clone()),
            ("red".to_string(), self.red.clone()),
            ("yellow".to_string(), self.yellow.clone()),
            ("orange".to_string(), self.orange.clone()),
            ("green".to_string(), self.green.clone()),
            ("purple".to_string(), self.purple.clone()),
            ("cyan".to_string(), self.cyan.clone()),
            ("magenta".to_string(), self.magenta.clone()),
        ]
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ColorThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub high_contrast: Option<bool>,
    pub base: ThemeBaseConfig,
    pub syntax: BTreeMap<String, String>,
    pub ui: BTreeMap<String, String>,
}

impl ColorThemeConfig {
    fn resolve_color(
        colors: &BTreeMap<String, String>,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        colors
            .iter()
            .map(|(name, hex)| {
                let color = if let Some(stripped) = hex.strip_prefix('$') {
                    base.get(stripped).cloned()
                } else {
                    Color::parse(hex)
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

    pub(super) fn resolve_ui_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.ui, base, default)
    }

    pub(super) fn resolve_syntax_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.syntax, base, default)
    }
}
