use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case", default)]
pub struct UIIconThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub use_editor_color: Option<bool>,
    pub icons: IndexMap<String, String>,
}
