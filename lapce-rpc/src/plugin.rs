use std::{collections::HashMap, fmt, path::PathBuf, process::Command};

use anyhow::{format_err, Error};
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
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

pub enum PluginResponse {}

#[derive(Deserialize, Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub repository: String,
    pub enabled: Option<bool>,
    pub wasm: Option<String>,
    pub themes: Option<Vec<String>>,
    pub dir: Option<PathBuf>,
    pub configuration: Option<HashMap<String, PluginConfiguration>>,
}

#[derive(Serialize, Clone)]
pub struct PluginInfo {
    pub arch: String,
    pub os: String,
    pub configuration: Option<Value>,
}

impl PluginDescription {
    pub fn get_plugin_env(&self) -> Result<Vec<(String, String)>, Error> {
        let conf = match &self.configuration {
            Some(val) => val,
            None => {
                return Err(format_err!(
                    "Empty configuration for plugin {}",
                    self.display_name
                ));
            }
        };

        // let conf = match conf.as_object() {
        //     Some(val) => val,
        //     None => {
        //         return Err(format_err!(
        //             "Empty configuration for plugin {}",
        //             self.display_name
        //         ));
        //     }
        // };

        let env = match conf.get("env_command") {
            Some(env) => env,
            // We do not print any error as no env is allowed.
            None => return Ok(vec![]),
        };

        // let args = match env.as_str() {
        //     Some(arg) => arg,
        //     None => {
        //         return Err(format_err!(
        //             "Plugin {}: env_command is not a string",
        //             self.display_name
        //         ));
        //     }
        // };

        // let output = if cfg!(target_os = "windows") {
        //     Command::new("cmd").arg("/c").arg(args).output()
        // } else {
        //     Command::new("sh").arg("-c").arg(args).output()
        // };

        // let output = match output {
        //     Ok(val) => val.stdout,
        //     Err(err) => {
        //         return Err(format_err!(
        //             "Error during env command execution for plugin {}: {}",
        //             self.name,
        //             err
        //         ))
        //     }
        // };

        // let data = match String::from_utf8(output) {
        //     Ok(val) => val,
        //     Err(err) => {
        //         return Err(format_err!(
        //             "Error during UTF-8 conversion for plugin {}: {}",
        //             self.display_name,
        //             err
        //         ))
        //     }
        // };

        // Ok(data
        //     .lines()
        //     .filter_map(|l| l.split_once('='))
        //     .map(|(k, v)| (k.into(), v.into()))
        //     .collect::<Vec<(String, String)>>())

        Err(format_err!(
            "Empty configuration for plugin {}",
            self.display_name
        ))
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
#[serde(rename_all = "kebab-case")]
pub struct VoltInfo {
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub meta: String,
}

impl VoltInfo {
    pub fn id(&self) -> String {
        format!("{}.{}", self.author, self.name)
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct VoltMetadata {
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub wasm: Option<String>,
    pub themes: Option<Vec<String>>,
    pub dir: Option<PathBuf>,
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
            meta: "".to_string(),
        }
    }
}
