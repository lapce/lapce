use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{
        create_rw_signal, RwSignal, SignalGet, SignalGetUntracked, SignalWith,
        SignalWithUntracked,
    },
};
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    config::LapceConfig, debug::RunDebugProcess, window_tab::CommonData,
    workspace::LapceWorkspace,
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
        run_debug: Option<RunDebugProcess>,
        common: CommonData,
    ) -> Self {
        let terminal_data = TerminalData::new(cx, workspace, run_debug, common);
        let terminals = im::vector![terminal_data];
        let terminals = create_rw_signal(cx.scope, terminals);
        let active = create_rw_signal(cx.scope, 0);
        Self { active, terminals }
    }

    pub fn active_terminal(&self, tracked: bool) -> Option<TerminalData> {
        let active = if tracked {
            self.active.get()
        } else {
            self.active.get_untracked()
        };

        if tracked {
            self.terminals
                .with(|t| t.get(active).or_else(|| t.last()).cloned())
        } else {
            self.terminals
                .with_untracked(|t| t.get(active).or_else(|| t.last()).cloned())
        }
    }
}
