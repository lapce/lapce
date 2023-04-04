use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal, SignalGetUntracked},
};
use lapce_core::mode::{Mode, VisualMode};
use lapce_rpc::{
    dap_types::RunDebugConfig, proxy::ProxyRpcHandler, terminal::TermId,
};
use parking_lot::RwLock;

use crate::{
    config::LapceConfig, debug::RunDebugProcess, window_tab::CommonData,
    workspace::LapceWorkspace,
};

use super::{event::TermEvent, raw::RawTerminal};

#[derive(Clone)]
pub struct TerminalData {
    pub term_id: TermId,
    pub workspace: Arc<LapceWorkspace>,
    pub title: RwSignal<String>,
    pub mode: RwSignal<Mode>,
    pub visual_mode: RwSignal<VisualMode>,
    pub raw: Arc<RwLock<RawTerminal>>,
    pub run_debug: RwSignal<Option<RunDebugProcess>>,
    pub common: CommonData,
}

impl TerminalData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        run_debug: Option<RunDebugProcess>,
        common: CommonData,
    ) -> Self {
        let term_id = TermId::next();

        let title = create_rw_signal(cx.scope, "".to_string());

        let raw = Self::new_raw_terminal(
            workspace.clone(),
            term_id,
            title,
            run_debug.as_ref().map(|r| &r.config),
            common.clone(),
        );

        let run_debug = create_rw_signal(cx.scope, run_debug);
        let mode = create_rw_signal(cx.scope, Mode::Terminal);
        let visual_mode = create_rw_signal(cx.scope, VisualMode::Normal);

        Self {
            term_id,
            workspace,
            raw,
            title,
            run_debug,
            mode,
            visual_mode,
            common,
        }
    }

    pub fn new_raw_terminal(
        workspace: Arc<LapceWorkspace>,
        term_id: TermId,
        title: RwSignal<String>,
        run_debug: Option<&RunDebugConfig>,
        common: CommonData,
    ) -> Arc<RwLock<RawTerminal>> {
        let raw = Arc::new(RwLock::new(RawTerminal::new(
            term_id,
            common.proxy.clone(),
            title,
        )));

        let mut cwd = workspace.path.as_ref().cloned();
        let shell = if let Some(run_debug) = run_debug {
            if let Some(path) = run_debug.cwd.as_ref() {
                cwd = Some(PathBuf::from(path));
                if path.contains("${workspace}") {
                    if let Some(workspace) = workspace
                        .path
                        .as_ref()
                        .and_then(|workspace| workspace.to_str())
                    {
                        cwd = Some(PathBuf::from(
                            &path.replace("${workspace}", workspace),
                        ));
                    }
                }
            }

            if let Some(debug_command) = run_debug.debug_command.as_ref() {
                debug_command.clone()
            } else {
                format!("{} {}", run_debug.program, run_debug.args.join(" "))
            }
        } else {
            common.config.get_untracked().terminal.shell.clone()
        };

        {
            let raw = raw.clone();
            let _ = common.term_tx.send((term_id, TermEvent::NewTerminal(raw)));
            common.proxy.new_terminal(term_id, cwd, shell);
        }
        raw
    }
}
