use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal},
};
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    config::LapceConfig, debug::RunDebugProcess, workspace::LapceWorkspace,
};

use super::tab::TerminalTabData;

pub struct TerminalTabInfo {
    pub active: usize,
    pub tabs: im::Vector<TerminalTabData>,
}

pub struct TerminalPanelData {
    pub tab_info: RwSignal<TerminalTabInfo>,
}

impl TerminalPanelData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        proxy: ProxyRpcHandler,
        run_debug: Option<RunDebugProcess>,
        config: &LapceConfig,
    ) -> Self {
        let terminal_tab =
            TerminalTabData::new(cx, workspace, proxy, run_debug, config);
        let tabs = im::vector![terminal_tab];
        let tab_info = TerminalTabInfo { active: 0, tabs };
        let tab_info = create_rw_signal(cx.scope, tab_info);

        Self { tab_info }
    }
}
