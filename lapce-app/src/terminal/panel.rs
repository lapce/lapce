use std::sync::Arc;

use floem::{
    app::AppContext,
    glazier::KeyEvent,
    reactive::{create_rw_signal, RwSignal, SignalWithUntracked},
};
use lapce_core::mode::Mode;
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    config::LapceConfig,
    debug::RunDebugProcess,
    keypress::{KeyPressData, KeyPressFocus},
    window_tab::CommonData,
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

    pub fn key_down(
        &self,
        cx: AppContext,
        key_event: &KeyEvent,
        keypress: &mut KeyPressData,
    ) {
        println!("terminal panel keydown");
        let tab = self.tab_info.with_untracked(|info| {
            info.tabs
                .get(info.active)
                .or_else(|| info.tabs.last())
                .cloned()
        });
        let terminal = tab.and_then(|tab| tab.active_terminal(false));
        if let Some(terminal) = terminal {
            let executed = keypress.key_down(cx, key_event, &terminal);
            let mode = terminal.get_mode();
            if !executed && mode == Mode::Terminal {
                terminal.send_keypress(cx, key_event);
            }
        }
    }
}
