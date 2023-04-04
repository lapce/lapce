use std::sync::Arc;

use crossbeam_channel::Sender;
use floem::{
    app::AppContext,
    ext_event::open_file_dialog,
    glazier::{FileDialogOptions, KeyEvent},
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{
        create_effect, create_rw_signal, create_signal, use_context, ReadSignal,
        RwSignal, SignalGet, SignalGetUntracked, SignalSet, SignalUpdate,
        SignalWith, SignalWithUntracked, WriteSignal,
    },
};
use itertools::Itertools;
use lapce_core::{mode::Mode, register::Register};
use lapce_rpc::{core::CoreNotification, proxy::ProxyRpcHandler, terminal::TermId};

use crate::{
    code_action::{CodeActionData, CodeActionStatus},
    command::{
        CommandKind, InternalCommand, LapceCommand, LapceWorkbenchCommand,
        WindowCommand,
    },
    completion::{CompletionData, CompletionStatus},
    config::LapceConfig,
    db::LapceDb,
    doc::EditorDiagnostic,
    editor::location::EditorLocation,
    id::WindowTabId,
    keypress::{DefaultKeyPress, KeyPressData, KeyPressFocus},
    main_split::{MainSplitData, SplitData, SplitDirection},
    palette::{kind::PaletteKind, PaletteData},
    panel::data::{default_panel_order, PanelData},
    proxy::{path_from_url, start_proxy, ProxyData},
    source_control::SourceControlData,
    terminal::event::{terminal_update_process, TermEvent},
    workspace::{LapceWorkspace, LapceWorkspaceType, WorkspaceInfo},
};

#[derive(Clone, PartialEq, Eq)]
pub enum Focus {
    Workbench,
    Palette,
    CodeAction,
}

#[derive(Clone)]
pub struct CommonData {
    pub focus: RwSignal<Focus>,
    pub completion: RwSignal<CompletionData>,
    pub register: RwSignal<Register>,
    pub window_command: WriteSignal<Option<WindowCommand>>,
    pub internal_command: RwSignal<Option<InternalCommand>>,
    pub lapce_command: RwSignal<Option<LapceCommand>>,
    pub workbench_command: RwSignal<Option<LapceWorkbenchCommand>>,
    pub term_tx: Sender<(TermId, TermEvent)>,
    pub proxy: ProxyRpcHandler,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

#[derive(Clone)]
pub struct WindowTabData {
    pub window_tab_id: WindowTabId,
    pub workspace: Arc<LapceWorkspace>,
    pub palette: PaletteData,
    pub main_split: MainSplitData,
    pub panel: PanelData,
    pub code_action: RwSignal<CodeActionData>,
    pub source_control: RwSignal<SourceControlData>,
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

        let config = LapceConfig::load(&workspace, &all_disabled_volts);
        let lapce_command = create_rw_signal(cx.scope, None);
        let workbench_command = create_rw_signal(cx.scope, None);
        let internal_command = create_rw_signal(cx.scope, None);
        let keypress = create_rw_signal(
            cx.scope,
            KeyPressData::new(&config, workbench_command.write_only()),
        );

