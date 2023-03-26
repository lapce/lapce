use std::sync::Arc;

use floem::{
    app::AppContext,
    ext_event::open_file_dialog,
    glazier::{FileDialogOptions, KeyEvent},
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{
        create_effect, create_rw_signal, create_signal, use_context, ReadSignal,
        RwSignal, SignalGet, SignalGetUntracked, SignalSet, SignalUpdate,
        SignalWithUntracked, WriteSignal,
    },
};
use lapce_core::register::Register;
use lapce_rpc::proxy::ProxyRpcHandler;

use crate::{
    code_action::{CodeActionData, CodeActionStatus},
    command::{
        CommandKind, InternalCommand, LapceCommand, LapceWorkbenchCommand,
        WindowCommand,
    },
    completion::{CompletionData, CompletionStatus},
    config::LapceConfig,
    db::LapceDb,
    editor::location::EditorLocation,
    id::WindowTabId,
    keypress::{DefaultKeyPress, KeyPressData, KeyPressFocus},
    main_split::{MainSplitData, SplitData, SplitDirection},
    palette::{kind::PaletteKind, PaletteData},
    proxy::{start_proxy, ProxyData},
    workspace::{LapceWorkspace, LapceWorkspaceType, WorkspaceInfo},
};

#[derive(Clone)]
pub enum Focus {
    Workbench,
    Palette,
}

