use std::collections::HashMap;

use dap_types::types::Breakpoint;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::counter::Counter;

pub type Breakpoints = Vec<Breakpoint>;
pub type BreakpointsMapping = HashMap<Url, Vec<Breakpoint>>;

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ThreadId(pub u64);

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DapId(pub u64);

impl Default for DapId {
    fn default() -> Self {
        Self::next()
    }
}

impl DapId {
    pub fn next() -> Self {
        static DAP_ID_COUNTER: Counter = Counter::new();
        Self(DAP_ID_COUNTER.next())
    }
}

pub struct DapServer {
    pub program: String,
    pub arguments: Vec<String>,
    pub working_directory: Option<Url>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct RunDebugProgram {
    pub program: String,
    pub arguments: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct RunDebugConfig {
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub name: String,
    pub program: String,
    pub arguments: Option<Vec<String>>,
    pub working_directory: Option<String>,
    pub environment: Option<HashMap<String, String>>,
    pub prelaunch: Option<RunDebugProgram>,
    #[serde(skip)]
    pub debug_command: Option<Vec<String>>,
    #[serde(skip)]
    pub dap_id: DapId,
}
