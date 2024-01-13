use std::{
    collections::{BTreeMap, HashSet},
    env,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;
use floem::{
    action::{exec_after, open_file, TimerToken},
    cosmic_text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout},
    ext_event::{create_ext_action, create_signal_from_channel},
    file::FileDialogOptions,
    keyboard::ModifiersState,
    kurbo::Size,
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{use_context, Memo, ReadSignal, RwSignal, Scope, WriteSignal},
};
use indexmap::IndexMap;
use itertools::Itertools;
use lapce_core::{
    command::FocusCommand, cursor::CursorAffinity, directory::Directory, meta,
    mode::Mode, register::Register,
};
use lapce_rpc::{
    core::CoreNotification,
    dap_types::RunDebugConfig,
    file::{PathObject, RenameState},
    proxy::{ProxyResponse, ProxyRpcHandler, ProxyStatus},
    source_control::FileDiff,
    terminal::TermId,
    RpcError,
};
use lsp_types::{ProgressParams, ProgressToken, ShowMessageParams};
use serde_json::Value;
use tracing::{debug, error};

use crate::{
    about::AboutData,
    alert::{AlertBoxData, AlertButton},
    code_action::{CodeActionData, CodeActionStatus},
    command::{
        CommandExecuted, CommandKind, InternalCommand, LapceCommand,
        LapceWorkbenchCommand, WindowCommand,
    },
    completion::{CompletionData, CompletionStatus},
    config::LapceConfig,
    db::LapceDb,
    debug::{DapData, LapceBreakpoint, RunDebugMode, RunDebugProcess},
    doc::{DocContent, DocumentExt, EditorDiagnostic},
    editor::location::{EditorLocation, EditorPosition},
    editor_tab::EditorTabChild,
    file_explorer::data::FileExplorerData,
    find::Find,
    global_search::GlobalSearchData,
    hover::HoverData,
    id::WindowTabId,
    inline_completion::InlineCompletionData,
    keypress::{condition::Condition, EventRef, KeyPressData, KeyPressFocus},
    listener::Listener,
    main_split::{MainSplitData, SplitData, SplitDirection, SplitMoveDirection},
    palette::{kind::PaletteKind, PaletteData, PaletteStatus},
    panel::{
        data::{default_panel_order, PanelData},
        kind::PanelKind,
        position::PanelContainerPosition,
    },
    plugin::PluginData,
    proxy::{new_proxy, path_from_url, ProxyData},
    rename::RenameData,
    source_control::SourceControlData,
    terminal::{
        event::{terminal_update_process, TermEvent, TermNotification},
        panel::TerminalPanelData,
    },
    window::WindowCommonData,
    workspace::{LapceWorkspace, LapceWorkspaceType, WorkspaceInfo},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Workbench,
    Palette,
    CodeAction,
    Rename,
    AboutPopup,
    Panel(PanelKind),
}

#[derive(Clone)]
pub enum DragContent {
    Panel(PanelKind),
    EditorTab(EditorTabChild),
}

impl DragContent {
    pub fn is_panel(&self) -> bool {
        matches!(self, DragContent::Panel(_))
    }
}

#[derive(Clone)]
pub struct WorkProgress {
    pub token: ProgressToken,
    pub title: String,
    pub message: Option<String>,
    pub percentage: Option<u32>,
}

#[derive(Clone)]
pub struct CommonData {
    pub workspace: Arc<LapceWorkspace>,
    pub scope: Scope,
    pub focus: RwSignal<Focus>,
    pub keypress: RwSignal<KeyPressData>,
    pub completion: RwSignal<CompletionData>,
    pub inline_completion: RwSignal<InlineCompletionData>,
    pub hover: HoverData,
    pub register: RwSignal<Register>,
    pub find: Find,
    pub workbench_size: RwSignal<Size>,
    pub window_origin: RwSignal<Point>,
    pub internal_command: Listener<InternalCommand>,
    pub lapce_command: Listener<LapceCommand>,
    pub workbench_command: Listener<LapceWorkbenchCommand>,
    pub term_tx: Sender<(TermId, TermEvent)>,
    pub term_notification_tx: Sender<TermNotification>,
    pub proxy: ProxyRpcHandler,
    pub view_id: RwSignal<floem::id::Id>,
    pub ui_line_height: Memo<f64>,
    pub dragging: RwSignal<Option<DragContent>>,
    pub config: ReadSignal<Arc<LapceConfig>>,
    pub proxy_status: RwSignal<Option<ProxyStatus>>,
    pub mouse_hover_timer: RwSignal<TimerToken>,
    pub breakpoints: RwSignal<BTreeMap<PathBuf, BTreeMap<usize, LapceBreakpoint>>>,
    // the current focused view which will receive keyboard events
    pub keyboard_focus: RwSignal<Option<floem::id::Id>>,
    pub window_common: Rc<WindowCommonData>,
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
    pub source_control: SourceControlData,
    pub rename: RenameData,
    pub global_search: GlobalSearchData,
    pub about_data: AboutData,
    pub alert_data: AlertBoxData,
    pub layout_rect: RwSignal<Rect>,
    pub title_height: RwSignal<f64>,
    pub status_height: RwSignal<f64>,
    pub proxy: ProxyData,
    pub set_config: WriteSignal<Arc<LapceConfig>>,
    pub update_in_progress: RwSignal<bool>,
    pub progresses: RwSignal<IndexMap<ProgressToken, WorkProgress>>,
    pub messages: RwSignal<Vec<(String, ShowMessageParams)>>,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for WindowTabData {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, condition: Condition) -> bool {
        match condition {
            Condition::PanelFocus => {
                matches!(self.common.focus.get_untracked(), Focus::Panel(_))
            }
            Condition::SourceControlFocus => {
                self.common.focus.get_untracked()
                    == Focus::Panel(PanelKind::SourceControl)
            }
            _ => false,
        }
    }

    fn run_command(
        &self,
        command: &LapceCommand,
        _count: Option<usize>,
        _mods: ModifiersState,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(cmd) => {
                self.run_workbench_command(cmd.clone(), None);
            }
            CommandKind::Focus(cmd) => {
                if self.common.focus.get_untracked() == Focus::Workbench {
                    match cmd {
                        FocusCommand::SplitClose => {
                            self.main_split.editor_tab_child_close_active();
                        }
                        FocusCommand::SplitVertical => {
                            self.main_split.split_active(SplitDirection::Vertical);
                        }
                        FocusCommand::SplitHorizontal => {
                            self.main_split.split_active(SplitDirection::Horizontal);
                        }
                        FocusCommand::SplitRight => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Right);
                        }
                        FocusCommand::SplitLeft => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Left);
                        }
                        FocusCommand::SplitUp => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Up);
                        }
                        FocusCommand::SplitDown => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Down);
                        }
                        FocusCommand::SplitExchange => {
                            self.main_split.split_exchange_active();
                        }
                        _ => {
                            return CommandExecuted::No;
                        }
                    }
                }
            }
            _ => {
                return CommandExecuted::No;
            }
        }

        CommandExecuted::Yes
    }

    fn receive_char(&self, _c: &str) {}
}

