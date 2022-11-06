use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::counter::Counter;

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PluginId(pub u64);

impl PluginId {
    pub fn next() -> Self {
        static PLUGIN_ID_COUNTER: Counter = Counter::new();
        Self(PLUGIN_ID_COUNTER.next())
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct PluginConfiguration {
    #[serde(rename(deserialize = "type"))]
    pub kind: String,
    pub default: Value,
    pub description: String,
}

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct VoltInfo {
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub repository: Option<String>,
    pub wasm: bool,
    pub updated_at_ts: i64,
}

impl VoltInfo {
    pub fn id(&self) -> String {
        format!("{}.{}", self.author, self.name)
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct VoltActivation {
    pub language: Option<Vec<String>>,
    pub workspace_contains: Option<Vec<String>>,
}

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct VoltConfig {
    pub default: Value,
    pub description: String,
}

#[derive(Deserialize, Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct VoltMetadata {
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub icon: Option<String>,
    pub repository: Option<String>,
    pub wasm: Option<String>,
    pub color_themes: Option<Vec<String>>,
    pub icon_themes: Option<Vec<String>>,
    pub dir: Option<PathBuf>,
    pub activation: Option<VoltActivation>,
    pub config: Option<HashMap<String, VoltConfig>>,
}

impl VoltMetadata {
    pub fn id(&self) -> String {
        format!("{}.{}", self.author, self.name)
    }

    pub fn info(&self) -> VoltInfo {
        VoltInfo {
            name: self.name.clone(),
            version: self.version.clone(),
            display_name: self.display_name.clone(),
            author: self.author.clone(),
            description: self.description.clone(),
            repository: self.repository.clone(),
            wasm: self.wasm.is_some(),
            updated_at_ts: 0,
        }
    }
}
