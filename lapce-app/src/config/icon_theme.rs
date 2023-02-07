use std::path::PathBuf;

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
