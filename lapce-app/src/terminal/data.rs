use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal},
};
use lapce_core::mode::{Mode, VisualMode};
use lapce_rpc::{
    dap_types::RunDebugConfig, proxy::ProxyRpcHandler, terminal::TermId,
};
use parking_lot::RwLock;

use crate::{
    config::LapceConfig, debug::RunDebugProcess, workspace::LapceWorkspace,
};

use super::raw::RawTerminal;

#[derive(Clone)]
pub struct TerminalData {
    pub workspace: Arc<LapceWorkspace>,
    pub proxy: ProxyRpcHandler,
    pub mode: RwSignal<Mode>,
    pub visual_mode: RwSignal<VisualMode>,
    pub raw: Arc<RwLock<RawTerminal>>,
    pub run_debug: RwSignal<Option<RunDebugProcess>>,
}

impl TerminalData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        proxy: ProxyRpcHandler,
        run_debug: Option<RunDebugProcess>,
        config: &LapceConfig,
    ) -> Self {
        let term_id = TermId::next();

        let raw = Self::new_raw_terminal(
            workspace.clone(),
            term_id,
            proxy.clone(),
            run_debug.as_ref().map(|r| &r.config),
            config,
        );

        let run_debug = create_rw_signal(cx.scope, run_debug);
        let mode = create_rw_signal(cx.scope, Mode::Terminal);
        let visual_mode = create_rw_signal(cx.scope, VisualMode::Normal);

        Self {
            workspace,
            proxy,
            raw,
            run_debug,
            mode,
            visual_mode,
        }
    }

    pub fn new_raw_terminal(
        workspace: Arc<LapceWorkspace>,
        term_id: TermId,
        proxy: ProxyRpcHandler,
        run_debug: Option<&RunDebugConfig>,
        config: &LapceConfig,
    ) -> Arc<RwLock<RawTerminal>> {
        let raw = Arc::new(RwLock::new(RawTerminal::new(term_id, proxy.clone())));

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
            config.terminal.shell.clone()
        };

        {
            let raw = raw.clone();
            std::thread::spawn(move || {
                proxy.new_terminal(term_id, cwd, shell);
            });
        }
        raw
    }
}