        let (term_tx, term_rx) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            terminal_update_process(term_rx);
        });

        let proxy = start_proxy(
            cx,
            workspace.clone(),
            all_disabled_volts,
            config.plugins.clone(),
        );
        let (config, set_config) = create_signal(cx.scope, Arc::new(config));

        let focus = create_rw_signal(cx.scope, Focus::Workbench);
        let completion = create_rw_signal(cx.scope, CompletionData::new(cx, config));

        let register = create_rw_signal(cx.scope, Register::default());

        let common = CommonData {
            focus,
            completion,
            register,
            window_command,
            internal_command,
            lapce_command,
            workbench_command,
            term_tx,
            proxy: proxy.rpc.clone(),
            config,
        };

        let main_split = MainSplitData::new(cx, common.clone());
        let code_action =
            create_rw_signal(cx.scope, CodeActionData::new(cx, common.clone()));
        let source_control =
            create_rw_signal(cx.scope, SourceControlData::new(cx, common.clone()));

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

        let panel_order = db
            .get_panel_orders()
            .unwrap_or_else(|_| default_panel_order());
        let panel = PanelData::new(cx, panel_order);

        let window_tab_data = Self {
            window_tab_id: WindowTabId::next(),
            workspace,
            palette,
            main_split,
            panel,
            code_action,
            source_control,
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

        {
            let window_tab_data = window_tab_data.clone();
            let notification = window_tab_data.proxy.notification;
            create_effect(cx.scope, move |_| {
                notification.with(|rpc| {
                    if let Some(rpc) = rpc.as_ref() {
                        window_tab_data.handle_core_notification(cx, rpc);
                    }
                });
            });
        }

        window_tab_data
    }

    pub fn run_lapce_command(&self, cx: AppContext, cmd: LapceCommand) {
        match cmd.kind {
            CommandKind::Workbench(cmd) => {
                self.run_workbench_command(cx, cmd);
            }
            CommandKind::Edit(_) => {}
            CommandKind::Move(_) => {}
            CommandKind::Focus(_) => {}
            CommandKind::MotionMode(_) => {}
            CommandKind::MultiSelection(_) => {}
        }
    }

    pub fn run_workbench_command(&self, cx: AppContext, cmd: LapceWorkbenchCommand) {
        use LapceWorkbenchCommand::*;
        match cmd {
            EnableModal => {}
            DisableModal => {}
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
            CloseFolder => {}
            OpenFile => {}
            RevealActiveFileInFileExplorer => {}
            ChangeColorTheme => {}
            ChangeIconTheme => {}
            OpenSettings => {}
            OpenSettingsFile => {}
            OpenSettingsDirectory => {}
            OpenKeyboardShortcuts => {}
            OpenKeyboardShortcutsFile => {}
            OpenLogFile => {}
            OpenLogsDirectory => {}
            OpenProxyDirectory => {}
            OpenThemesDirectory => {}
            OpenPluginsDirectory => {}
            CloseWindowTab => {}
            NewWindowTab => {}
            NewTerminalTab => {}
            CloseTerminalTab => {}
            NextTerminalTab => {}
            PreviousTerminalTab => {}
            NextWindowTab => {}
            PreviousWindowTab => {}
            ReloadWindow => {}
            NewWindow => {}
            CloseWindow => {}
            NewFile => {}
            ConnectSshHost => {}
            ConnectWsl => {}
            DisconnectRemote => {}
            PaletteLine => {
                self.palette.run(cx, PaletteKind::Line);
            }
            Palette => {
                self.palette.run(cx, PaletteKind::File);
            }
            PaletteSymbol => {
                self.palette.run(cx, PaletteKind::DocumentSymbol);
            }
            PaletteWorkspaceSymbol => {}
            PaletteCommand => {
                self.palette.run(cx, PaletteKind::Command);
            }
            PaletteWorkspace => {
                self.palette.run(cx, PaletteKind::Workspace);
            }
            PaletteRunAndDebug => {}
            RunAndDebugRestart => {}
            RunAndDebugStop => {}
            CheckoutBranch => {}
            ToggleMaximizedPanel => {}
            HidePanel => {}
            ShowPanel => {}
            TogglePanelFocus => {}
            TogglePanelVisual => {}
            TogglePanelLeftVisual => {}
            TogglePanelRightVisual => {}
            TogglePanelBottomVisual => {}
            ToggleTerminalFocus => {}
            ToggleSourceControlFocus => {}
            TogglePluginFocus => {}
            ToggleFileExplorerFocus => {}
            ToggleProblemFocus => {}
            ToggleSearchFocus => {}
            ToggleTerminalVisual => {}
            ToggleSourceControlVisual => {}
            TogglePluginVisual => {}
            ToggleFileExplorerVisual => {}
            ToggleProblemVisual => {}
            ToggleDebugVisual => {}
            ToggleSearchVisual => {}
            FocusEditor => {}
            FocusTerminal => {}
            SourceControlInit => {}
            SourceControlCommit => {}
            SourceControlCopyActiveFileRemoteUrl => {}
            SourceControlDiscardActiveFileChanges => {}
            SourceControlDiscardTargetFileChanges => {}
            SourceControlDiscardWorkspaceChanges => {}
            ExportCurrentThemeSettings => {}
            InstallTheme => {}
            ChangeFileLanguage => {}
            NextEditorTab => {}
            PreviousEditorTab => {}
            ToggleInlayHints => {}
            RestartToUpdate => {}
            ShowAbout => {}
            SaveAll => {}
            #[cfg(target_os = "macos")]
            InstallToPATH => {}
            #[cfg(target_os = "macos")]
            UninstallFromPATH => {}
            JumpLocationForward => {
                self.main_split.jump_location_forward(cx, false);
            }
            JumpLocationBackward => {
                self.main_split.jump_location_backward(cx, false);
            }
            JumpLocationForwardLocal => {
                self.main_split.jump_location_forward(cx, true);
            }
            JumpLocationBackwardLocal => {
                self.main_split.jump_location_backward(cx, true);
            }
            NextError => {
                self.main_split.next_error(cx);
            }
            PreviousError => {}
            Quit => {}
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
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    },
                    None,
                );
            }
            InternalCommand::GoToLocation { location } => {
                self.main_split.go_to_location(cx, location, None);
            }
            InternalCommand::JumpToLocation { location } => {
                self.main_split.jump_to_location(cx, location, None);
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
            InternalCommand::ShowCodeActions {
                offset,
                mouse_click,
                code_actions,
            } => {
                let mut code_action = self.code_action.get_untracked();
                code_action.show(code_actions, offset, mouse_click);
                self.code_action.set(code_action);
            }
            InternalCommand::RunCodeAction { plugin_id, action } => {
                self.main_split.run_code_action(cx, plugin_id, action);
            }
        }
    }

    fn handle_core_notification(&self, cx: AppContext, rpc: &CoreNotification) {
        match rpc {
            CoreNotification::DiffInfo { diff } => {
                self.source_control.update(|source_control| {
                    source_control.branch = diff.head.clone();
                    source_control.branches =
                        diff.branches.iter().cloned().collect();
                    source_control.file_diffs = diff
                        .diffs
                        .iter()
                        .cloned()
                        .map(|diff| {
                            let checked = source_control
                                .file_diffs
                                .get(diff.path())
                                .map_or(true, |(_, c)| *c);
                            (diff.path().clone(), (diff, checked))
                        })
                        .collect();
                });
            }
            CoreNotification::CompletionResponse {
                request_id,
                input,
                resp,
                plugin_id,
            } => {
                self.common.completion.update(|completion| {
                    completion.receive(*request_id, input, resp, *plugin_id);
                });
            }
            CoreNotification::PublishDiagnostics { diagnostics } => {
                let path = path_from_url(&diagnostics.uri);
                let diagnostics: im::Vector<EditorDiagnostic> = diagnostics
                    .diagnostics
                    .iter()
                    .map(|d| EditorDiagnostic {
                        range: (0, 0),
                        diagnostic: d.clone(),
                    })
                    .sorted_by_key(|d| d.diagnostic.range.start)
                    .collect();

                // inform the document about the diagnostics
                if let Some(doc) = self
                    .main_split
                    .docs
                    .with_untracked(|docs| docs.get(&path).cloned())
                {
                    doc.update(|doc| {
                        doc.set_diagnostics(diagnostics.clone());
                    });
                }

                self.main_split.diagnostics.update(|d| {
                    d.insert(path, diagnostics);
                });
            }
            CoreNotification::UpdateTerminal { term_id, content } => {
                let _ = self
                    .common
                    .term_tx
                    .send((*term_id, TermEvent::UpdateContent(content.to_vec())));
            }
            CoreNotification::TerminalProcessStopped { term_id } => {
                let _ = self
                    .common
                    .term_tx
                    .send((*term_id, TermEvent::CloseTerminal));
            }
            _ => {}
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
            Focus::CodeAction => {
                let code_action = self.code_action.get_untracked();
                keypress.key_down(cx, key_event, &code_action);
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

        let editor =
            if let Some(editor) = self.main_split.active_editor.get_untracked() {
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
        let code_action = self.code_action.get();
        let config = self.common.config.get();
        if code_action.status.get_untracked() == CodeActionStatus::Inactive {
            return Point::ZERO;
        }

        let tab_size = self.layout_rect.get().size();
        let code_action_size = code_action.layout_rect.size();

        let editor =
            if let Some(editor) = self.main_split.active_editor.get_untracked() {
                editor
            } else {
                return Point::ZERO;
            };

        let (window_origin, viewport, doc) =
            editor.with_untracked(|e| (e.window_origin, e.viewport, e.doc));

        let (_point_above, point_below) =
            doc.with_untracked(|doc| doc.points_of_offset(code_action.offset));

        let window_origin = window_origin.get();
        let viewport = viewport.get();

        let mut origin = window_origin
            + Vec2::new(
                if code_action.mouse_click {
                    0.0
                } else {
                    point_below.x - viewport.x0
                },
                point_below.y - viewport.y0,
            );

        if origin.y + code_action_size.height > tab_size.height {
            origin.y = origin.y
                - config.editor.line_height() as f64
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

    /// Get the mode for the current editor or terminal
    pub fn mode(&self) -> Mode {
        if self.common.config.get().core.modal {
            let mode = if self.common.focus.get() == Focus::Workbench {
                self.main_split.active_editor.get().map(|e| {
                    e.with_untracked(|editor| editor.cursor).get().get_mode()
                })
            } else {
                None
            };

            mode.unwrap_or(Mode::Normal)
        } else {
            Mode::Insert
        }
    }
}
