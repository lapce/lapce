use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct IconThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub use_editor_color: Option<bool>,
    pub ui: IndexMap<String, String>,
    pub foldername: IndexMap<String, String>,
    pub filename: IndexMap<String, String>,
    pub extension: IndexMap<String, String>,
}

impl IconThemeConfig {
    pub fn resolve_path_to_icon(&self, path: &Path) -> Option<PathBuf> {
        if let Some((_, icon)) = self.filename.get_key_value(
            path.file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default(),
        ) {
            Some(self.path.join(icon))
        } else if let Some((_, icon)) = self.extension.get_key_value(
            path.extension()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default(),
        ) {
            Some(self.path.join(icon))
        } else {
            None
        }
    }
}
