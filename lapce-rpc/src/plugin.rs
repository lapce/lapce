use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Eq, PartialEq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct PluginId(pub u64);

#[derive(Deserialize, Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub repository: String,
    pub wasm: String,
    pub dir: Option<PathBuf>,
    pub configuration: Option<Value>,
}

#[derive(Serialize, Clone)]
pub struct PluginInfo {
    pub arch: String,
    pub os: String,
    pub configuration: Option<Value>,
}

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct PluginConfiguration {
    pub language_id: String,
    pub env_command: String,
    pub options: Option<Value>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PluginOptions {
    pub binary_args: Vec<String>,
}
