use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal},
};
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    config::LapceConfig, debug::RunDebugProcess, workspace::LapceWorkspace,
};

use super::data::TerminalData;

#[derive(Clone)]
pub struct TerminalTabData {
    pub active: RwSignal<usize>,
    pub terminals: RwSignal<im::Vector<TerminalData>>,
}

impl TerminalTabData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        proxy: ProxyRpcHandler,
        run_debug: Option<RunDebugProcess>,
        config: &LapceConfig,
    ) -> Self {
        let terminal_data =
            TerminalData::new(cx, workspace, proxy, run_debug, config);
        let terminals = im::vector![terminal_data];
        let terminals = create_rw_signal(cx.scope, terminals);
        let active = create_rw_signal(cx.scope, 0);
        Self { active, terminals }
    }
}
