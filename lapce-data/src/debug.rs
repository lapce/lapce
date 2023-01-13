use std::{fmt::Display, path::Path};

use druid::WidgetId;
use lapce_rpc::terminal::TermId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunDebugMode {
    Run,
    Debug,
}

impl Display for RunDebugMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RunDebugMode::Run => "Run",
            RunDebugMode::Debug => "Debug",
        };
        f.write_str(s)
    }
}

#[derive(Clone)]
pub struct RunDebugProcess {
    pub mode: RunDebugMode,
    pub name: String,
    pub command: String,
    pub stopped: bool,
}

#[derive(Deserialize, Serialize)]
pub struct RunDebugConfigs {
    pub configs: Vec<RunDebugConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct RunDebugConfig {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

pub fn run_configs(workspace: Option<&Path>) -> Option<RunDebugConfigs> {
    let workspace = workspace?;
    let run_toml = workspace.join(".lapce").join("run.toml");
    let content = std::fs::read_to_string(run_toml).ok()?;
    let configs: RunDebugConfigs = toml_edit::easy::from_str(&content).ok()?;
    Some(configs)
}

#[derive(Clone)]
pub struct RunDebugData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub active_term: Option<TermId>,
}

impl Default for RunDebugData {
    fn default() -> Self {
        Self::new()
    }
}

impl RunDebugData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            active_term: None,
        }
    }
}
