use std::{rc::Rc, sync::Arc};

use floem::reactive::{RwSignal, Scope, SignalGet, SignalWith};
use lapce_rpc::terminal::TerminalProfile;

use super::data::TerminalData;
use crate::{
    debug::RunDebugProcess, id::TerminalTabId, window_tab::CommonData,
    workspace::LapceWorkspace,
};

#[derive(Clone)]
pub struct TerminalTabData {
    pub scope: Scope,
    pub terminal_tab_id: TerminalTabId,
    pub active: RwSignal<usize>,
    pub terminals: RwSignal<im::Vector<(RwSignal<usize>, TerminalData)>>,
}

impl TerminalTabData {
    pub fn new(
        workspace: Arc<LapceWorkspace>,
        profile: Option<TerminalProfile>,
        common: Rc<CommonData>,
    ) -> Self {
        TerminalTabData::new_run_debug(workspace, None, profile, common)
    }

    /// Create the information for a terminal tab, which can contain multiple terminals.  
    pub fn new_run_debug(
        workspace: Arc<LapceWorkspace>,
        run_debug: Option<RunDebugProcess>,
        profile: Option<TerminalProfile>,
        common: Rc<CommonData>,
    ) -> Self {
        let cx = common.scope.create_child();
        let terminal_data =
            TerminalData::new_run_debug(cx, workspace, run_debug, profile, common);
        let terminals = im::vector![(cx.create_rw_signal(0), terminal_data)];
        let terminals = cx.create_rw_signal(terminals);
        let active = cx.create_rw_signal(0);
        let terminal_tab_id = TerminalTabId::next();
        Self {
            scope: cx,
            terminal_tab_id,
            active,
            terminals,
        }
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
                .map(|(_, t)| t)
        } else {
            self.terminals
                .with_untracked(|t| t.get(active).or_else(|| t.last()).cloned())
                .map(|(_, t)| t)
        }
    }
}
