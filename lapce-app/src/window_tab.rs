use std::sync::Arc;

use floem::{
    app::AppContext,
    glazier::KeyEvent,
    reactive::{
        create_rw_signal, create_signal, use_context, ReadSignal, RwSignal,
        UntrackedGettableSignal,
    },
};
use lapce_core::register::Register;

use crate::{
    command::{InternalCommand, LapceWorkbenchCommand},
    config::LapceConfig,
    db::LapceDb,
    keypress::{DefaultKeyPress, KeyPressData, KeyPressFocus},
    main_split::MainSplitData,
    palette::{kind::PaletteKind, PaletteData},
    proxy::{start_proxy, ProxyData},
    workspace::LapceWorkspace,
};

#[derive(Clone)]
pub enum Focus {
    Workbench,
    Palette,
}

#[derive(Clone)]
pub struct WindowTabData {
    pub palette: PaletteData,
    pub main_split: MainSplitData,
    pub proxy: ProxyData,
    pub keypress: RwSignal<KeyPressData>,
    pub focus: RwSignal<Focus>,
    pub workbench_command: ReadSignal<Option<LapceWorkbenchCommand>>,
    pub internal_command: ReadSignal<Option<InternalCommand>>,
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
        let (internal_command, set_internal_command) = create_signal(cx.scope, None);
        let config = LapceConfig::load(&workspace, &all_disabled_volts);
        let keypress = create_rw_signal(
            cx.scope,
            KeyPressData::new(&config, set_workbench_command),
        );
        let (config, set_config) = create_signal(cx.scope, Arc::new(config));

        let focus = create_rw_signal(cx.scope, Focus::Workbench);

        let proxy = start_proxy(cx, workspace.clone());

        let register = create_rw_signal(cx.scope, Register::default());

        let palette = PaletteData::new(
            cx,
            workspace,
            proxy.rpc.clone(),
            register,
            set_internal_command,
            focus,
            config,
        );

        let main_split = MainSplitData::new(
            cx,
            proxy.rpc.clone(),
            register,
            set_internal_command,
            config,
        );

        Self {
            palette,
            main_split,
            proxy,
            keypress,
            focus,
            workbench_command,
            internal_command,
        }
    }

    pub fn run_workbench_command(&self, cx: AppContext, cmd: LapceWorkbenchCommand) {
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
            Palette => {
                self.palette.run(cx, PaletteKind::File);
                self.focus.set(Focus::Palette);
            }
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
            #[cfg(target_os = "macos")]
            InstallToPATH => todo!(),
            #[cfg(target_os = "macos")]
            UninstallFromPATH => todo!(),
            Quit => todo!(),
        }
    }

    pub fn run_internal_command(&self, cx: AppContext, cmd: InternalCommand) {
        match cmd {
            InternalCommand::OpenFile { path } => {
                self.main_split.open_file(cx, path);
            }
            InternalCommand::Split {
                direction,
                editor_tab_id,
            } => {
                self.main_split.split(cx, direction, editor_tab_id);
            }
            InternalCommand::SplitMove {
                direction,
                editor_tab_id,
            } => {
                self.main_split.split_move(cx, direction, editor_tab_id);
            }
            InternalCommand::SplitExchange { editor_tab_id } => {
                self.main_split.split_exchange(cx, editor_tab_id);
            }
            InternalCommand::EditorTabChildClose {
                editor_tab_id,
                child,
            } => {
                self.main_split
                    .editor_tab_child_close(cx, editor_tab_id, child);
            }
        }
    }

    pub fn key_down(&self, cx: AppContext, key_event: &KeyEvent) {
        let focus = self.focus.get_untracked();
        self.keypress.update(|keypress| {
            let executed = match focus {
                Focus::Workbench => {
                    self.main_split.key_down(cx, key_event, keypress).is_some()
                }
                Focus::Palette => {
                    keypress.key_down(cx, key_event, &self.palette);
                    true
                }
            };

            if !executed {
                keypress.key_down(cx, key_event, &DefaultKeyPress {});
            }
        });
    }
}
