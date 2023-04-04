use std::time::Instant;

use lapce_rpc::dap_types::RunDebugConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunDebugMode {
    Run,
    Debug,
}

#[derive(Clone)]
pub struct RunDebugProcess {
    pub mode: RunDebugMode,
    pub config: RunDebugConfig,
    pub stopped: bool,
    pub created: Instant,
}
