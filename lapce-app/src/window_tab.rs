use std::{collections::HashSet, sync::Arc, time::Instant};

use crossbeam_channel::Sender;
use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout},
    ext_event::create_signal_from_channel,
    glazier::{FileDialogOptions, KeyEvent, Modifiers},
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{
        create_effect, create_memo, create_rw_signal, create_signal, use_context,
        Memo, ReadSignal, RwSignal, Scope, SignalGet, SignalGetUntracked, SignalSet,
        SignalUpdate, SignalWith, SignalWithUntracked, WriteSignal,
    },
};
use itertools::Itertools;
use lapce_core::{mode::Mode, register::Register};
use lapce_rpc::{
    core::CoreNotification, dap_types::RunDebugConfig, proxy::ProxyRpcHandler,
    terminal::TermId,
};
use serde_json::Value;

use crate::{
    code_action::{CodeActionData, CodeActionStatus},
    command::{
        CommandExecuted, CommandKind, InternalCommand, LapceCommand,
        LapceWorkbenchCommand, WindowCommand,
    },
    completion::{CompletionData, CompletionStatus},
    config::LapceConfig,
    db::LapceDb,
    debug::{DapData, RunDebugMode, RunDebugProcess},
    doc::EditorDiagnostic,
    editor::location::EditorLocation,
    file_explorer::data::FileExplorerData,
    find::Find,
    global_search::GlobalSearchData,
    id::WindowTabId,
    keypress::{condition::Condition, KeyPressData, KeyPressFocus},
    main_split::{MainSplitData, SplitData, SplitDirection},
    palette::{kind::PaletteKind, PaletteData},
    panel::{
        data::{default_panel_order, PanelData},
        kind::PanelKind,
    },
    plugin::PluginData,
    proxy::{path_from_url, start_proxy, ProxyData},
    rename::RenameData,
    source_control::SourceControlData,
    terminal::{
        event::{terminal_update_process, TermEvent, TermNotification},
        panel::TerminalPanelData,
    },
    workspace::{LapceWorkspace, LapceWorkspaceType, WorkspaceInfo},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Workbench,
    Palette,
    CodeAction,
    Rename,
    Panel(PanelKind),
}

#[derive(Clone)]
pub struct CommonData {
    pub workspace: Arc<LapceWorkspace>,
    pub scope: Scope,
    pub focus: RwSignal<Focus>,
    pub keypress: RwSignal<KeyPressData>,
    pub completion: RwSignal<CompletionData>,
    pub register: RwSignal<Register>,
    pub find: Find,
    pub window_command: WriteSignal<Option<WindowCommand>>,
    pub internal_command: RwSignal<Option<InternalCommand>>,
    pub lapce_command: RwSignal<Option<LapceCommand>>,
    pub workbench_command: RwSignal<Option<LapceWorkbenchCommand>>,
    pub term_tx: Sender<(TermId, TermEvent)>,
    pub term_notification_tx: Sender<TermNotification>,
    pub proxy: ProxyRpcHandler,
    pub view_id: RwSignal<floem::id::Id>,
    pub ui_line_height: Memo<f64>,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

#[derive(Clone)]
pub struct WindowTabData {
    pub scope: Scope,
    pub window_tab_id: WindowTabId,
    pub workspace: Arc<LapceWorkspace>,
    pub palette: PaletteData,
    pub main_split: MainSplitData,
    pub file_explorer: FileExplorerData,
    pub panel: PanelData,
    pub terminal: TerminalPanelData,
    pub plugin: PluginData,
    pub code_action: RwSignal<CodeActionData>,
    pub source_control: RwSignal<SourceControlData>,
    pub rename: RenameData,
    pub global_search: GlobalSearchData,
    pub window_origin: RwSignal<Point>,
    pub layout_rect: RwSignal<Rect>,
    pub proxy: ProxyData,
    pub window_scale: RwSignal<f64>,
    pub common: CommonData,
}

impl KeyPressFocus for WindowTabData {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, condition: Condition) -> bool {
        if let Condition::PanelFocus = condition {
            if let Focus::Panel(_) = self.common.focus.get_untracked() {
                return true;
            }
        }
        false
    }