#[derive(Clone)]
pub struct CommonData {
    pub focus: RwSignal<Focus>,
    pub completion: RwSignal<CompletionData>,
    pub code_action: RwSignal<CodeActionData>,
    pub register: RwSignal<Register>,
    pub window_command: WriteSignal<Option<WindowCommand>>,
    pub internal_command: RwSignal<Option<InternalCommand>>,
    pub lapce_command: RwSignal<Option<LapceCommand>>,
    pub workbench_command: RwSignal<Option<LapceWorkbenchCommand>>,
    pub proxy: ProxyRpcHandler,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

#[derive(Clone)]
pub struct WindowTabData {
    pub window_tab_id: WindowTabId,
    pub workspace: Arc<LapceWorkspace>,
    pub palette: PaletteData,
    pub main_split: MainSplitData,
    pub keypress: RwSignal<KeyPressData>,
    pub window_origin: RwSignal<Point>,
    pub layout_rect: RwSignal<Rect>,
    pub proxy: ProxyData,
    pub common: CommonData,
}

impl WindowTabData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        window_command: WriteSignal<Option<WindowCommand>>,
    ) -> Self {
        let db: Arc<LapceDb> = use_context(cx.scope).unwrap();

        let disabled_volts = db.get_disabled_volts().unwrap_or_default();
        let workspace_disabled_volts = db
            .get_workspace_disabled_volts(&workspace)
            .unwrap_or_default();
        let mut all_disabled_volts = disabled_volts;
        all_disabled_volts.extend(workspace_disabled_volts.into_iter());

        let workspace_info = if workspace.path.is_some() {
            db.get_workspace_info(&workspace).ok()
        } else {
            let mut info = db.get_workspace_info(&workspace).ok();
            if let Some(info) = info.as_mut() {
                info.split.children.clear();
            }
            info
        };

        let lapce_command = create_rw_signal(cx.scope, None);
        let workbench_command = create_rw_signal(cx.scope, None);
        let internal_command = create_rw_signal(cx.scope, None);
        let config = LapceConfig::load(&workspace, &all_disabled_volts);
        let keypress = create_rw_signal(
            cx.scope,
            KeyPressData::new(&config, workbench_command.write_only()),
        );
        let (config, set_config) = create_signal(cx.scope, Arc::new(config));

        let focus = create_rw_signal(cx.scope, Focus::Workbench);
        let completion = create_rw_signal(cx.scope, CompletionData::new(cx, config));
        let code_action =
            create_rw_signal(cx.scope, CodeActionData::new(cx, config));

        let proxy = start_proxy(cx, workspace.clone(), completion.write_only());

        let register = create_rw_signal(cx.scope, Register::default());

        let common = CommonData {
            focus,
            completion,
            code_action,
            register,
            window_command,
            internal_command,
            lapce_command,
            workbench_command,
            proxy: proxy.rpc.clone(),
            config,
        };

        let main_split = MainSplitData::new(cx, common.clone());

        if let Some(info) = workspace_info {
            let root_split = main_split.root_split;
            info.split.to_data(cx, main_split.clone(), None, root_split);
        } else {
            let root_split = main_split.root_split;
            let root_split_data = SplitData {
                parent_split: None,
                split_id: root_split,
                children: Vec::new(),
                direction: SplitDirection::Horizontal,
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
            };
            main_split.splits.update(|splits| {
                splits
                    .insert(root_split, create_rw_signal(cx.scope, root_split_data));
            });
        }

        let palette = PaletteData::new(
            cx,
            workspace.clone(),
            main_split.clone(),
            keypress.read_only(),
            common.clone(),
        );

        let window_tab_data = Self {
            window_tab_id: WindowTabId::next(),
            workspace,
            palette,
            main_split,
            keypress,
            window_origin: create_rw_signal(cx.scope, Point::ZERO),
            layout_rect: create_rw_signal(cx.scope, Rect::ZERO),
            proxy,
            common,
        };

        {
            let window_tab_data = window_tab_data.clone();
            create_effect(cx.scope, move |_| {
                if let Some(cmd) = window_tab_data.common.lapce_command.get() {
                    window_tab_data.run_lapce_command(cx, cmd);
                }
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            create_effect(cx.scope, move |_| {
                if let Some(cmd) = window_tab_data.common.workbench_command.get() {
                    window_tab_data.run_workbench_command(cx, cmd);
                }
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            let internal_command = window_tab_data.common.internal_command;
            create_effect(cx.scope, move |_| {
                if let Some(cmd) = internal_command.get() {
                    window_tab_data.run_internal_command(cx, cmd);
                }
            });
        }

        window_tab_data
    }

    pub fn run_lapce_command(&self, cx: AppContext, cmd: LapceCommand) {
        match cmd.kind {
            CommandKind::Workbench(cmd) => {
                self.run_workbench_command(cx, cmd);
            }
            CommandKind::Edit(_) => todo!(),
            CommandKind::Move(_) => todo!(),
            CommandKind::Focus(_) => todo!(),
            CommandKind::MotionMode(_) => todo!(),
            CommandKind::MultiSelection(_) => todo!(),
        }
    }

    pub fn run_workbench_command(&self, cx: AppContext, cmd: LapceWorkbenchCommand) {
        use LapceWorkbenchCommand::*;
        match cmd {
            EnableModal => todo!(),
            DisableModal => todo!(),
            OpenFolder => {
                println!("open folder");
                if !self.workspace.kind.is_remote() {
                    let window_command = self.common.window_command;
                    let options = FileDialogOptions::new().select_directories();
                    open_file_dialog(options, move |file| {
                        if let Some(file) = file {
                            let workspace = LapceWorkspace {
                                kind: LapceWorkspaceType::Local,
                                path: Some(file.path),
                                last_open: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            };
                            window_command.set(Some(WindowCommand::SetWorkspace {
                                workspace,
                            }));
                        }
                    });
                }
            }
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
            }
            PaletteSymbol => todo!(),
            PaletteWorkspaceSymbol => todo!(),
            PaletteCommand => {
                self.palette.run(cx, PaletteKind::Command);
            }
            PaletteWorkspace => {
                self.palette.run(cx, PaletteKind::Workspace);
            }
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
                self.main_split.go_to_location(
                    cx,
                    EditorLocation {
                        path,
                        position: None,
                        scroll_offset: None,
                    },
                );
            }
            InternalCommand::GoToLocation { location } => {
                self.main_split.go_to_location(cx, location);
            }
            InternalCommand::JumpToLocation { location } => {
                self.main_split.jump_to_location(cx, location);
            }
            InternalCommand::JumpLocationForward => {
                self.main_split.jump_location_forward(cx);
            }
            InternalCommand::JumpLocationBackward => {
                self.main_split.jump_location_backward(cx);
            }
            InternalCommand::PaletteReferences { references } => {
                self.palette.references.set(references);
                self.palette.run(cx, PaletteKind::Reference);
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
        let focus = self.common.focus.get_untracked();
        let mut keypress = self.keypress.get_untracked();
        let executed = match focus {
            Focus::Workbench => self
                .main_split
                .key_down(cx, key_event, &mut keypress)
                .is_some(),
            Focus::Palette => {
                keypress.key_down(cx, key_event, &self.palette);
                true
            }
        };

        if !executed {
            keypress.key_down(cx, key_event, &DefaultKeyPress {});
        }

        self.keypress.set(keypress);
    }

    pub fn workspace_info(&self) -> WorkspaceInfo {
        let main_split_data = self
            .main_split
            .splits
            .get_untracked()
            .get(&self.main_split.root_split)
            .cloned()
            .unwrap();
        WorkspaceInfo {
            split: main_split_data.get_untracked().split_info(self),
        }
    }

    pub fn completion_origin(&self) -> Point {
        let completion = self.common.completion.get();
        let config = self.common.config.get();
        if completion.status == CompletionStatus::Inactive {
            return Point::ZERO;
        }

        let editor = if let Some(editor) = self.main_split.active_editor() {
            editor
        } else {
            return Point::ZERO;
        };

        let (window_origin, viewport, doc) =
            editor.with_untracked(|e| (e.window_origin, e.viewport, e.doc));

        let (point_above, point_below) =
            doc.with_untracked(|doc| doc.points_of_offset(completion.offset));

        let window_origin = window_origin.get();
        let viewport = viewport.get();
        let completion_size = completion.layout_rect.size();
        let tab_size = self.layout_rect.get().size();

        let mut origin = window_origin
            + Vec2::new(
                point_below.x
                    - viewport.x0
                    - config.editor.line_height() as f64
                    - 5.0,
                point_below.y - viewport.y0,
            );
        if origin.y + completion_size.height > tab_size.height {
            origin.y = window_origin.y + (point_above.y - viewport.y0)
                - completion_size.height;
        }
        if origin.x + completion_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - completion_size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        origin
    }

    pub fn code_action_origin(&self) -> Point {
        let code_action = self.common.code_action.get();
        let config = self.common.config.get();
        if code_action.status == CodeActionStatus::Inactive {
            return Point::ZERO;
        }

        let editor = if let Some(editor) = self.main_split.active_editor() {
            editor
        } else {
            return Point::ZERO;
        };

        let (window_origin, viewport, doc) =
            editor.with_untracked(|e| (e.window_origin, e.viewport, e.doc));

        let (point_above, point_below) =
            doc.with_untracked(|doc| doc.points_of_offset(code_action.offset));

        let window_origin = window_origin.get();
        let viewport = viewport.get();
        let code_action_size = code_action.layout_rect.size();
        let tab_size = self.layout_rect.get().size();

        let mut origin = window_origin
            + Vec2::new(point_below.x - viewport.x0, point_below.y - viewport.y0);
        if origin.y + code_action_size.height > tab_size.height {
            origin.y = window_origin.y + (point_above.y - viewport.y0)
                - code_action_size.height;
        }
        if origin.x + code_action_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - code_action_size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        origin
    }
}
