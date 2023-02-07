use std::sync::Arc;

use floem::{
    app::AppContext,
    reactive::{create_rw_signal, create_signal, use_context, ReadSignal, RwSignal},
};

use crate::{
    command::LapceWorkbenchCommand,
    config::LapceConfig,
    db::LapceDb,
    keypress::{KeyPressData, KeyPressFocus},
    palette::PaletteData,
    proxy::{start_proxy, ProxyData},
    workspace::LapceWorkspace,
};

#[derive(Clone)]
pub struct WindowTabData {
    pub palette: PaletteData,
    pub proxy: ProxyData,
    pub keypress: RwSignal<KeyPressData>,
    pub workbench_command: ReadSignal<Option<LapceWorkbenchCommand>>,
}

impl WindowTabData {
    pub fn new(cx: AppContext, workspace: Arc<LapceWorkspace>) -> Self {
        let db: Arc<LapceDb> = use_context(cx.scope).unwrap();

        let disabled_volts = db.get_disabled_volts().unwrap_or_default();
        let workspace_disabled_volts = db
            .get_workspace_disabled_volts(&workspace)
            .unwrap_or_default();
        let mut all_disabled_volts = disabled_volts;
        all_disabled_volts.extend(workspace_disabled_volts.into_iter());

        let (workbench_command, set_workbench_command) =
            create_signal(cx.scope, None);
        let config = LapceConfig::load(&workspace, &all_disabled_volts);
        let keypress = create_rw_signal(
            cx.scope,
            KeyPressData::new(&config, set_workbench_command),
        );
        let (config, set_config) = create_signal(cx.scope, config);

        let proxy = start_proxy(cx);

        let palette = PaletteData::new(cx, proxy.rpc.clone());

        Self {
            palette,
            proxy,
            keypress,
            workbench_command,
        }
    }

    pub fn run_workbench_command(&self, cmd: LapceWorkbenchCommand) {
        use LapceWorkbenchCommand::*;
        match cmd {
            EnableModal => todo!(),
            DisableModal => todo!(),
            OpenFolder => todo!(),
            CloseFolder => todo!(),
            OpenFile => todo!(),
            RevealActiveFileInFileExplorer => todo!(),
            ChangeColorTheme => todo!(),
            ChangeIconTheme => todo!(),
            OpenSettings => todo!(),
            OpenSettingsFile => todo!(),
            OpenSettingsDirectory => todo!(),
            OpenKeyboardShortcuts => todo!(),
            OpenKeyboardShortcutsFile => todo!(),
            OpenLogFile => todo!(),
            OpenLogsDirectory => todo!(),
            OpenProxyDirectory => todo!(),
            OpenThemesDirectory => todo!(),
            OpenPluginsDirectory => todo!(),
            CloseWindowTab => todo!(),
            NewWindowTab => todo!(),
            NewTerminalTab => todo!(),
            CloseTerminalTab => todo!(),
            NextTerminalTab => todo!(),
            PreviousTerminalTab => todo!(),
            NextWindowTab => todo!(),
            PreviousWindowTab => todo!(),
            ReloadWindow => todo!(),
            NewWindow => todo!(),
            CloseWindow => todo!(),
            NewFile => todo!(),
            ConnectSshHost => todo!(),
            ConnectWsl => todo!(),
            DisconnectRemote => todo!(),
            PaletteLine => todo!(),
            Palette => todo!(),
            PaletteSymbol => todo!(),
            PaletteWorkspaceSymbol => todo!(),
            PaletteCommand => todo!(),
            PaletteWorkspace => todo!(),
            PaletteRunAndDebug => todo!(),
            RunAndDebugRestart => todo!(),
            RunAndDebugStop => todo!(),
            CheckoutBranch => todo!(),
            ToggleMaximizedPanel => todo!(),
            HidePanel => todo!(),
            ShowPanel => todo!(),
            TogglePanelFocus => todo!(),
            TogglePanelVisual => todo!(),
            TogglePanelLeftVisual => todo!(),
            TogglePanelRightVisual => todo!(),
            TogglePanelBottomVisual => todo!(),
            ToggleTerminalFocus => todo!(),
            ToggleSourceControlFocus => todo!(),
            TogglePluginFocus => todo!(),
            ToggleFileExplorerFocus => todo!(),
            ToggleProblemFocus => todo!(),
            ToggleSearchFocus => todo!(),
            ToggleTerminalVisual => todo!(),
            ToggleSourceControlVisual => todo!(),
            TogglePluginVisual => todo!(),
            ToggleFileExplorerVisual => todo!(),
            ToggleProblemVisual => todo!(),
            ToggleDebugVisual => todo!(),
            ToggleSearchVisual => todo!(),
            FocusEditor => todo!(),
            FocusTerminal => todo!(),
            SourceControlInit => todo!(),
            SourceControlCommit => todo!(),
            SourceControlCopyActiveFileRemoteUrl => todo!(),
            SourceControlDiscardActiveFileChanges => todo!(),
            SourceControlDiscardTargetFileChanges => todo!(),
            SourceControlDiscardWorkspaceChanges => todo!(),
            ExportCurrentThemeSettings => todo!(),
            InstallTheme => todo!(),
            ChangeFileLanguage => todo!(),
            NextEditorTab => todo!(),
            PreviousEditorTab => todo!(),
            ToggleInlayHints => todo!(),
            RestartToUpdate => todo!(),
            ShowAbout => todo!(),
            SaveAll => todo!(),
            InstallToPATH => todo!(),
            UninstallFromPATH => todo!(),
            Quit => todo!(),
        }
    }
}
