use std::{fmt::Display, path::Path, time::Instant};

use druid::WidgetId;
use lapce_rpc::{
    dap_types::{DapId, RunDebugConfig, ThreadId},
    terminal::TermId,
};
use serde::{Deserialize, Serialize};

const DEFAULT_RUN_TOML: &str = include_str!("../../defaults/run.toml");

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

#[derive(Clone, Debug)]
pub enum RunDebugAction {
    Run(RunAction),
    Debug(DebugAction),
}

#[derive(Clone, Debug)]
pub enum DebugAction {
    Continue,
    Restart,
    Stop,
    Close,
}

#[derive(Clone, Debug)]
pub enum RunAction {
    Restart,
    Stop,
    Close,
}

#[derive(Clone)]
pub struct RunDebugProcess {
    pub mode: RunDebugMode,
    pub config: RunDebugConfig,
    pub stopped: bool,
    pub created: Instant,
}

#[derive(Deserialize, Serialize)]
pub struct RunDebugConfigs {
    pub configs: Vec<RunDebugConfig>,
}

pub fn run_configs(workspace: Option<&Path>) -> Option<RunDebugConfigs> {
    let workspace = workspace?;
    let run_toml = workspace.join(".lapce").join("run.toml");
    if !run_toml.exists() {
        if !workspace.join(".lapce").exists() {
            let _ = std::fs::create_dir_all(workspace.join(".lapce"));
        }
        let _ = std::fs::write(&run_toml, DEFAULT_RUN_TOML);
        return None;
    }
    let content = std::fs::read_to_string(run_toml).ok()?;
    let configs: RunDebugConfigs = toml_edit::easy::from_str(&content).ok()?;
    Some(configs)
}

#[derive(Clone)]
pub struct DapData {
    pub term_id: TermId,
    pub dap_id: DapId,
    pub stopped: bool,
    pub thread_id: Option<ThreadId>,
}

impl DapData {
    pub fn new(dap_id: DapId, term_id: TermId) -> Self {
        Self {
            term_id,
            dap_id,
            stopped: false,
            thread_id: None,
        }
    }
}

#[derive(Clone)]
pub struct RunDebugData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub active_term: Option<TermId>,
    pub daps: im::HashMap<DapId, DapData>,
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
            daps: im::HashMap::new(),
        }
    }
}
