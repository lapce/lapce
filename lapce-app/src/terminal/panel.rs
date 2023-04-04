use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal},
};
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    config::LapceConfig, debug::RunDebugProcess, window_tab::CommonData,
    workspace::LapceWorkspace,
};

use super::tab::TerminalTabData;

pub struct TerminalTabInfo {
    pub active: usize,
    pub tabs: im::Vector<TerminalTabData>,
}

#[derive(Clone)]
pub struct TerminalPanelData {
    pub tab_info: RwSignal<TerminalTabInfo>,
}

impl TerminalPanelData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        run_debug: Option<RunDebugProcess>,
        common: CommonData,
    ) -> Self {
        let terminal_tab = TerminalTabData::new(cx, workspace, run_debug, common);
        let tabs = im::vector![terminal_tab];
        let tab_info = TerminalTabInfo { active: 0, tabs };
        let tab_info = create_rw_signal(cx.scope, tab_info);

        Self { tab_info }
    }
}
