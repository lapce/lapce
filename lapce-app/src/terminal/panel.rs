use std::sync::Arc;

use floem::{
    app::AppContext,
    glazier::KeyEvent,
    reactive::{
        create_rw_signal, RwSignal, SignalGetUntracked, SignalSet, SignalUpdate,
        SignalWith, SignalWithUntracked,
    },
};
use lapce_core::mode::Mode;
use lapce_rpc::{proxy::ProxyRpcHandler, terminal::TermId};

use crate::{
    config::LapceConfig,
    debug::RunDebugProcess,
    id::TerminalTabId,
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
    pub workspace: Arc<LapceWorkspace>,
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
            TerminalTabData::new(cx, workspace.clone(), run_debug, common.clone());

        let tabs = im::vector![(create_rw_signal(cx.scope, 0), terminal_tab)];
        let tab_info = TerminalTabInfo { active: 0, tabs };
        let tab_info = create_rw_signal(cx.scope, tab_info);

        Self {
            workspace,
            tab_info,
            common,
        }
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
        if self.tab_info.with_untracked(|info| info.tabs.is_empty()) {
            self.new_tab(cx);
        }

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

    pub fn new_tab(&self, cx: AppContext) {
        let terminal_tab = TerminalTabData::new(
            cx,
            self.workspace.clone(),
            None,
            self.common.clone(),
        );

        self.tab_info.update(|info| {
            info.tabs.insert(
                if info.tabs.is_empty() {
                    0
                } else {
                    (info.active + 1).min(info.tabs.len())
                },
                (create_rw_signal(cx.scope, 0), terminal_tab),
            );
            let new_active = (info.active + 1).min(info.tabs.len() - 1);
            info.active = new_active;
        })
    }

    pub fn next_tab(&self) {
        self.tab_info.update(|info| {
            if info.active >= info.tabs.len().saturating_sub(1) {
                info.active = 0;
            } else {
                info.active += 1;
            }
        });
    }

    pub fn previous_tab(&self) {
        self.tab_info.update(|info| {
            if info.active == 0 {
                info.active = info.tabs.len().saturating_sub(1);
            } else {
                info.active -= 1;
            }
        });
    }

    pub fn close_tab(&self, terminal_tab_id: Option<TerminalTabId>) {
        self.tab_info.update(|info| {
            if let Some(terminal_tab_id) = terminal_tab_id {
                info.tabs
                    .retain(|(_, t)| t.terminal_tab_id != terminal_tab_id);
            } else {
                let active = info.active.min(info.tabs.len().saturating_sub(1));
                if !info.tabs.is_empty() {
                    info.tabs.remove(active);
                }
            }
            let new_active = info.active.min(info.tabs.len().saturating_sub(1));
            info.active = new_active;
        });
    }

    pub fn set_title(&self, term_id: &TermId, title: &str) {
        if let Some(t) = self.get_terminal(term_id) {
            t.title.set(title.to_string());
        }
    }

    fn get_terminal(&self, term_id: &TermId) -> Option<TerminalData> {
        self.tab_info.with_untracked(|info| {
            for (_, tab) in &info.tabs {
                let terminal = tab.terminals.with_untracked(|terminals| {
                    terminals
                        .iter()
                        .find(|(_, t)| &t.term_id == term_id)
                        .cloned()
                });
                if let Some(terminal) = terminal {
                    return Some(terminal.1);
                }
            }
            None
        })
    }

    fn get_terminal_in_tab(
        &self,
        term_id: &TermId,
    ) -> Option<(TerminalTabData, usize)> {
        self.tab_info.with_untracked(|info| {
            for (_, tab) in &info.tabs {
                let index = tab.terminals.with_untracked(|terminals| {
                    terminals
                        .iter()
                        .enumerate()
                        .find(|(_, (_, t))| &t.term_id == term_id)
                        .map(|(i, _)| i)
                });
                if let Some(index) = index {
                    return Some((tab.clone(), index));
                }
            }
            None
        })
    }

    pub fn split(&self, cx: AppContext, term_id: TermId) {
        if let Some((tab, index)) = self.get_terminal_in_tab(&term_id) {
            let terminal_data = TerminalData::new(
                cx,
                self.workspace.clone(),
                None,
                self.common.clone(),
            );
            let i = create_rw_signal(cx.scope, 0);
            tab.terminals.update(|terminals| {
                terminals.insert(index, (i, terminal_data));
            });
        }
    }

    pub fn split_next(&self, _cx: AppContext, term_id: TermId) {
        if let Some((tab, index)) = self.get_terminal_in_tab(&term_id) {
            let max = tab.terminals.with_untracked(|t| t.len() - 1);
            let new_index = (index + 1).min(max);
            if new_index != index {
                tab.active.set(new_index);
            }
        }
    }

    pub fn split_previous(&self, _cx: AppContext, term_id: TermId) {
        if let Some((tab, index)) = self.get_terminal_in_tab(&term_id) {
            let new_index = index.saturating_sub(1);
            if new_index != index {
                tab.active.set(new_index);
            }
        }
    }

    pub fn split_exchange(&self, _cx: AppContext, term_id: TermId) {
        if let Some((tab, index)) = self.get_terminal_in_tab(&term_id) {
            let max = tab.terminals.with_untracked(|t| t.len() - 1);
            if index < max {
                tab.terminals.update(|terminals| {
                    terminals.swap(index, index + 1);
                });
            }
        }
    }

    pub fn close_terminal(&self, term_id: &TermId) {
        if let Some((tab, index)) = self.get_terminal_in_tab(term_id) {
            let active = tab.active.get_untracked();
            let len = tab
                .terminals
                .try_update(|terminals| {
                    terminals.remove(index);
                    terminals.len()
                })
                .unwrap();
            if len == 0 {
                self.close_tab(Some(tab.terminal_tab_id));
            } else {
                let new_active = active.min(len.saturating_sub(1));
                if new_active != active {
                    tab.active.set(new_active);
                }
            }
        }
    }

    pub fn terminal_stopped(&self, term_id: &TermId) {
        if let Some(terminal) = self.get_terminal(term_id) {
            if terminal.run_debug.with_untracked(|r| r.is_some()) {
                terminal.run_debug.update(|run_debug| {
                    if let Some(run_debug) = run_debug.as_mut() {
                        run_debug.stopped = true
                    }
                });
            } else {
                self.close_terminal(term_id);
            }
        }
    }
}