impl WindowTabData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cx: Scope,
        workspace: Arc<LapceWorkspace>,
        window_common: Rc<WindowCommonData>,
    ) -> Self {
        let cx = cx.create_child();
        let db: Arc<LapceDb> = use_context().unwrap();

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
        let lapce_command = Listener::new_empty(cx);
        let workbench_command = Listener::new_empty(cx);
        let internal_command = Listener::new_empty(cx);
        let keypress = cx.create_rw_signal(KeyPressData::new(cx, &config));
        let proxy_status = cx.create_rw_signal(None);

        let (term_tx, term_rx) = crossbeam_channel::unbounded();
        let (term_notification_tx, term_notification_rx) =
            crossbeam_channel::unbounded();
        {
            let term_notification_tx = term_notification_tx.clone();
            std::thread::spawn(move || {
                terminal_update_process(term_rx, term_notification_tx);
            });
        }

        let proxy = new_proxy(
            workspace.clone(),
            all_disabled_volts,
            config.plugins.clone(),
            term_tx.clone(),
        );
        let (config, set_config) = cx.create_signal(Arc::new(config));

        let focus = cx.create_rw_signal(Focus::Workbench);
        let completion = cx.create_rw_signal(CompletionData::new(cx, config));
        let inline_completion = cx.create_rw_signal(InlineCompletionData::new(cx));
        let hover = HoverData::new(cx);

        let register = cx.create_rw_signal(Register::default());
        let view_id = cx.create_rw_signal(floem::id::Id::next());
        let find = Find::new(cx);

        let ui_line_height = cx.create_memo(move |_| {
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

        let common = Rc::new(CommonData {
            workspace: workspace.clone(),
            scope: cx,
            keypress,
            focus,
            completion,
            inline_completion,
            hover,
            register,
            find,
            internal_command,
            lapce_command,
            workbench_command,
            term_tx,
            term_notification_tx,
            proxy: proxy.proxy_rpc.clone(),
            view_id,
            ui_line_height,
            dragging: cx.create_rw_signal(None),
            workbench_size: cx.create_rw_signal(Size::ZERO),
            config,
            proxy_status,
            mouse_hover_timer: cx.create_rw_signal(TimerToken::INVALID),
            window_origin: cx.create_rw_signal(Point::ZERO),
            breakpoints: cx.create_rw_signal(BTreeMap::new()),
            keyboard_focus: cx.create_rw_signal(None),
            window_common: window_common.clone(),
        });

        let main_split = MainSplitData::new(cx, common.clone());
        let code_action =
            cx.create_rw_signal(CodeActionData::new(cx, common.clone()));
        let source_control = SourceControlData::new(cx, common.clone());
        let file_explorer = FileExplorerData::new(cx, common.clone());

        if let Some(info) = workspace_info.as_ref() {
            let root_split = main_split.root_split;
            info.split.to_data(main_split.clone(), None, root_split);
        } else {
            let root_split = main_split.root_split;
            let root_split_data = {
                let cx = cx.create_child();
                let root_split_data = SplitData {
                    scope: cx,
                    parent_split: None,
                    split_id: root_split,
                    children: Vec::new(),
                    direction: SplitDirection::Horizontal,
                    window_origin: Point::ZERO,
                    layout_rect: Rect::ZERO,
                };
                cx.create_rw_signal(root_split_data)
            };
            main_split.splits.update(|splits| {
                splits.insert(root_split, root_split_data);
            });
        }

        let palette = PaletteData::new(
            cx,
            workspace.clone(),
            main_split.clone(),
            keypress.read_only(),
            source_control.clone(),
            common.clone(),
        );

        let title_height = cx.create_rw_signal(0.0);
        let status_height = cx.create_rw_signal(0.0);
        let panel_available_size = cx.create_memo(move |_| {
            let title_height = title_height.get();
            let status_height = status_height.get();
            let num_window_tabs = window_common.num_window_tabs.get();
            let window_size = window_common.size.get();
            Size::new(
                window_size.width,
                window_size.height
                    - title_height
                    - status_height
                    - if num_window_tabs > 1 {
                        window_common.window_tab_header_height.get()
                    } else {
                        0.0
                    },
            )
        });
        let panel = workspace_info
            .as_ref()
            .map(|i| {
                let panel_order = db
                    .get_panel_orders()
                    .unwrap_or_else(|_| i.panel.panels.clone());
                PanelData {
                    panels: cx.create_rw_signal(panel_order),
                    styles: cx.create_rw_signal(i.panel.styles.clone()),
                    size: cx.create_rw_signal(i.panel.size.clone()),
                    available_size: panel_available_size,
                    common: common.clone(),
                }
            })
            .unwrap_or_else(|| {
                let panel_order = db
                    .get_panel_orders()
                    .unwrap_or_else(|_| default_panel_order());
                PanelData::new(cx, panel_order, panel_available_size, common.clone())
            });

        let terminal =
            TerminalPanelData::new(workspace.clone(), None, None, common.clone());
        if let Some(workspace_info) = workspace_info.as_ref() {
            terminal.debug.breakpoints.set(
                workspace_info
                    .breakpoints
                    .clone()
                    .into_iter()
                    .map(|(path, breakpoints)| {
                        (
                            path,
                            breakpoints
                                .into_iter()
                                .map(|b| (b.line, b))
                                .collect::<BTreeMap<usize, LapceBreakpoint>>(),
                        )
                    })
                    .collect(),
            );
        }

        let rename = RenameData::new(cx, common.clone());
        let global_search = GlobalSearchData::new(cx, main_split.clone());

        let plugin = PluginData::new(
            cx,
            HashSet::from_iter(disabled_volts),
            HashSet::from_iter(workspace_disabled_volts),
            common.clone(),
        );

        {
            let notification = create_signal_from_channel(term_notification_rx);
            let terminal = terminal.clone();
            cx.create_effect(move |_| {
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

        let about_data = AboutData::new(cx, common.focus);
        let alert_data = AlertBoxData::new(cx, common.clone());

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
            about_data,
            alert_data,
            layout_rect: cx.create_rw_signal(Rect::ZERO),
            title_height,
            status_height,
            proxy,
            set_config,
            update_in_progress: cx.create_rw_signal(false),
            progresses: cx.create_rw_signal(IndexMap::new()),
            messages: cx.create_rw_signal(Vec::new()),
            common,
        };

        {
            let focus = window_tab_data.common.focus;
            let active_editor = window_tab_data.main_split.active_editor;
            let rename_active = window_tab_data.rename.active;
            let internal_command = window_tab_data.common.internal_command;
            cx.create_effect(move |_| {
                let focus = focus.get();
                active_editor.track();
                internal_command.send(InternalCommand::ResetBlinkCursor);

                if focus != Focus::Rename && rename_active.get_untracked() {
                    rename_active.set(false);
                }
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            window_tab_data.common.lapce_command.listen(move |cmd| {
                window_tab_data.run_lapce_command(cmd);
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            window_tab_data.common.workbench_command.listen(move |cmd| {
                window_tab_data.run_workbench_command(cmd, None);
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            let internal_command = window_tab_data.common.internal_command;
            internal_command.listen(move |cmd| {
                window_tab_data.run_internal_command(cmd);
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            let notification = window_tab_data.proxy.notification;
            cx.create_effect(move |_| {
                notification.with(|rpc| {
                    if let Some(rpc) = rpc.as_ref() {
                        window_tab_data.handle_core_notification(rpc);
                    }
                });
            });
        }

        window_tab_data
    }

    pub fn reload_config(&self) {
        let db: Arc<LapceDb> = use_context().unwrap();

        let disabled_volts = db.get_disabled_volts().unwrap_or_default();
        let workspace_disabled_volts = db
            .get_workspace_disabled_volts(&self.workspace)
            .unwrap_or_default();
        let mut all_disabled_volts = disabled_volts;
        all_disabled_volts.extend(workspace_disabled_volts);

        let config = LapceConfig::load(&self.workspace, &all_disabled_volts);
        self.common.keypress.update(|keypress| {
            keypress.update_keymaps(&config);
        });
        self.set_config.set(Arc::new(config));
    }

    pub fn run_lapce_command(&self, cmd: LapceCommand) {
        match cmd.kind {
            CommandKind::Workbench(command) => {
                self.run_workbench_command(command, cmd.data);
            }
            CommandKind::Focus(_) | CommandKind::Edit(_) | CommandKind::Move(_) => {
                if self.palette.status.get_untracked() != PaletteStatus::Inactive {
                    self.palette
                        .run_command(&cmd, None, ModifiersState::empty());
                } else if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.run_command(&cmd, None, ModifiersState::empty());
                } else {
                    // TODO: dispatch to current focused view?
                }
            }
            CommandKind::MotionMode(_) => {}
            CommandKind::MultiSelection(_) => {}
        }
    }

    pub fn run_workbench_command(
        &self,
        cmd: LapceWorkbenchCommand,
        data: Option<Value>,
    ) {
        use LapceWorkbenchCommand::*;
        match cmd {
            // ==== Modal ====
            EnableModal => {
                let internal_command = self.common.internal_command;
                internal_command.send(InternalCommand::SetModal { modal: true });
            }
            DisableModal => {
                let internal_command = self.common.internal_command;
                internal_command.send(InternalCommand::SetModal { modal: false });
            }

            // ==== Files / Folders ====
            OpenFolder => {
                if !self.workspace.kind.is_remote() {
                    let window_command = self.common.window_common.window_command;
                    let options = FileDialogOptions::new().select_directories();
                    open_file(options, move |file| {
                        if let Some(file) = file {
                            let workspace = LapceWorkspace {
                                kind: LapceWorkspaceType::Local,
                                path: Some(file.path),
                                last_open: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            };
                            window_command
                                .send(WindowCommand::SetWorkspace { workspace });
                        }
                    });
                }
            }
            CloseFolder => {
                if !self.workspace.kind.is_remote() {
                    let window_command = self.common.window_common.window_command;
                    let workspace = LapceWorkspace {
                        kind: LapceWorkspaceType::Local,
                        path: None,
                        last_open: 0,
                    };
                    window_command.send(WindowCommand::SetWorkspace { workspace });
                }
            }
            OpenFile => {
                if !self.workspace.kind.is_remote() {
                    let internal_command = self.common.internal_command;
                    let options = FileDialogOptions::new();
                    open_file(options, move |file| {
                        if let Some(file) = file {
                            internal_command
                                .send(InternalCommand::OpenFile { path: file.path })
                        }
                    });
                }
            }
            NewFile => {
                self.main_split.new_file();
            }
            RevealActiveFileInFileExplorer => {
                if let Some(editor_data) = self.main_split.active_editor.get() {
                    let path = editor_data.view.doc.with_untracked(|doc| {
                        if let DocContent::File { path, .. } =
                            doc.content.get_untracked()
                        {
                            Some(path)
                        } else {
                            None
                        }
                    });
                    let Some(path) = path else { return };
                    let path = path.parent().unwrap_or(&path);

                    open_uri(path);
                }
            }

            SaveAll => {
                self.main_split.editors.with_untracked(|editors| {
                    let mut paths = HashSet::new();
                    for (_, editor_data) in editors.iter() {
                        let should_save =
                            editor_data.view.doc.with_untracked(|doc| {
                                let DocContent::File { path, .. } =
                                    doc.content.get_untracked()
                                else {
                                    return false;
                                };

                                if paths.contains(&path) {
                                    return false;
                                }

                                paths.insert(path.clone());

                                true
                            });

                        if should_save {
                            editor_data.save(true, || {});
                        }
                    }
                });
            }

            // ==== Configuration / Info Files and Folders ====
            OpenSettings => {
                self.main_split.open_settings();
            }
            OpenSettingsFile => {
                if let Some(path) = LapceConfig::settings_file() {
                    self.main_split.jump_to_location(
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
            }
            OpenSettingsDirectory => {
                if let Some(dir) = Directory::config_directory() {
                    open_uri(&dir);
                }
            }
            OpenThemeColorSettings => {
                self.main_split.open_theme_color_settings();
            }
            OpenKeyboardShortcuts => {
                self.main_split.open_keymap();
            }
            OpenKeyboardShortcutsFile => {
                if let Some(path) = LapceConfig::keymaps_file() {
                    self.main_split.jump_to_location(
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
            }
            OpenLogFile => {
                if let Some(dir) = Directory::logs_directory() {
                    self.open_paths(&[PathObject::from_path(
                        dir.join(format!(
                            "lapce.{}.log",
                            chrono::prelude::Local::now().format("%Y-%m-%d")
                        )),
                        false,
                    )])
                }
            }
            OpenLogsDirectory => {
                if let Some(dir) = Directory::logs_directory() {
                    open_uri(&dir);
                }
            }
            OpenProxyDirectory => {
                if let Some(dir) = Directory::proxy_directory() {
                    open_uri(&dir);
                }
            }
            OpenThemesDirectory => {
                if let Some(dir) = Directory::themes_directory() {
                    open_uri(&dir);
                }
            }
            OpenPluginsDirectory => {
                if let Some(dir) = Directory::plugins_directory() {
                    open_uri(&dir);
                }
            }

            InstallTheme => {}
            ExportCurrentThemeSettings => {
                self.main_split.export_theme();
            }
            ToggleInlayHints => {}

            // ==== Window ====
            ReloadWindow => {
                self.common.window_common.window_command.send(
                    WindowCommand::SetWorkspace {
                        workspace: (*self.workspace).clone(),
                    },
                );
            }
            NewWindow => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::NewWindow);
            }
            CloseWindow => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::CloseWindow);
            }
            // ==== Window Tabs ====
            NewWindowTab => {
                self.common.window_common.window_command.send(
                    WindowCommand::NewWorkspaceTab {
                        workspace: LapceWorkspace::default(),
                        end: false,
                    },
                );
            }
            CloseWindowTab => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::CloseWorkspaceTab { index: None });
            }
            NextWindowTab => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::NextWorkspaceTab);
            }
            PreviousWindowTab => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::PreviousWorkspaceTab);
            }

            // ==== Editor Tabs ====
            NextEditorTab => {
                if let Some(editor_tab_id) =
                    self.main_split.active_editor_tab.get_untracked()
                {
                    self.main_split.editor_tabs.with_untracked(|editor_tabs| {
                        let Some(editor_tab) = editor_tabs.get(&editor_tab_id)
                        else {
                            return;
                        };

                        let new_index = editor_tab.with_untracked(|editor_tab| {
                            if editor_tab.children.is_empty() {
                                None
                            } else if editor_tab.active
                                == editor_tab.children.len() - 1
                            {
                                Some(0)
                            } else {
                                Some(editor_tab.active + 1)
                            }
                        });

                        if let Some(new_index) = new_index {
                            editor_tab.update(|editor_tab| {
                                editor_tab.active = new_index;
                            });
                        }
                    });
                }
            }
            PreviousEditorTab => {
                if let Some(editor_tab_id) =
                    self.main_split.active_editor_tab.get_untracked()
                {
                    self.main_split.editor_tabs.with_untracked(|editor_tabs| {
                        let Some(editor_tab) = editor_tabs.get(&editor_tab_id)
                        else {
                            return;
                        };

                        let new_index = editor_tab.with_untracked(|editor_tab| {
                            if editor_tab.children.is_empty() {
                                None
                            } else if editor_tab.active == 0 {
                                Some(editor_tab.children.len() - 1)
                            } else {
                                Some(editor_tab.active - 1)
                            }
                        });

                        if let Some(new_index) = new_index {
                            editor_tab.update(|editor_tab| {
                                editor_tab.active = new_index;
                            });
                        }
                    });
                }
            }

            // ==== Terminal ====
            NewTerminalTab => {
                self.terminal.new_tab(
                    None,
                    self.common
                        .config
                        .get_untracked()
                        .terminal
                        .get_default_profile(),
                );
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

            // ==== Remote ====
            ConnectSshHost => {
                self.palette.run(PaletteKind::SshHost);
            }
            #[cfg(windows)]
            ConnectWslHost => {
                self.palette.run(PaletteKind::WslHost);
            }
            DisconnectRemote => {
                self.common.window_common.window_command.send(
                    WindowCommand::SetWorkspace {
                        workspace: LapceWorkspace {
                            kind: LapceWorkspaceType::Local,
                            path: None,
                            last_open: 0,
                        },
                    },
                );
            }

            // ==== Palette Commands ====
            PaletteHelp => self.palette.run(PaletteKind::PaletteHelp),
            PaletteLine => {
                self.palette.run(PaletteKind::Line);
            }
            Palette => {
                self.palette.run(PaletteKind::File);
            }
            PaletteSymbol => {
                self.palette.run(PaletteKind::DocumentSymbol);
            }
            PaletteWorkspaceSymbol => {}
            PaletteCommand => {
                self.palette.run(PaletteKind::Command);
            }
            PaletteWorkspace => {
                self.palette.run(PaletteKind::Workspace);
            }
            PaletteRunAndDebug => {
                self.palette.run(PaletteKind::RunAndDebug);
            }
            PaletteSCMReferences => {
                self.palette.run(PaletteKind::SCMReferences);
            }
            ChangeColorTheme => {
                self.palette.run(PaletteKind::ColorTheme);
            }
            ChangeIconTheme => {
                self.palette.run(PaletteKind::IconTheme);
            }
            ChangeFileLanguage => {
                self.palette.run(PaletteKind::Language);
            }
            DiffFiles => self.palette.run(PaletteKind::DiffFiles),

            // ==== Running / Debugging ====
            RunAndDebugRestart => {
                let active_term = self.terminal.debug.active_term.get_untracked();
                if active_term
                    .and_then(|term_id| self.terminal.restart_run_debug(term_id))
                    .is_none()
                {
                    self.palette.run(PaletteKind::RunAndDebug);
                }
            }
            RunAndDebugStop => {
                let active_term = self.terminal.debug.active_term.get_untracked();
                if let Some(term_id) = active_term {
                    self.terminal.stop_run_debug(term_id);
                }
            }

            // ==== UI ====
            ZoomIn => {
                let mut scale =
                    self.common.window_common.window_scale.get_untracked();
                scale += 0.1;
                if scale > 4.0 {
                    scale = 4.0
                }
                self.common.window_common.window_scale.set(scale);
            }
            ZoomOut => {
                let mut scale =
                    self.common.window_common.window_scale.get_untracked();
                scale -= 0.1;
                if scale < 0.1 {
                    scale = 0.1
                }
                self.common.window_common.window_scale.set(scale);
            }
            ZoomReset => {
                self.common.window_common.window_scale.set(1.0);
            }

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
            TogglePanelLeftVisual => {
                self.toggle_container_visual(&PanelContainerPosition::Left);
            }
            TogglePanelRightVisual => {
                self.toggle_container_visual(&PanelContainerPosition::Right);
            }
            TogglePanelBottomVisual => {
                self.toggle_container_visual(&PanelContainerPosition::Bottom);
            }
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
            OpenUIInspector => {
                self.common.view_id.get_untracked().inspect();
            }

            // ==== Source Control ====
            SourceControlInit => {
                self.proxy.proxy_rpc.git_init();
            }
            CheckoutReference => match data {
                Some(reference) => {
                    if let Some(reference) = reference.as_str() {
                        self.proxy.proxy_rpc.git_checkout(reference.to_string());
                    }
                }
                None => error!("No ref provided"),
            },
            SourceControlCommit => {
                self.source_control.commit();
            }
            SourceControlCopyActiveFileRemoteUrl => {
                // TODO:
            }
            SourceControlDiscardActiveFileChanges => {
                // TODO:
            }
            SourceControlDiscardTargetFileChanges => {
                if let Some(diff) = data
                    .and_then(|data| serde_json::from_value::<FileDiff>(data).ok())
                {
                    match diff {
                        FileDiff::Added(path) => {
                            self.common.proxy.trash_path(path, Box::new(|_| {}));
                        }
                        FileDiff::Modified(path) | FileDiff::Deleted(path) => {
                            self.common.proxy.git_discard_files_changes(vec![path]);
                        }
                        FileDiff::Renamed(old_path, new_path) => {
                            self.common
                                .proxy
                                .git_discard_files_changes(vec![old_path]);
                            self.common.proxy.trash_path(new_path, Box::new(|_| {}));
                        }
                    }
                }
            }
            SourceControlDiscardWorkspaceChanges => {
                // TODO:
            }

            // ==== UI ====
            ShowAbout => {
                self.about_data.open();
            }

            // ==== Updating ====
            RestartToUpdate => {
                if let Some(release) = self
                    .common
                    .window_common
                    .latest_release
                    .get_untracked()
                    .as_ref()
                {
                    let release = release.clone();
                    let update_in_progress = self.update_in_progress;
                    if release.version != *meta::VERSION {
                        if let Ok(process_path) = env::current_exe() {
                            update_in_progress.set(true);
                            let send = create_ext_action(
                                self.common.scope,
                                move |_started| {
                                    update_in_progress.set(false);
                                },
                            );
                            std::thread::spawn(move || {
                                let do_update = || -> anyhow::Result<()> {
                                    let src =
                                        crate::update::download_release(&release)?;

                                    let path =
                                        crate::update::extract(&src, &process_path)?;

                                    crate::update::restart(&path)?;

                                    Ok(())
                                };

                                if let Err(err) = do_update() {
                                    error!("Failed to update: {err}");
                                }

                                send(false);
                            });
                        }
                    }
                }
            }

            // ==== Movement ====
            #[cfg(target_os = "macos")]
            InstallToPATH => {}
            #[cfg(target_os = "macos")]
            UninstallFromPATH => {}
            JumpLocationForward => {
                self.main_split.jump_location_forward(false);
            }
            JumpLocationBackward => {
                self.main_split.jump_location_backward(false);
            }
            JumpLocationForwardLocal => {
                self.main_split.jump_location_forward(true);
            }
            JumpLocationBackwardLocal => {
                self.main_split.jump_location_backward(true);
            }
            NextError => {
                self.main_split.next_error();
            }
            PreviousError => {}
            Quit => {
                floem::quit_app();
            }
        }
    }

    pub fn run_internal_command(&self, cmd: InternalCommand) {
        let cx = self.scope;
        match cmd {
            InternalCommand::ReloadConfig => {
                self.reload_config();
            }
            InternalCommand::UpdateLogLevel { level } => {
                // TODO: implement logging panel, runtime log level change
                debug!("{level}");
            }
            InternalCommand::MakeConfirmed => {
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    editor.confirmed.set(true);
                }
            }
            InternalCommand::OpenFile { path } => {
                self.main_split.jump_to_location(
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
            InternalCommand::OpenFileInNewTab { path } => {
                self.main_split.jump_to_location(
                    EditorLocation {
                        path,
                        position: None,
                        scroll_offset: None,
                        ignore_unconfirmed: true,
                        same_editor_tab: false,
                    },
                    None,
                );
            }
            InternalCommand::OpenFileChanges { path } => {
                self.main_split.open_file_changes(path);
            }
            InternalCommand::StartRenamePath { path } => {
                self.file_explorer.rename_state.set(RenameState::Renaming {
                    path,
                    editor_needs_reset: true,
                });
            }
            InternalCommand::TestRenamePath { new_path } => {
                let rename_state = self.file_explorer.rename_state;

                let send = create_ext_action(
                    self.scope,
                    move |response: Result<ProxyResponse, RpcError>| match response {
                        Ok(_) => {
                            rename_state.update(RenameState::set_ok);
                        }
                        Err(err) => {
                            rename_state.update(|rename_state| {
                                rename_state.set_err(err.message)
                            });
                        }
                    },
                );

                self.common.proxy.test_create_at_path(new_path, send);
            }
            InternalCommand::FinishRenamePath {
                current_path,
                new_path,
            } => {
                let send_current_path = current_path.clone();
                let send_new_path = new_path.clone();
                let file_explorer = self.file_explorer.clone();
                let editors = self.main_split.editors;

                let send = create_ext_action(
                    self.scope,
                    move |response: Result<ProxyResponse, RpcError>| match response {
                        Ok(response) => {
                            // Get the canonicalized new path from the proxy.
                            let new_path =
                                if let ProxyResponse::CreatePathResponse { path } =
                                    response
                                {
                                    path
                                } else {
                                    send_new_path
                                };

                            // If the renamed item is a file, update any editors the file is open
                            // in to use the new path.
                            // If the renamed item is a directory, update any editors in which a
                            // file the renamed directory is an ancestor of is open to use the
                            // file's new path.
                            let renamed_editors_content: Vec<_> = editors
                                .with_untracked(|editors| {
                                    editors
                                        .values()
                                        .map(|editor| {
                                            editor
                                                .view
                                                .doc
                                                .with_untracked(|doc| doc.content)
                                        })
                                        .filter(|content| {
                                            content.with_untracked(|content| {
                                                match content {
                                                    DocContent::File {
                                                        path,
                                                        ..
                                                    } => path.starts_with(
                                                        &send_current_path,
                                                    ),
                                                    _ => false,
                                                }
                                            })
                                        })
                                        .collect()
                                });

                            for content in renamed_editors_content {
                                content.update(|content| {
                                    if let DocContent::File { path, .. } = content {
                                        if let Ok(suffix) =
                                            path.strip_prefix(&send_current_path)
                                        {
                                            *path = new_path.join(suffix);
                                        }
                                    }
                                });
                            }

                            file_explorer.reload();
                            file_explorer.rename_state.set(RenameState::NotRenaming);
                        }
                        Err(err) => {
                            file_explorer.rename_state.update(|rename_state| {
                                rename_state.set_err(err.message)
                            });
                        }
                    },
                );

                self.file_explorer
                    .rename_state
                    .update(RenameState::set_pending);
                self.common
                    .proxy
                    .rename_path(current_path.clone(), new_path, send);
            }
            InternalCommand::GoToLocation { location } => {
                self.main_split.go_to_location(location, None);
            }
            InternalCommand::JumpToLocation { location } => {
                self.main_split.jump_to_location(location, None);
            }
            InternalCommand::PaletteReferences { references } => {
                self.palette.references.set(references);
                self.palette.run(PaletteKind::Reference);
            }
            InternalCommand::Split {
                direction,
                editor_tab_id,
            } => {
                self.main_split.split(direction, editor_tab_id);
            }
            InternalCommand::SplitMove {
                direction,
                editor_tab_id,
            } => {
                self.main_split.split_move(direction, editor_tab_id);
            }
            InternalCommand::SplitExchange { editor_tab_id } => {
                self.main_split.split_exchange(editor_tab_id);
            }
            InternalCommand::EditorTabClose { editor_tab_id } => {
                self.main_split.editor_tab_close(editor_tab_id);
            }
            InternalCommand::EditorTabChildClose {
                editor_tab_id,
                child,
            } => {
                self.main_split
                    .editor_tab_child_close(editor_tab_id, child, false);
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
                self.main_split.run_code_action(plugin_id, action);
            }
            InternalCommand::ApplyWorkspaceEdit { edit } => {
                self.main_split.apply_workspace_edit(&edit);
            }
            InternalCommand::SaveJumpLocation {
                path,
                offset,
                scroll_offset,
            } => {
                self.main_split
                    .save_jump_location(path, offset, scroll_offset);
            }
            InternalCommand::NewTerminal { profile } => {
                self.terminal.new_tab(None, profile);
            }
            InternalCommand::SplitTerminal { term_id } => {
                self.terminal.split(term_id);
            }
            InternalCommand::SplitTerminalNext { term_id } => {
                self.terminal.split_next(term_id);
            }
            InternalCommand::SplitTerminalPrevious { term_id } => {
                self.terminal.split_previous(term_id);
            }
            InternalCommand::SplitTerminalExchange { term_id } => {
                self.terminal.split_exchange(term_id);
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
            InternalCommand::SetColorTheme { name, save } => {
                if save {
                    // The config file is watched
                    LapceConfig::update_file(
                        "core",
                        "color-theme",
                        toml_edit::Value::from(name),
                    );
                } else {
                    let mut new_config = self.common.config.get_untracked();
                    Arc::make_mut(&mut new_config)
                        .set_color_theme(&self.workspace, &name);
                    self.set_config.set(new_config);
                }
            }
            InternalCommand::SetIconTheme { name, save } => {
                if save {
                    // The config file is watched
                    LapceConfig::update_file(
                        "core",
                        "icon-theme",
                        toml_edit::Value::from(name),
                    );
                } else {
                    let mut new_config = self.common.config.get_untracked();
                    Arc::make_mut(&mut new_config)
                        .set_icon_theme(&self.workspace, &name);
                    self.set_config.set(new_config);
                }
            }
            InternalCommand::SetModal { modal } => {
                LapceConfig::update_file(
                    "core",
                    "modal",
                    toml_edit::Value::from(modal),
                );
            }
            InternalCommand::OpenWebUri { uri } => {
                if !uri.is_empty() {
                    match open::that(&uri) {
                        Ok(_) => {
                            debug!("opened web uri: {uri:?}");
                        }
                        Err(e) => {
                            error!("failed to open web uri: {uri:?}, error: {e}");
                        }
                    }
                }
            }
            InternalCommand::ShowAlert {
                title,
                msg,
                buttons,
            } => {
                self.show_alert(title, msg, buttons);
            }
            InternalCommand::HideAlert => {
                self.alert_data.active.set(false);
            }
            InternalCommand::SaveScratchDoc { doc } => {
                self.main_split.save_scratch_doc(doc);
            }
            InternalCommand::UpdateProxyStatus { status } => {
                self.common.proxy_status.set(Some(status));
            }
            InternalCommand::DapFrameScopes { dap_id, frame_id } => {
                self.terminal.dap_frame_scopes(dap_id, frame_id);
            }
            InternalCommand::OpenVoltView { volt_id } => {
                self.main_split.open_volt_view(volt_id);
            }
            InternalCommand::ResetBlinkCursor => {
                if self.common.window_common.hide_cursor.get_untracked() {
                    self.common.window_common.hide_cursor.set(false);
                }
                self.common
                    .window_common
                    .cursor_blink_timer
                    .set(TimerToken::INVALID);

                let should_blink = {
                    let focus = self.common.focus;
                    let keyboard_focus = self.common.keyboard_focus;
                    move || {
                        let focus = focus.get_untracked();
                        if matches!(
                            focus,
                            Focus::Workbench
                                | Focus::Palette
                                | Focus::Panel(PanelKind::Plugin)
                                | Focus::Panel(PanelKind::Search)
                                | Focus::Panel(PanelKind::SourceControl)
                        ) {
                            return true;
                        }
                        if keyboard_focus.get_untracked().is_some() {
                            return true;
                        }
                        false
                    }
                };

                blink_cursor(
                    self.common.window_common.cursor_blink_timer,
                    self.common.window_common.hide_cursor,
                    should_blink,
                    self.common.config,
                );
            }
            InternalCommand::OpenDiffFiles {
                left_path,
                right_path,
            } => self.main_split.open_diff_files(left_path, right_path),
        }
    }

    fn handle_core_notification(&self, rpc: &CoreNotification) {
        let cx = self.scope;
        match rpc {
            CoreNotification::ProxyStatus { status } => {
                self.common.proxy_status.set(Some(status.to_owned()));
            }
            CoreNotification::DiffInfo { diff } => {
                self.source_control.branch.set(diff.head.clone());
                self.source_control
                    .branches
                    .set(diff.branches.iter().cloned().collect());
                self.source_control
                    .tags
                    .set(diff.tags.iter().cloned().collect());
                self.source_control.file_diffs.update(|file_diffs| {
                    *file_diffs = diff
                        .diffs
                        .iter()
                        .cloned()
                        .map(|diff| {
                            let checked = file_diffs
                                .get(diff.path())
                                .map_or(true, |(_, c)| *c);
                            (diff.path().clone(), (diff, checked))
                        })
                        .collect();
                });

                let docs = self.main_split.docs.get_untracked();
                for (_, doc) in docs {
                    doc.retrieve_head();
                }
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

                let completion = self.common.completion.get_untracked();
                let editor_data = completion.latest_editor_id.and_then(|id| {
                    self.main_split
                        .editors
                        .with_untracked(|tabs| tabs.get(&id).cloned())
                });
                if let Some(editor_data) = editor_data {
                    let cursor_offset =
                        editor_data.cursor.with_untracked(|c| c.offset());
                    completion.update_document_completion(
                        &editor_data.view,
                        cursor_offset,
                    );
                }
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
                    doc.init_diagnostics();
                }
            }
            CoreNotification::TerminalProcessStopped { term_id } => {
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
            CoreNotification::TerminalLaunchFailed { term_id, error } => {
                self.terminal.launch_failed(term_id, error);
            }
            CoreNotification::RunInTerminal { config } => {
                self.run_in_terminal(cx, &RunDebugMode::Debug, config, true);
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
                variables,
            } => {
                self.terminal
                    .dap_stopped(dap_id, stopped, stack_frames, variables);
            }
            CoreNotification::OpenPaths { paths } => {
                self.open_paths(paths);
            }
            CoreNotification::DapContinued { dap_id } => {
                self.terminal.dap_continued(dap_id);
            }
            CoreNotification::DapBreakpointsResp {
                path, breakpoints, ..
            } => {
                self.terminal.debug.breakpoints.update(|all_breakpoints| {
                    if let Some(current_breakpoints) = all_breakpoints.get_mut(path)
                    {
                        let mut line_changed = HashSet::new();
                        let mut i = 0;
                        for (_, current_breakpoint) in current_breakpoints.iter_mut()
                        {
                            if !current_breakpoint.active {
                                continue;
                            }
                            if let Some(breakpoint) = breakpoints.get(i) {
                                current_breakpoint.id = breakpoint.id;
                                current_breakpoint.verified = breakpoint.verified;
                                current_breakpoint.message =
                                    breakpoint.message.clone();
                                if let Some(new_line) = breakpoint.line {
                                    if current_breakpoint.line + 1 != new_line {
                                        line_changed.insert(current_breakpoint.line);
                                        current_breakpoint.line =
                                            new_line.saturating_sub(1);
                                    }
                                }
                            }
                            i += 1;
                        }
                        for line in line_changed {
                            if let Some(changed) = current_breakpoints.remove(&line)
                            {
                                current_breakpoints.insert(changed.line, changed);
                            }
                        }
                    }
                });
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
            CoreNotification::WorkDoneProgress { progress } => {
                self.update_progress(progress);
            }
            CoreNotification::ShowMessage { title, message } => {
                self.show_message(title, message);
            }
            CoreNotification::Log { level, message } => {
                match level.as_str() {
                    "TRACE" => {
                        tracing::trace!(message);
                    }
                    "DEBUG" => {
                        tracing::debug!(message);
                    }
                    "INFO" => {
                        tracing::info!(message);
                    }
                    "WARN" => {
                        tracing::warn!(message);
                    }
                    "ERROR" => {
                        tracing::error!(message);
                    }
                    _ => {
                        tracing::debug!(message);
                    }
                };
            }
            CoreNotification::WorkspaceFileChange => {
                self.file_explorer.reload();
            }
            _ => {}
        }
    }

    pub fn key_down<'a>(&self, event: impl Into<EventRef<'a>> + Copy) -> bool {
        if self.alert_data.active.get_untracked() {
            return false;
        }
        let focus = self.common.focus.get_untracked();
        let keypress = self.common.keypress.get_untracked();
        let executed = match focus {
            Focus::Workbench => {
                self.main_split.key_down(event, &keypress) == Some(true)
            }
            Focus::Palette => keypress.key_down(event, &self.palette),
            Focus::CodeAction => {
                let code_action = self.code_action.get_untracked();
                keypress.key_down(event, &code_action)
            }
            Focus::Rename => keypress.key_down(event, &self.rename),
            Focus::AboutPopup => keypress.key_down(event, &self.about_data),
            Focus::Panel(PanelKind::Terminal) => {
                self.terminal.key_down(event, &keypress)
            }
            Focus::Panel(PanelKind::Search) => {
                keypress.key_down(event, &self.global_search)
            }
            Focus::Panel(PanelKind::Plugin) => {
                keypress.key_down(event, &self.plugin)
            }
            Focus::Panel(PanelKind::SourceControl) => {
                keypress.key_down(event, &self.source_control)
            }
            _ => false,
        };

        if executed {
            return true;
        }

        keypress.key_down(event, self)
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
            panel: self.panel.panel_info(),
            breakpoints: self
                .terminal
                .debug
                .breakpoints
                .get_untracked()
                .into_iter()
                .map(|(path, breakpoints)| {
                    (path, breakpoints.into_values().collect::<Vec<_>>())
                })
                .collect(),
        }
    }

    pub fn hover_origin(&self) -> Option<Point> {
        if !self.common.hover.active.get_untracked() {
            return None;
        }

        let editor_id = self.common.hover.editor_id.get_untracked();
        let editor = self
            .main_split
            .editors
            .with(|editors| editors.get(&editor_id).cloned())?;

        let (window_origin, viewport, view) =
            (editor.window_origin, editor.viewport, editor.view.clone());

        // TODO(minor): affinity should be gotten from where the hover was started at.
        let (point_above, point_below) = view.points_of_offset(
            self.common.hover.offset.get_untracked(),
            CursorAffinity::Forward,
        );

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
        let viewport = viewport.get();
        let hover_size = self.common.hover.layout_rect.get().size();
        let tab_size = self.layout_rect.get().size();

        let mut origin = window_origin
            + Vec2::new(
                point_below.x - viewport.x0,
                (point_above.y - viewport.y0) - hover_size.height,
            );
        if origin.y < 0.0 {
            origin.y = window_origin.y + point_below.y - viewport.y0;
        }
        if origin.x + hover_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - hover_size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        Some(origin)
    }

    pub fn completion_origin(&self) -> Point {
        let completion = self.common.completion.get();
        if completion.status == CompletionStatus::Inactive {
            return Point::ZERO;
        }
        let config = self.common.config.get();
        let editor =
            if let Some(editor) = self.main_split.active_editor.get_untracked() {
                editor
            } else {
                return Point::ZERO;
            };

        let (window_origin, viewport, view) =
            (editor.window_origin, editor.viewport, editor.view.clone());

        // TODO(minor): What affinity should we use for this? Probably just use the cursor's
        // original affinity..
        let (point_above, point_below) =
            view.points_of_offset(completion.offset, CursorAffinity::Forward);

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
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
            (editor.window_origin, editor.viewport, editor.view.clone());

        // TODO(minor): What affinity should we use for this?
        let (_point_above, point_below) =
            view.points_of_offset(code_action.offset, CursorAffinity::Forward);

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
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
            (editor.window_origin, editor.viewport, editor.view.clone());

        // TODO(minor): What affinity should we use for this?
        let (_point_above, point_below) = view.points_of_offset(
            self.rename.start.get_untracked(),
            CursorAffinity::Forward,
        );

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
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
                self.main_split
                    .active_editor
                    .get()
                    .map(|editor| editor.cursor.with(|c| c.get_mode()))
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

    /// Toggle a specific kind of panel.
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

    /// Toggle a panel on one of the sides.
    fn toggle_container_visual(&self, position: &PanelContainerPosition) {
        let shown = !self.panel.is_container_shown(position, false);
        self.panel.set_shown(&position.first(), shown);
        self.panel.set_shown(&position.second(), shown);

        if shown {
            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.second(), false)
            {
                self.show_panel(kind);
            }

            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.first(), false)
            {
                self.show_panel(kind);
            }
        } else {
            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.second(), false)
            {
                self.hide_panel(kind);
            }

            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.first(), false)
            {
                self.hide_panel(kind);
            }
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
            self.terminal.new_tab(None, None);
        }
        self.panel.show_panel(&kind);
        if kind == PanelKind::Search
            && self.common.focus.get_untracked() == Focus::Workbench
        {
            let active_editor = self.main_split.active_editor.get_untracked();
            let word = active_editor.map(|editor| editor.word_at_cursor());
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
                self.run_in_terminal(cx, mode, config, false);
            }
            RunDebugMode::Debug => {
                if config.prelaunch.is_some() {
                    self.run_in_terminal(cx, mode, config, false);
                } else {
                    self.common.proxy.dap_start(
                        config.clone(),
                        self.terminal.debug.source_breakpoints(),
                    )
                };
            }
        }
    }

    fn run_in_terminal(
        &self,
        cx: Scope,
        mode: &RunDebugMode,
        config: &RunDebugConfig,
        from_dap: bool,
    ) {
        // if not from dap, then run prelaunch first
        let is_prelaunch = !from_dap;
        let term_id = if let Some(terminal) =
            self.terminal.get_stopped_run_debug_terminal(mode, config)
        {
            terminal.new_process(Some(RunDebugProcess {
                mode: *mode,
                config: config.clone(),
                stopped: false,
                created: Instant::now(),
                is_prelaunch,
            }));

            terminal.term_id
        } else {
            let new_terminal_tab = self.terminal.new_tab(
                Some(RunDebugProcess {
                    mode: *mode,
                    config: config.clone(),
                    stopped: false,
                    created: Instant::now(),
                    is_prelaunch,
                }),
                None,
            );
            new_terminal_tab.active_terminal(false).unwrap().term_id
        };
        self.common.focus.set(Focus::Panel(PanelKind::Terminal));
        self.terminal.focus_terminal(term_id);

        self.terminal.debug.active_term.set(Some(term_id));
        self.terminal.debug.daps.update(|daps| {
            daps.insert(
                config.dap_id,
                DapData::new(cx, config.dap_id, term_id, self.common.clone()),
            );
        });

        if !self.panel.is_panel_visible(&PanelKind::Terminal) {
            self.panel.show_panel(&PanelKind::Terminal);
        }
    }

    pub fn open_paths(&self, paths: &[PathObject]) {
        let (folders, files): (Vec<&PathObject>, Vec<&PathObject>) =
            paths.iter().partition(|p| p.is_dir);

        for folder in folders {
            self.common.window_common.window_command.send(
                WindowCommand::NewWorkspaceTab {
                    workspace: LapceWorkspace {
                        kind: self.workspace.kind.clone(),
                        path: Some(folder.path.clone()),
                        last_open: 0,
                    },
                    end: false,
                },
            );
        }

        for file in files {
            let position = file.linecol.map(|pos| {
                EditorPosition::Position(lsp_types::Position {
                    line: pos.line.saturating_sub(1) as u32,
                    character: pos.column.saturating_sub(1) as u32,
                })
            });

            self.common
                .internal_command
                .send(InternalCommand::GoToLocation {
                    location: EditorLocation {
                        path: file.path.clone(),
                        position,
                        scroll_offset: None,
                        // Create a new editor for the file, so we don't change any current unconfirmed
                        // editor
                        ignore_unconfirmed: true,
                        same_editor_tab: false,
                    },
                });
        }
    }

    pub fn show_alert(&self, title: String, msg: String, buttons: Vec<AlertButton>) {
        self.alert_data.title.set(title);
        self.alert_data.msg.set(msg);
        self.alert_data.buttons.set(buttons);
        self.alert_data.active.set(true);
    }

    fn update_progress(&self, progress: &ProgressParams) {
        let token = progress.token.clone();
        match &progress.value {
            lsp_types::ProgressParamsValue::WorkDone(progress) => match progress {
                lsp_types::WorkDoneProgress::Begin(progress) => {
                    let progress = WorkProgress {
                        token: token.clone(),
                        title: progress.title.clone(),
                        message: progress.message.clone(),
                        percentage: progress.percentage,
                    };
                    self.progresses.update(|p| {
                        p.insert(token, progress);
                    });
                }
                lsp_types::WorkDoneProgress::Report(report) => {
                    self.progresses.update(|p| {
                        if let Some(progress) = p.get_mut(&token) {
                            progress.message = report.message.clone();
                            progress.percentage = report.percentage;
                        }
                    })
                }
                lsp_types::WorkDoneProgress::End(_) => {
                    self.progresses.update(|p| {
                        p.remove(&token);
                    });
                }
            },
        }
    }

    fn show_message(&self, title: &str, message: &ShowMessageParams) {
        self.messages.update(|messages| {
            messages.push((title.to_string(), message.clone()));
        });
    }
}

fn blink_cursor(
    cursor_blink_timer: RwSignal<TimerToken>,
    hide_cursor: RwSignal<bool>,
    should_blink: impl Fn() -> bool + 'static + Copy,
    config: ReadSignal<Arc<LapceConfig>>,
) {
    let blink_interval =
        config.with_untracked(|config| config.editor.blink_interval());
    if blink_interval > 0 && should_blink() {
        let timer_token =
            exec_after(Duration::from_millis(blink_interval), move |timer_token| {
                if cursor_blink_timer.try_get_untracked() == Some(timer_token) {
                    hide_cursor.update(|hide| {
                        *hide = !*hide;
                    });
                    blink_cursor(
                        cursor_blink_timer,
                        hide_cursor,
                        should_blink,
                        config,
                    );
                }
            });
        cursor_blink_timer.set(timer_token);
    }
}

/// Open path with the default application without blocking.
fn open_uri(path: &Path) {
    match open::that(path) {
        Ok(_) => {
            debug!("opened active file: {path:?}");
        }
        Err(e) => {
            error!("failed to open active file: {path:?}, error: {e}");
        }
    }
}
