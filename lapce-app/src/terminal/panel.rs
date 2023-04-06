use std::sync::Arc;

use floem::{
    app::AppContext,
    glazier::KeyEvent,
    reactive::{
        create_rw_signal, RwSignal, SignalSet, SignalWith, SignalWithUntracked,
    },
};
use lapce_core::mode::Mode;
use lapce_rpc::{proxy::ProxyRpcHandler, terminal::TermId};

use crate::{
    config::LapceConfig,
    debug::RunDebugProcess,
    keypress::{KeyPressData, KeyPressFocus},
    window_tab::CommonData,
    workspace::LapceWorkspace,
};

use super::{data::TerminalData, tab::TerminalTabData};

pub struct TerminalTabInfo {
    pub active: usize,
    pub tabs: im::Vector<(RwSignal<usize>, TerminalTabData)>,
}

#[derive(Clone)]
pub struct TerminalPanelData {
    pub tab_info: RwSignal<TerminalTabInfo>,
    pub common: CommonData,
}

impl TerminalPanelData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        run_debug: Option<RunDebugProcess>,
        common: CommonData,
    ) -> Self {
        let terminal_tab =
            TerminalTabData::new(cx, workspace, run_debug, common.clone());

        let tabs = im::vector![(create_rw_signal(cx.scope, 0), terminal_tab)];
        let tab_info = TerminalTabInfo { active: 0, tabs };
        let tab_info = create_rw_signal(cx.scope, tab_info);

        Self { tab_info, common }
    }

    pub fn active_tab(&self, tracked: bool) -> Option<TerminalTabData> {
        if tracked {
            self.tab_info.with(|info| {
                info.tabs
                    .get(info.active)
                    .or_else(|| info.tabs.last())
                    .cloned()
                    .map(|(_, tab)| tab)
            })
        } else {
            self.tab_info.with_untracked(|info| {
                info.tabs
                    .get(info.active)
                    .or_else(|| info.tabs.last())
                    .cloned()
                    .map(|(_, tab)| tab)
            })
        }
    }

    pub fn key_down(
        &self,
        cx: AppContext,
        key_event: &KeyEvent,
        keypress: &mut KeyPressData,
    ) {
        let tab = self.active_tab(false);
        let terminal = tab.and_then(|tab| tab.active_terminal(false));
        if let Some(terminal) = terminal {
            let executed = keypress.key_down(cx, key_event, &terminal);
            let mode = terminal.get_mode();
            if !executed && mode == Mode::Terminal {
                terminal.send_keypress(cx, key_event);
            }
        }
    }

    pub fn set_title(&self, term_id: &TermId, title: &str) {
        let t = self.tab_info.with_untracked(|info| {
            for (_, tab) in &info.tabs {
                let terminal = tab.terminals.with_untracked(|terminals| {
                    terminals.iter().find(|t| &t.term_id == term_id).cloned()
                });
                if let Some(terminal) = terminal {
                    return Some(terminal.title);
                }
            }
            None
        });
        if let Some(t) = t {
            t.set(title.to_string());
        }
    }
}
