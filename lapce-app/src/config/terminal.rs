use std::{collections::HashMap, sync::Arc};

use floem::peniko::Color;
use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

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
    pub line_height: f64,
    #[field_names(desc = "Profiles available in terminal pane")]
    pub profiles: HashMap<String, TerminalProfile>,
    #[field_names(desc = "Default profile for each platform")]
    pub default_profile: HashMap<String, String>,

    #[serde(skip)]
    #[field_names(skip)]
    pub indexed_colors: Arc<HashMap<u8, Color>>,
}

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct TerminalProfile {
    #[field_names(desc = "Command to execute when launching terminal")]
    pub command: Option<String>,
    #[field_names(desc = "Arguments passed to command")]
    pub arguments: Option<Vec<String>>,
    #[field_names(desc = "Command to execute when launching terminal")]
    pub workdir: Option<std::path::PathBuf>,
    #[field_names(desc = "Arguments passed to command")]
    pub environment: Option<HashMap<String, String>>,
}

impl TerminalConfig {
    pub fn get_indexed_colors(&mut self) {
        let mut indexed_colors = HashMap::new();
        // Build colors.
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    // Override colors 16..232 with the config (if present).
                    let index = 16 + r * 36 + g * 6 + b;
                    let color = Color::rgb8(
                        if r == 0 { 0 } else { r * 40 + 55 },
                        if g == 0 { 0 } else { g * 40 + 55 },
                        if b == 0 { 0 } else { b * 40 + 55 },
                    );
                    indexed_colors.insert(index, color);
                }
            }
        }

        let index: u8 = 232;

        for i in 0..24 {
            // Override colors 232..256 with the config (if present).

            let value = i * 10 + 8;
            indexed_colors.insert(index + i, Color::rgb8(value, value, value));
        }

        self.indexed_colors = Arc::new(indexed_colors);
    }

    pub fn get_default_profile(
        &self,
    ) -> Option<lapce_rpc::terminal::TerminalProfile> {
        let Some(profile) = self.profiles.get(
            self.default_profile
                .get(&std::env::consts::OS.to_string())
                .unwrap_or(&String::from("default")),
        ) else {
            return None;
        };
        let workdir = if let Some(workdir) = &profile.workdir {
            url::Url::parse(&workdir.display().to_string()).ok()
        } else {
            None
        };

        let profile = profile.clone();

        Some(lapce_rpc::terminal::TerminalProfile {
            name: std::env::consts::OS.to_string(),
            command: profile.command,
            arguments: profile.arguments,
            workdir,
            environment: profile.environment,
        })
    }
}
