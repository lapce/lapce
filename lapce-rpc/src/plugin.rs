use std::{path::PathBuf, process::Command};

use anyhow::{format_err, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RpcMessage;

#[derive(Eq, PartialEq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct PluginId(pub u64);

pub type PluginRpcMessage =
    RpcMessage<PluginRequest, PluginNotification, PluginResponse>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginNotification {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
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
    pub configuration: Option<Value>,
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

        let conf = match conf.as_object() {
            Some(val) => val,
            None => {
                return Err(format_err!(
                    "Empty configuration for plugin {}",
                    self.display_name
                ));
            }
        };

        let env = match conf.get("env_command") {
            Some(env) => env,
            // We do not print any error as no env is allowed.
            None => return Ok(vec![]),
        };

        let args = match env.as_str() {
            Some(arg) => arg,
            None => {
                return Err(format_err!(
                    "Plugin {}: env_command is not a string",
                    self.display_name
                ));
            }
        };

        let output = if cfg!(target_os = "windows") {
            Command::new("cmd").arg("/c").arg(args).output()
        } else {
            Command::new("sh").arg("-c").arg(args).output()
        };

        let output = match output {
            Ok(val) => val.stdout,
            Err(err) => {
                return Err(format_err!(
                    "Error during env command execution for plugin {}: {}",
                    self.name,
                    err
                ))
            }
        };

        let data = match String::from_utf8(output) {
            Ok(val) => val,
            Err(err) => {
                return Err(format_err!(
                    "Error during UTF-8 conversion for plugin {}: {}",
                    self.display_name,
                    err
                ))
            }
        };

        Ok(data
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.into(), v.into()))
            .collect::<Vec<(String, String)>>())
    }
}
