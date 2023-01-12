use std::path::Path;

use druid::WidgetId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct RunConfigs {
    pub configs: Vec<RunConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct RunConfig {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

pub fn run_configs(workspace: Option<&Path>) -> Option<RunConfigs> {
    let workspace = workspace?;
    let run_toml = workspace.join(".lapce").join("run.toml");
    println!("run toml is  {run_toml:?}");
    let content = std::fs::read_to_string(run_toml).ok()?;
    let configs: RunConfigs = toml_edit::easy::from_str(&content).ok()?;
    Some(configs)
}

pub struct DebugData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
}

impl DebugData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
        }
    }
}