    fn run_command(
        &self,
        _command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
    ) -> CommandExecuted {
        CommandExecuted::No
    }

    fn receive_char(&self, _c: &str) {}
}

impl WindowTabData {
    pub fn new(
        cx: Scope,
        workspace: Arc<LapceWorkspace>,
        window_command: WriteSignal<Option<WindowCommand>>,
        window_scale: RwSignal<f64>,
    ) -> Self {
        let db: Arc<LapceDb> = use_context(cx).unwrap();

        let disabled_volts = db.get_disabled_volts().unwrap_or_default();
        let workspace_disabled_volts = db
            .get_workspace_disabled_volts(&workspace)
            .unwrap_or_default();
        let mut all_disabled_volts = disabled_volts.clone();
        all_disabled_volts.extend(workspace_disabled_volts.clone());

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
        let lapce_command = create_rw_signal(cx, None);
        let workbench_command = create_rw_signal(cx, None);
        let internal_command = create_rw_signal(cx, None);
        let keypress = create_rw_signal(
            cx,
            KeyPressData::new(&config, workbench_command.write_only()),
        );

        let (term_tx, term_rx) = crossbeam_channel::unbounded();
        let (term_notification_tx, term_notification_rx) =
            crossbeam_channel::unbounded();
        {
            let term_notification_tx = term_notification_tx.clone();
            std::thread::spawn(move || {
                terminal_update_process(term_rx, term_notification_tx);
            });
        }

        let proxy = start_proxy(
            cx,
            workspace.clone(),
            all_disabled_volts,
            config.plugins.clone(),
            term_tx.clone(),
        );
        let (config, _set_config) = create_signal(cx, Arc::new(config));

        let focus = create_rw_signal(cx, Focus::Workbench);
        let completion = create_rw_signal(cx, CompletionData::new(cx, config));

        let register = create_rw_signal(cx, Register::default());
        let view_id = create_rw_signal(cx, floem::id::Id::next());
        let find = Find::new(cx);

        let ui_line_height = create_memo(cx, move |_| {
            let config = config.get();
            let mut text_layout = TextLayout::new();

            let family: Vec<FamilyOwned> =
                FamilyOwned::parse_list(&config.ui.font_family).collect();
            let attrs = Attrs::new()
                .family(&family)
                .font_size(config.ui.font_size() as f32)
                .line_height(LineHeightValue::Normal(1.6));
            let attrs_list = AttrsList::new(attrs);
            text_layout.set_text("W", attrs_list);
            text_layout.size().height
        });

        let common = CommonData {
            workspace: workspace.clone(),
            scope: cx,
            keypress,
            focus,
            completion,
            register,
            find,
            window_command,
            internal_command,
            lapce_command,
            workbench_command,
            term_tx,
            term_notification_tx,
            proxy: proxy.rpc.clone(),
            view_id,
            ui_line_height,
            config,
        };

        let main_split = MainSplitData::new(cx, common.clone());
        let code_action =
            create_rw_signal(cx, CodeActionData::new(cx, common.clone()));
        let source_control =
            create_rw_signal(cx, SourceControlData::new(cx, common.clone()));
        let file_explorer = FileExplorerData::new(cx, common.clone());

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
                splits.insert(root_split, create_rw_signal(cx, root_split_data));
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
        let panel = PanelData::new(cx, panel_order, common.clone());

        let terminal =
            TerminalPanelData::new(workspace.clone(), None, common.clone());

        let rename = RenameData::new(cx, common.clone());
        let global_search =
            GlobalSearchData::new(cx, main_split.clone(), common.clone());

        let plugin = PluginData::new(
            cx,
            HashSet::from_iter(disabled_volts),
            HashSet::from_iter(workspace_disabled_volts),
            common.clone(),
        );

        {
            let notification = create_signal_from_channel(cx, term_notification_rx);
            let terminal = terminal.clone();
            create_effect(cx, move |_| {
                notification.with(|notification| {
                    if let Some(notification) = notification.as_ref() {
                        match notification {
                            TermNotification::SetTitle { term_id, title } => {
                                terminal.set_title(term_id, title);
                            }
                            TermNotification::RequestPaint => {
                                view_id.get_untracked().request_paint();
                            }
                        }
                    }
                });
            });
        }

        let window_tab_data = Self {
            scope: cx,
            window_tab_id: WindowTabId::next(),
            workspace,
            palette,
            main_split,
            terminal,
            panel,
            file_explorer,
            code_action,
            source_control,
            plugin,
            rename,
            global_search,
            window_origin: create_rw_signal(cx, Point::ZERO),
            layout_rect: create_rw_signal(cx, Rect::ZERO),
            proxy,
            window_scale,
            common,
        };

        {
            let window_tab_data = window_tab_data.clone();
            create_effect(cx, move |_| {
                if let Some(cmd) = window_tab_data.common.lapce_command.get() {
                    window_tab_data.run_lapce_command(cmd);
                }
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            create_effect(cx, move |_| {
                if let Some(cmd) = window_tab_data.common.workbench_command.get() {
                    window_tab_data.run_workbench_command(cmd, None);
                }
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            let internal_command = window_tab_data.common.internal_command;
            create_effect(cx, move |_| {
                if let Some(cmd) = internal_command.get() {
                    window_tab_data.run_internal_command(cmd);
                }
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            let notification = window_tab_data.proxy.notification;
            create_effect(cx, move |_| {
                notification.with(|rpc| {
                    if let Some(rpc) = rpc.as_ref() {
                        window_tab_data.handle_core_notification(rpc);
                    }
                });
            });
        }

        window_tab_data
    }

    pub fn run_lapce_command(&self, cmd: LapceCommand) {
        match cmd.kind {
            CommandKind::Workbench(command) => {
                self.run_workbench_command(command, cmd.data);
            }
            CommandKind::Edit(_) => {}
            CommandKind::Move(_) => {}
            CommandKind::Focus(_) => {}
            CommandKind::MotionMode(_) => {}
            CommandKind::MultiSelection(_) => {}
        }
    }

    pub fn run_workbench_command(
        &self,
        cmd: LapceWorkbenchCommand,
        data: Option<Value>,
    ) {
        let cx = self.scope;
        use LapceWorkbenchCommand::*;
        match cmd {
            EnableModal => {}
            DisableModal => {}
            OpenFolder => {
                if !self.workspace.kind.is_remote() {
                    let window_command = self.common.window_command;
                    let options = FileDialogOptions::new().select_directories();
                    self.common.view_id.get_untracked().open_file(
                        options,
                        move |file| {
                            if let Some(file) = file {
                                let workspace = LapceWorkspace {
                                    kind: LapceWorkspaceType::Local,
                                    path: Some(file.path),
                                    last_open: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs(),
                                };
                                window_command.set(Some(
                                    WindowCommand::SetWorkspace { workspace },
                                ));
                            }
                        },
                    );
                }
            }
            CloseFolder => {}
            OpenFile => {}
            RevealActiveFileInFileExplorer => {}
            ChangeColorTheme => {}
            ChangeIconTheme => {}
            OpenSettings => {
                self.main_split.open_settings();
            }
            OpenSettingsFile => {}
            OpenSettingsDirectory => {}
            OpenKeyboardShortcuts => {}
            OpenKeyboardShortcutsFile => {}
            OpenLogFile => {}
            OpenLogsDirectory => {}
            OpenProxyDirectory => {}
            OpenThemesDirectory => {}
            OpenPluginsDirectory => {}
            ZoomIn => {
                let mut scale = self.window_scale.get_untracked();
                scale += 0.1;
                if scale > 4.0 {
                    scale = 4.0
                }
                self.window_scale.set(scale);
            }
            ZoomOut => {
                let mut scale = self.window_scale.get_untracked();
                scale -= 0.1;
                if scale < 0.1 {
                    scale = 0.1
                }
                self.window_scale.set(scale);
            }
            ZoomReset => {
                self.window_scale.set(1.0);
            }
            NewWindowTab => {
                self.common.window_command.set(Some(
                    WindowCommand::NewWorkspaceTab {
                        workspace: LapceWorkspace::default(),
                        end: false,
                    },
                ));
            }
            CloseWindowTab => {
                self.common
                    .window_command
                    .set(Some(WindowCommand::CloseWorkspaceTab { index: None }));
            }
            NextWindowTab => {
                self.common
                    .window_command
                    .set(Some(WindowCommand::NextWorkspaceTab));
            }
            PreviousWindowTab => {
                self.common
                    .window_command
                    .set(Some(WindowCommand::PreviousWorkspaceTab));
            }
            NewTerminalTab => {
                self.terminal.new_tab(None);
                if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                    self.panel.show_panel(&PanelKind::Terminal);
                }
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }
            CloseTerminalTab => {
                self.terminal.close_tab(None);
                if self
                    .terminal
                    .tab_info
                    .with_untracked(|info| info.tabs.is_empty())
                {
                    if self.panel.is_panel_visible(&PanelKind::Terminal) {
                        self.panel.hide_panel(&PanelKind::Terminal);
                    }
                    self.common.focus.set(Focus::Workbench);
                } else {
                    if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                        self.panel.show_panel(&PanelKind::Terminal);
                    }
                    self.common.focus.set(Focus::Panel(PanelKind::Terminal));
                }
            }
            NextTerminalTab => {
                self.terminal.next_tab();
                if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                    self.panel.show_panel(&PanelKind::Terminal);
                }
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }
            PreviousTerminalTab => {
                self.terminal.previous_tab();
                if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                    self.panel.show_panel(&PanelKind::Terminal);
                }
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }
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
            PaletteRunAndDebug => {
                self.palette.run(cx, PaletteKind::RunAndDebug);
            }
            RunAndDebugRestart => {
                let active_term = self.terminal.debug.active_term.get_untracked();
                if active_term
                    .and_then(|term_id| self.terminal.restart_run_debug(term_id))
                    .is_none()
                {
                    self.palette.run(cx, PaletteKind::RunAndDebug);
                }
            }
            RunAndDebugStop => {
                let active_term = self.terminal.debug.active_term.get_untracked();
                if let Some(term_id) = active_term {
                    self.terminal.stop_run_debug(term_id);
                }
            }
            CheckoutBranch => {}
            ToggleMaximizedPanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.panel.toggle_maximize(&kind);
                    }
                } else {
                    self.panel.toggle_active_maximize();
                }
            }
            HidePanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.hide_panel(kind);
                    }
                }
            }
            ShowPanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.show_panel(kind);
                    }
                }
            }
            TogglePanelFocus => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.toggle_panel_focus(kind);
                    }
                }
            }
            TogglePanelVisual => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.toggle_panel_visual(kind);
                    }
                }
            }
            TogglePanelLeftVisual => {}
            TogglePanelRightVisual => {}
            TogglePanelBottomVisual => {}
            ToggleTerminalFocus => {
                self.toggle_panel_focus(PanelKind::Terminal);
            }
            ToggleSourceControlFocus => {
                self.toggle_panel_focus(PanelKind::SourceControl);
            }
            TogglePluginFocus => {
                self.toggle_panel_focus(PanelKind::Plugin);
            }
            ToggleFileExplorerFocus => {
                self.toggle_panel_focus(PanelKind::FileExplorer);
            }
            ToggleProblemFocus => {
                self.toggle_panel_focus(PanelKind::Problem);
            }
            ToggleSearchFocus => {
                self.toggle_panel_focus(PanelKind::Search);
            }
            ToggleTerminalVisual => {
                self.toggle_panel_visual(PanelKind::Terminal);
            }
            ToggleSourceControlVisual => {
                self.toggle_panel_visual(PanelKind::SourceControl);
            }
            TogglePluginVisual => {
                self.toggle_panel_visual(PanelKind::Plugin);
            }
            ToggleFileExplorerVisual => {
                self.toggle_panel_visual(PanelKind::FileExplorer);
            }
            ToggleProblemVisual => {
                self.toggle_panel_visual(PanelKind::Problem);
            }
            ToggleDebugVisual => {
                self.toggle_panel_visual(PanelKind::Debug);
            }
            ToggleSearchVisual => {
                self.toggle_panel_visual(PanelKind::Search);
            }
            FocusEditor => {
                self.common.focus.set(Focus::Workbench);
            }
            FocusTerminal => {
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }
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

    pub fn run_internal_command(&self, cmd: InternalCommand) {
        let cx = self.scope;
        match cmd {
            InternalCommand::OpenFile { path } => {
                self.main_split.jump_to_location(
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
            InternalCommand::EditorTabClose { editor_tab_id } => {
                self.main_split.editor_tab_close(cx, editor_tab_id);
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
            InternalCommand::ApplyWorkspaceEdit { edit } => {
                self.main_split.apply_workspace_edit(cx, &edit);
            }
            InternalCommand::SaveJumpLocation {
                path,
                offset,
                scroll_offset,
            } => {
                self.main_split
                    .save_jump_location(path, offset, scroll_offset);
            }
            InternalCommand::SplitTerminal { term_id } => {
                self.terminal.split(cx, term_id);
            }
            InternalCommand::SplitTerminalNext { term_id } => {
                self.terminal.split_next(cx, term_id);
            }
            InternalCommand::SplitTerminalPrevious { term_id } => {
                self.terminal.split_previous(cx, term_id);
            }
            InternalCommand::SplitTerminalExchange { term_id } => {
                self.terminal.split_exchange(cx, term_id);
            }
            InternalCommand::RunAndDebug { mode, config } => {
                self.run_and_debug(cx, &mode, &config);
            }
            InternalCommand::StartRename {
                path,
                placeholder,
                position,
                start,
            } => {
                self.rename.start(path, placeholder, start, position);
            }
            InternalCommand::Search { pattern } => {
                self.main_split.set_find_pattern(pattern);
            }
            InternalCommand::FindEditorReceiveChar { s } => {
                self.main_split.find_editor.receive_char(&s);
            }
            InternalCommand::ReplaceEditorReceiveChar { s } => {
                self.main_split.replace_editor.receive_char(&s);
            }
            InternalCommand::FindEditorCommand {
                command,
                count,
                mods,
            } => {
                self.main_split
                    .find_editor
                    .run_command(&command, count, mods);
            }
            InternalCommand::ReplaceEditorCommand {
                command,
                count,
                mods,
            } => {
                self.main_split
                    .replace_editor
                    .run_command(&command, count, mods);
            }
            InternalCommand::FocusEditorTab { editor_tab_id } => {
                self.main_split.active_editor_tab.set(Some(editor_tab_id));
            }
        }
    }

    fn handle_core_notification(&self, rpc: &CoreNotification) {
        let cx = self.scope;
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

                self.main_split
                    .get_diagnostic_data(&path)
                    .diagnostics
                    .set(diagnostics);

                // inform the document about the diagnostics
                if let Some(doc) = self
                    .main_split
                    .docs
                    .with_untracked(|docs| docs.get(&path).cloned())
                {
                    doc.update(|doc| doc.init_diagnostics());
                }
            }
            CoreNotification::TerminalProcessStopped { term_id } => {
                println!("terminal stopped {term_id:?}");
                let _ = self
                    .common
                    .term_tx
                    .send((*term_id, TermEvent::CloseTerminal));
                self.terminal.terminal_stopped(term_id);
                if self
                    .terminal
                    .tab_info
                    .with_untracked(|info| info.tabs.is_empty())
                {
                    if self.panel.is_panel_visible(&PanelKind::Terminal) {
                        self.panel.hide_panel(&PanelKind::Terminal);
                    }
                    self.common.focus.set(Focus::Workbench);
                }
            }
            CoreNotification::RunInTerminal { config } => {
                self.run_in_terminal(cx, &RunDebugMode::Debug, config);
            }
            CoreNotification::TerminalProcessId {
                term_id,
                process_id,
            } => {
                self.terminal.set_process_id(term_id, *process_id);
            }
            CoreNotification::DapStopped {
                dap_id,
                stopped,
                stack_frames,
            } => {
                self.terminal.dap_stopped(dap_id, stopped, stack_frames);
            }
            CoreNotification::DapContinued { dap_id } => {
                self.terminal.dap_continued(dap_id);
            }
            CoreNotification::OpenFileChanged { path, content } => {
                self.main_split.open_file_changed(path, content);
            }
            CoreNotification::VoltInstalled { volt, icon } => {
                self.plugin.volt_installed(volt, icon);
            }
            CoreNotification::VoltRemoved { volt, .. } => {
                self.plugin.volt_removed(volt);
            }
            _ => {}
        }
    }

    pub fn key_down(&self, key_event: &KeyEvent) {
        let focus = self.common.focus.get_untracked();
        let mut keypress = self.common.keypress.get_untracked();
        let executed = match focus {
            Focus::Workbench => {
                self.main_split.key_down(key_event, &mut keypress).is_some()
            }
            Focus::Palette => {
                keypress.key_down(key_event, &self.palette);
                true
            }
            Focus::CodeAction => {
                let code_action = self.code_action.get_untracked();
                keypress.key_down(key_event, &code_action);
                true
            }
            Focus::Rename => {
                keypress.key_down(key_event, &self.rename);
                true
            }
            Focus::Panel(PanelKind::Terminal) => {
                self.terminal.key_down(key_event, &mut keypress);
                true
            }
            Focus::Panel(PanelKind::Search) => {
                keypress.key_down(key_event, &self.global_search);
                true
            }
            Focus::Panel(PanelKind::Plugin) => {
                keypress.key_down(key_event, &self.plugin);
                true
            }
            _ => false,
        };

        if !executed {
            keypress.key_down(key_event, self);
        }

        self.common.keypress.set(keypress);
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

        let (window_origin, viewport, view) =
            editor.with_untracked(|e| (e.window_origin, e.viewport, e.view.clone()));

        let (point_above, point_below) = view.points_of_offset(completion.offset);

        let window_origin = window_origin.get() - self.window_origin.get().to_vec2();
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

        let (window_origin, viewport, view) =
            editor.with_untracked(|e| (e.window_origin, e.viewport, e.view.clone()));

        let (_point_above, point_below) = view.points_of_offset(code_action.offset);

        let window_origin = window_origin.get() - self.window_origin.get().to_vec2();
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

    pub fn rename_origin(&self) -> Point {
        let config = self.common.config.get();
        if !self.rename.active.get() {
            return Point::ZERO;
        }

        let tab_size = self.layout_rect.get().size();
        let rename_size = self.rename.layout_rect.get().size();

        let editor =
            if let Some(editor) = self.main_split.active_editor.get_untracked() {
                editor
            } else {
                return Point::ZERO;
            };

        let (window_origin, viewport, view) =
            editor.with_untracked(|e| (e.window_origin, e.viewport, e.view.clone()));

        let (_point_above, point_below) =
            view.points_of_offset(self.rename.start.get_untracked());

        let window_origin = window_origin.get() - self.window_origin.get().to_vec2();
        let viewport = viewport.get();

        let mut origin = window_origin
            + Vec2::new(point_below.x - viewport.x0, point_below.y - viewport.y0);

        if origin.y + rename_size.height > tab_size.height {
            origin.y =
                origin.y - config.editor.line_height() as f64 - rename_size.height;
        }
        if origin.x + rename_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - rename_size.width - 1.0;
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

    pub fn toggle_panel_visual(&self, kind: PanelKind) {
        if self.panel.is_panel_visible(&kind) {
            self.hide_panel(kind);
        } else {
            self.show_panel(kind);
        }
    }

    fn toggle_panel_focus(&self, kind: PanelKind) {
        let should_hide = match kind {
            PanelKind::FileExplorer
            | PanelKind::Plugin
            | PanelKind::Problem
            | PanelKind::Debug => {
                // Some panels don't accept focus (yet). Fall back to visibility check
                // in those cases.
                self.panel.is_panel_visible(&kind)
            }
            PanelKind::Terminal | PanelKind::SourceControl | PanelKind::Search => {
                self.is_panel_focused(kind)
            }
        };
        if should_hide {
            self.hide_panel(kind);
        } else {
            self.show_panel(kind);
        }
    }

    fn is_panel_focused(&self, kind: PanelKind) -> bool {
        // Moving between e.g. Search and Problems doesn't affect focus, so we need to also check
        // visibility.
        self.common.focus.get_untracked() == Focus::Panel(kind)
            && self.panel.is_panel_visible(&kind)
    }

    fn hide_panel(&self, kind: PanelKind) {
        self.panel.hide_panel(&kind);
        self.common.focus.set(Focus::Workbench);
    }

    pub fn show_panel(&self, kind: PanelKind) {
        if kind == PanelKind::Terminal
            && self
                .terminal
                .tab_info
                .with_untracked(|info| info.tabs.is_empty())
        {
            self.terminal.new_tab(None);
        }
        self.panel.show_panel(&kind);
        if kind == PanelKind::Search
            && self.common.focus.get_untracked() == Focus::Workbench
        {
            let active_editor = self.main_split.active_editor.get_untracked();
            let word = active_editor.map(|editor| {
                editor.with_untracked(|editor| editor.word_at_cursor())
            });
            if let Some(word) = word {
                if !word.is_empty() {
                    self.global_search.set_pattern(word);
                }
            }
        }
        self.common.focus.set(Focus::Panel(kind));
    }

    fn run_and_debug(
        &self,
        cx: Scope,
        mode: &RunDebugMode,
        config: &RunDebugConfig,
    ) {
        match mode {
            RunDebugMode::Run => {
                self.run_in_terminal(cx, mode, config);
            }
            RunDebugMode::Debug => {
                self.common.proxy.dap_start(
                    config.clone(),
                    self.terminal.debug.source_breakpoints(),
                );
            }
        }
    }

    fn run_in_terminal(
        &self,
        cx: Scope,
        mode: &RunDebugMode,
        config: &RunDebugConfig,
    ) {
        let term_id = if let Some(terminal) =
            self.terminal.get_stopped_run_debug_terminal(mode, config)
        {
            terminal.new_process(Some(RunDebugProcess {
                mode: *mode,
                config: config.clone(),
                stopped: false,
                created: Instant::now(),
            }));

            // let _ = ctx.get_external_handle().submit_command(
            //     LAPCE_UI_COMMAND,
            //     LapceUICommand::Focus,
            //     Target::Widget(terminal.widget_id),
            // );
            terminal.term_id
        } else {
            let new_terminal_tab = self.terminal.new_tab(Some(RunDebugProcess {
                mode: *mode,
                config: config.clone(),
                stopped: false,
                created: Instant::now(),
            }));
            new_terminal_tab.active_terminal(false).unwrap().term_id
        };
        self.common.focus.set(Focus::Panel(PanelKind::Terminal));
        self.terminal.focus_terminal(term_id);

        self.terminal.debug.active_term.set(Some(term_id));
        self.terminal.debug.daps.update(|daps| {
            daps.insert(config.dap_id, DapData::new(cx, config.dap_id, term_id));
        });

        if !self.panel.is_panel_visible(&PanelKind::Terminal) {
            self.panel.show_panel(&PanelKind::Terminal);
        }
    }
}
