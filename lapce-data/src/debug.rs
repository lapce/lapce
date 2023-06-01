use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    path::{Path, PathBuf},
    time::Instant,
};

use druid::WidgetId;
use lapce_rpc::{
    dap_types::{
        self, DapId, RunDebugConfig, SourceBreakpoint, StackFrame, Stopped, ThreadId,
    },
    terminal::TermId,
};
use serde::{Deserialize, Serialize};

use crate::db::WorkspaceInfo;

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
    Pause,
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

#[derive(Clone, PartialEq)]
pub struct StackTraceData {
    pub expanded: bool,
    pub frames: Vec<StackFrame>,
    pub frames_shown: usize,
}

#[derive(Clone)]
pub struct LapceBreakpoint {
    pub id: Option<usize>,
    pub verified: bool,
    pub message: Option<String>,
    pub line: usize,
    pub offset: usize,
    pub dap_line: Option<usize>,
}

#[derive(Clone)]
pub struct DapData {
    pub term_id: TermId,
    pub dap_id: DapId,
    pub stopped: bool,
    pub thread_id: Option<ThreadId>,
    pub stack_frames: BTreeMap<ThreadId, StackTraceData>,
}

impl DapData {
    pub fn new(dap_id: DapId, term_id: TermId) -> Self {
        Self {
            term_id,
            dap_id,
            stopped: false,
            thread_id: None,
            stack_frames: BTreeMap::new(),
        }
    }

    pub fn stopped(
        &mut self,
        stopped: &Stopped,
        stack_frames: &HashMap<ThreadId, Vec<StackFrame>>,
    ) {
        self.stopped = true;
        if self.thread_id.is_none() {
            self.thread_id = Some(stopped.thread_id.unwrap_or_default());
        }
        for (thread_id, frames) in stack_frames {
            let main_thread_expanded = self.thread_id.as_ref() == Some(thread_id);
            if let Some(current) = self.stack_frames.get_mut(thread_id) {
                current.frames = frames.to_owned();
                current.expanded |= main_thread_expanded;
            } else {
                self.stack_frames.insert(
                    *thread_id,
                    StackTraceData {
                        expanded: main_thread_expanded,
                        frames: frames.to_owned(),
                        frames_shown: 20,
                    },
                );
            }
        }
        self.stack_frames
            .retain(|thread_id, _| stack_frames.contains_key(thread_id));
    }
}

#[derive(Clone)]
pub struct RunDebugData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub active_term: Option<TermId>,
    pub daps: im::HashMap<DapId, DapData>,
    pub breakpoints: BTreeMap<PathBuf, Vec<LapceBreakpoint>>,
}

impl RunDebugData {
    pub fn new(workspace_info: Option<&WorkspaceInfo>) -> Self {
        let breakpoints = workspace_info
            .and_then(|info| info.breakpoints.as_ref())
            .map(|breakpoints| {
                breakpoints
                    .iter()
                    .map(|(path, breakpoints)| {
                        (
                            path.to_path_buf(),
                            breakpoints
                                .iter()
                                .map(|line| LapceBreakpoint {
                                    id: None,
                                    verified: false,
                                    line: *line,
                                    offset: 0,
                                    message: None,
                                    dap_line: None,
                                })
                                .collect(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            active_term: None,
            daps: im::HashMap::new(),
            breakpoints,
        }
    }

    pub fn source_breakpoints(&self) -> HashMap<PathBuf, Vec<SourceBreakpoint>> {
        self.breakpoints
            .iter()
            .map(|(path, breakpoints)| {
                (
                    path.to_path_buf(),
                    breakpoints
                        .iter()
                        .map(|b| SourceBreakpoint {
                            line: b.line + 1,
                            column: None,
                            condition: None,
                            hit_condition: None,
                            log_message: None,
                        })
                        .collect(),
                )
            })
            .collect()
    }

    pub fn set_breakpoints_resp(
        &mut self,
        path: &PathBuf,
        dap_breakpoints: &Vec<dap_types::Breakpoint>,
    ) {
        println!("set dap breakpoints {dap_breakpoints:?}");
        if let Some(breakpoints) = self.breakpoints.get_mut(path) {
            for (breakpoint, dap_breakpoint) in
                breakpoints.iter_mut().zip(dap_breakpoints)
            {
                breakpoint.id = dap_breakpoint.id;
                breakpoint.verified = dap_breakpoint.verified;
                breakpoint.message = dap_breakpoint.message.clone();
                breakpoint.dap_line =
                    dap_breakpoint.line.map(|l| l.saturating_sub(1));
            }
        }
    }
}
