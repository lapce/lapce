use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::{HashMap, HashSet},
    env,
    fmt::Display,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    thread,
    time::Instant,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use druid::{
    piet::PietText, theme, Command, Data, Env, EventCtx, ExtEventSink,
    FileDialogOptions, Lens, Point, Rect, Size, Target, Vec2, WidgetId, WindowId,
};
use im::Vector;
use itertools::Itertools;
use lapce_core::{
    command::{FocusCommand, MultiSelectionCommand},
    cursor::{Cursor, CursorMode},
    directory::Directory,
    editor::EditType,
    meta,
    mode::{Mode, MotionMode},
    movement::Movement,
    register::Register,
    selection::Selection,
};
use lapce_rpc::{
    buffer::BufferId,
    core::{CoreMessage, CoreNotification},
    plugin::{VoltID, VoltInfo},
    proxy::ProxyResponse,
    source_control::FileDiff,
    terminal::TermId,
    RpcMessage,
};
use lapce_xi_rope::{Rope, RopeDelta};
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, ProgressToken, TextEdit};
use notify::Watcher;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    about::AboutData,
    alert::{AlertContentData, AlertData},
    command::{
        CommandKind, EnsureVisiblePosition, InitBufferContentCb, LapceCommand,
        LapceUICommand, LapceWorkbenchCommand, LAPCE_COMMAND, LAPCE_OPEN_FILE,
        LAPCE_OPEN_FOLDER, LAPCE_UI_COMMAND,
    },
    completion::CompletionData,
    config::{ConfigWatcher, GetConfig, LapceConfig, LapceTheme},
    db::{
        EditorInfo, EditorTabChildInfo, EditorTabInfo, LapceDb, SplitContentInfo,
        SplitInfo, TabsInfo, WindowInfo, WorkspaceInfo,
    },
    document::{BufferContent, Document, LocalBufferKind},
    editor::{EditorLocation, EditorPosition, LapceEditorBufferData, Line, TabRect},
    explorer::FileExplorerData,
    find::Find,
    hover::HoverData,
    images::ImageCache,
    keypress::KeyPressData,
    palette::{PaletteData, PaletteType, PaletteViewData},
    panel::{
        PanelContainerPosition, PanelData, PanelKind, PanelOrder, PanelPosition,
    },
    picker::FilePickerData,
    plugin::PluginData,
    problem::ProblemData,
    proxy::{LapceProxy, ProxyStatus, TermEvent},
    rename::RenameData,
    search::SearchData,
    settings::LapceSettingsPanelData,
    signature::SignatureData,
    source_control::SourceControlData,
    split::{SplitDirection, SplitMoveDirection},
    terminal::TerminalPanelData,
    title::TitleData,
    update::ReleaseInfo,
};

/// `LapceData` is the topmost structure in a tree of structures that holds
/// the application model for Lapce.
///
/// Druid requires that application models implement the
/// [Data trait](https://linebender.org/druid/data.html).
#[derive(Clone, Data)]
pub struct LapceData {
    /// The set of top-level windows in Lapce. Normally there is only one;
    /// a new window can be created using the "New Window" command.
    pub windows: im::HashMap<WindowId, LapceWindowData>,
    /// How key presses are to be processed.
    pub keypress: Arc<KeyPressData>,
    /// The persistent state of the program, such as recent workspaces.
    pub db: Arc<LapceDb>,
    /// The order of panels in each postion
    pub panel_orders: PanelOrder,
    /// The latest release information
    pub latest_release: Arc<Option<ReleaseInfo>>,
    /// whether the update is in progress
    pub update_in_process: bool,
    /// log file path
    #[data(ignore)]
    pub log_file: Arc<Option<PathBuf>>,
    /// The window on focus
    pub active_window: Arc<WindowId>,
}

impl LapceData {
    /// Create a new `LapceData` struct by loading configuration, and state
    /// previously written to the Lapce database.
    pub fn load(
        event_sink: ExtEventSink,
        paths: Vec<PathBuf>,
        log_file: Option<PathBuf>,
    ) -> Self {
        let _ = lapce_proxy::register_lapce_path();

        let log_file = Arc::new(log_file);
        let db = Arc::new(LapceDb::new().unwrap());
        let mut windows = im::HashMap::new();
        let config = LapceConfig::load(&LapceWorkspace::default(), &[]);
        let keypress = Arc::new(KeyPressData::new(&config, event_sink.clone()));
        let panel_orders = db
            .get_panel_orders()
            .unwrap_or_else(|_| Self::default_panel_orders());
        let latest_release = Arc::new(None);

        let dirs: Vec<&PathBuf> = paths.iter().filter(|p| p.is_dir()).collect();
        let files: Vec<&PathBuf> = paths.iter().filter(|p| p.is_file()).collect();
        if !dirs.is_empty() {
            let (size, mut pos) = db
                .get_last_window_info()
                .map(|i| (i.size, i.pos))
                .unwrap_or_else(|_| (Size::new(800.0, 600.0), Point::new(0.0, 0.0)));
            for dir in dirs {
                #[cfg(windows)]
                let workspace_type =
                    if !env::var("WSL_DISTRO_NAME").unwrap_or_default().is_empty()
                        || !env::var("WSL_INTEROP").unwrap_or_default().is_empty()
                    {
                        LapceWorkspaceType::RemoteWSL
                    } else {
                        LapceWorkspaceType::Local
                    };

                #[cfg(not(windows))]
                let workspace_type = LapceWorkspaceType::Local;

                let info = WindowInfo {
                    size,
                    pos,
                    maximised: false,
                    tabs: TabsInfo {
                        active_tab: 0,
                        workspaces: vec![LapceWorkspace {
                            kind: workspace_type,
                            path: Some(dir.to_path_buf()),
                            last_open: 0,
                        }],
                    },
                };
                pos += (50.0, 50.0);
                let window = LapceWindowData::new(
                    keypress.clone(),
                    latest_release.clone(),
                    false,
                    log_file.clone(),
                    None,
                    panel_orders.clone(),
                    event_sink.clone(),
                    &info,
                    db.clone(),
                );
                windows.insert(window.window_id, window);
            }
        } else if files.is_empty() {
            if let Ok(app) = db.get_app() {
                for info in app.windows.iter() {
                    let window = LapceWindowData::new(
                        keypress.clone(),
                        latest_release.clone(),
                        false,
                        log_file.clone(),
                        None,
                        panel_orders.clone(),
                        event_sink.clone(),
                        info,
                        db.clone(),
                    );
                    windows.insert(window.window_id, window);
                }
            }
        }

        if windows.is_empty() {
            let (size, pos) = db
                .get_last_window_info()
                .map(|i| (i.size, i.pos))
                .unwrap_or_else(|_| (Size::new(800.0, 600.0), Point::new(0.0, 0.0)));
            let info = WindowInfo {
                size,
                pos,
                maximised: false,
                tabs: TabsInfo {
                    active_tab: 0,
                    workspaces: vec![],
                },
            };
            let window = LapceWindowData::new(
                keypress.clone(),
                latest_release.clone(),
                false,
                log_file.clone(),
                None,
                panel_orders.clone(),
                event_sink.clone(),
                &info,
                db.clone(),
            );
            windows.insert(window.window_id, window);
        }

        if let Some((window_id, _)) = windows.iter().next() {
            for file in files {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::OpenFile(file.to_path_buf(), false),
                    Target::Window(*window_id),
                );
            }
        }

        #[cfg(feature = "updater")]
        {
            let local_event_sink = event_sink.clone();
            std::thread::spawn(move || loop {
                if let Ok(release) = crate::update::get_latest_release() {
                    let _ = local_event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateLatestRelease(release),
                        Target::Global,
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(60 * 60));
            });
        }

        std::thread::spawn(move || {
            let _ = Self::listen_local_socket(event_sink);
        });

        Self {
            active_window: Arc::new(
                windows
                    .iter()
                    .next()
                    .map(|(w, _)| *w)
                    .unwrap_or_else(WindowId::next),
            ),

            windows,
            keypress,
            db,
            panel_orders,
            latest_release,
            update_in_process: false,
            log_file,
        }
    }

    pub fn default_panel_orders() -> PanelOrder {
        let mut order = PanelOrder::new();
        order.insert(
            PanelPosition::LeftTop,
            im::vector![
                PanelKind::FileExplorer,
                PanelKind::SourceControl,
                PanelKind::Plugin,
            ],
        );
        order.insert(
            PanelPosition::BottomLeft,
            im::vector![PanelKind::Terminal, PanelKind::Search, PanelKind::Problem,],
        );

        order
    }

    pub fn reload_env(&self, env: &mut Env) {
        env.set(theme::SCROLLBAR_WIDTH, 10.0);
        env.set(theme::SCROLLBAR_EDGE_WIDTH, 0.0);
        env.set(theme::SCROLLBAR_PAD, 0.0);
        env.set(theme::SCROLLBAR_MAX_OPACITY, 0.7);
        env.set(LapceTheme::PALETTE_INPUT_LINE_HEIGHT, 18.0);
        env.set(LapceTheme::PALETTE_INPUT_LINE_PADDING, 4.0);
        env.set(LapceTheme::INPUT_LINE_HEIGHT, 20.0);
        env.set(LapceTheme::INPUT_LINE_PADDING, 5.0);
        env.set(LapceTheme::INPUT_FONT_SIZE, 13u64);
    }

    fn listen_local_socket(event_sink: ExtEventSink) -> Result<()> {
        let local_socket = Directory::local_socket()
            .ok_or_else(|| anyhow!("can't get local socket folder"))?;
        let _ = std::fs::remove_file(&local_socket);
        let socket =
            interprocess::local_socket::LocalSocketListener::bind(local_socket)?;

        for stream in socket.incoming().flatten() {
            let event_sink = event_sink.clone();
            thread::spawn(move || -> Result<()> {
                let mut reader = BufReader::new(stream);
                loop {
                    let msg: CoreMessage = lapce_rpc::stdio::read_msg(&mut reader)?;

                    if let RpcMessage::Notification(CoreNotification::OpenPaths {
                        window_tab_id,
                        folders,
                        files,
                    }) = msg
                    {
                        let window_tab_id =
                            window_tab_id.map(|(window_id, tab_id)| {
                                (
                                    WindowId::from_usize(window_id),
                                    WidgetId::from_usize(tab_id),
                                )
                            });
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::OpenPaths {
                                window_tab_id,
                                folders,
                                files,
                            },
                            Target::Global,
                        );
                    } else {
                        log::trace!("Unhandled message: {msg:?}");
                    }

                    let stream_ref = reader.get_mut();
                    let _ = stream_ref.write_all(b"received");
                    let _ = stream_ref.flush();
                }
            });
        }
        Ok(())
    }

    pub fn try_open_in_existing_process(paths: &[PathBuf]) -> Result<()> {
        let local_socket = Directory::local_socket()
            .ok_or_else(|| anyhow!("can't get local socket folder"))?;
        let mut socket =
            interprocess::local_socket::LocalSocketStream::connect(local_socket)?;
        let folders: Vec<_> = paths.iter().filter(|p| p.is_dir()).cloned().collect();
        let files: Vec<_> = paths.iter().filter(|p| p.is_file()).cloned().collect();
        let msg: CoreMessage =
            RpcMessage::Notification(CoreNotification::OpenPaths {
                window_tab_id: None,
                folders,
                files,
            });
        lapce_rpc::stdio::write_msg(&mut socket, msg)?;

        let (tx, rx) = crossbeam_channel::bounded(1);
        thread::spawn(move || {
            let mut buf = [0; 100];
            let received = if let Ok(n) = socket.read(&mut buf) {
                &buf[..n] == b"received"
            } else {
                false
            };
            tx.send(received)
        });

        let received = rx.recv_timeout(std::time::Duration::from_millis(500))?;
        if !received {
            return Err(anyhow!("didn't receive response"));
        }

        Ok(())
    }
}

/// `LapceWindowData` is the application model for a top-level window.
///
/// A top-level window can be independently moved around and
/// resized using your window manager. Normally Lapce has only one
/// top-level window, but new ones can be created using the "New Window"
/// command.
///
/// Each window has its own collection of "window tabs" (again, there is
/// normally only one window tab), size, position etc. and `Arc` references to
/// state that is common to this instance of Lapce, such as configuration and the
/// keymap setup.
#[derive(Clone, Data)]
pub struct LapceWindowData {
    /// The unique identifier for the Window. Generated by Druid.
    #[data(ignore)]
    pub window_id: WindowId,
    /// The set of tabs within the window. These tabs are high-level
    /// constructs, in particular they are not **editor tabs**, which are
    /// lower down the hierarchy at [LapceEditorTabData].
    ///
    /// Normally there is only one window-level tab, and it is not visible
    /// on screen as a separate thing - only its contents are. If you
    /// create a new tab using the "Create New Tab" command then both
    /// tabs will appear in the user interface at the top of the window.
    pub tabs: im::HashMap<WidgetId, LapceTabData>,
    /// The order of the window tabs in the user interface.
    pub tabs_order: Arc<Vec<WidgetId>>,
    /// The index of the active window tab.
    pub active: usize,
    /// The Id of the active window tab.
    pub active_id: Arc<WidgetId>,
    pub keypress: Arc<KeyPressData>,
    pub config: Arc<LapceConfig>,
    pub db: Arc<LapceDb>,
    pub watcher: Arc<notify::RecommendedWatcher>,
    /// The size of the window.
    pub size: Size,
    pub maximised: bool,
    /// The position of the window.
    pub pos: Point,
    pub panel_orders: PanelOrder,
    pub latest_release: Arc<Option<ReleaseInfo>>,
    pub update_in_progress: bool,
    #[data(ignore)]
    pub log_file: Arc<Option<PathBuf>>,
}

impl LapceWindowData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        keypress: Arc<KeyPressData>,
        latest_release: Arc<Option<ReleaseInfo>>,
        update_in_progress: bool,
        log_file: Arc<Option<PathBuf>>,
        current_panels: Option<PanelData>,
        panel_orders: PanelOrder,
        event_sink: ExtEventSink,
        info: &WindowInfo,
        db: Arc<LapceDb>,
    ) -> Self {
        let mut tabs = im::HashMap::new();
        let mut tabs_order = Vec::new();
        let mut active_tab_id = WidgetId::next();
        let mut active = 0;

        let window_id = WindowId::next();
        for (i, workspace) in info.tabs.workspaces.iter().enumerate() {
            let tab_id = WidgetId::next();
            let tab = LapceTabData::new(
                window_id,
                tab_id,
                workspace.clone(),
                db.clone(),
                keypress.clone(),
                latest_release.clone(),
                update_in_progress,
                log_file.clone(),
                current_panels.clone(),
                panel_orders.clone(),
                event_sink.clone(),
            );
            tabs.insert(tab_id, tab);
            tabs_order.push(tab_id);
            if i == info.tabs.active_tab {
                active_tab_id = tab_id;
                active = i;
            }
        }

        if tabs.is_empty() {
            let tab_id = WidgetId::next();
            let tab = LapceTabData::new(
                window_id,
                tab_id,
                LapceWorkspace::default(),
                db.clone(),
                keypress.clone(),
                latest_release.clone(),
                update_in_progress,
                log_file.clone(),
                current_panels,
                panel_orders.clone(),
                event_sink.clone(),
            );
            tabs.insert(tab_id, tab);
            tabs_order.push(tab_id);
            active_tab_id = tab_id;
        }

        let config = Arc::new(LapceConfig::load(
            &LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: None,
                last_open: 0,
            },
            &[],
        ));
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(active_tab_id),
        );

        let mut watcher =
            notify::recommended_watcher(ConfigWatcher::new(event_sink)).unwrap();
        if let Some(path) = LapceConfig::settings_file() {
            let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
        }
        if let Some(path) = Directory::themes_directory() {
            let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
        }
        if let Some(path) = LapceConfig::keymaps_file() {
            let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
        }
        if let Some(path) = Directory::plugins_directory() {
            let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
        }

        Self {
            window_id,
            tabs,
            tabs_order: Arc::new(tabs_order),
            active,
            active_id: Arc::new(active_tab_id),
            keypress,
            config,
            db,
            watcher: Arc::new(watcher),
            size: info.size,
            pos: info.pos,
            maximised: info.maximised,
            panel_orders,
            latest_release,
            update_in_progress,
            log_file,
        }
    }

    pub fn info(&self) -> WindowInfo {
        let mut active_tab = 0;
        let workspaces: Vec<LapceWorkspace> = self
            .tabs_order
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let tab = self.tabs.get(w).unwrap();
                if tab.id == *self.active_id {
                    active_tab = i;
                }
                (*tab.workspace).clone()
            })
            .collect();
        WindowInfo {
            size: self.size,
            pos: self.pos,
            maximised: self.maximised,
            tabs: TabsInfo {
                active_tab,
                workspaces,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct EditorDiagnostic {
    pub range: (usize, usize),
    pub diagnostic: Diagnostic,
    /// Line counter for the editor diagnostic.
    /// Contains the total number of message lines and related information lines
    pub lines: usize,
}

#[derive(Clone)]
pub struct WorkProgress {
    pub token: ProgressToken,
    pub title: String,
    pub message: Option<String>,
    pub percentage: Option<u32>,
}

#[derive(Clone, PartialEq, Eq, Data)]
pub enum FocusArea {
    Palette,
    Editor,
    Rename,
    Panel(PanelKind),
    FilePicker,
    BranchPicker,
}

#[derive(Clone)]
pub enum DragContent {
    EditorTab(WidgetId, usize, EditorTabChild, Box<TabRect>),
    Panel(PanelKind, Rect),
}

#[derive(Clone, Lens, Data)]
pub struct LapceTabData {
    #[data(ignore)]
    pub id: WidgetId,
    pub window_id: Arc<WindowId>,
    pub multiple_tab: bool,
    pub workspace: Arc<LapceWorkspace>,
    pub title: Arc<TitleData>,
    pub main_split: LapceMainSplitData,
    pub completion: Arc<CompletionData>,
    pub signature: Arc<SignatureData>,
    pub hover: Arc<HoverData>,
    pub rename: Arc<RenameData>,
    pub terminal: Arc<TerminalPanelData>,
    pub palette: Arc<PaletteData>,
    pub find: Arc<Find>,
    pub source_control: Arc<SourceControlData>,
    pub problem: Arc<ProblemData>,
    pub search: Arc<SearchData>,
    pub plugin: Arc<PluginData>,
    pub picker: Arc<FilePickerData>,
    pub file_explorer: Arc<FileExplorerData>,
    #[data(ignore)]
    pub proxy: Arc<LapceProxy>,
    pub proxy_status: Arc<ProxyStatus>,
    pub keypress: Arc<KeyPressData>,
    pub settings: Arc<LapceSettingsPanelData>,
    pub about: Arc<AboutData>,
    pub alert: Arc<AlertData>,
    pub message_widget_id: Arc<WidgetId>,
    #[data(ignore)]
    pub term_tx: Arc<Sender<(TermId, TermEvent)>>,
    #[data(ignore)]
    pub term_rx: Option<Receiver<(TermId, TermEvent)>>,
    #[data(ignore)]
    pub window_origin: Rc<RefCell<Point>>,
    pub panel: Arc<PanelData>,
    pub config: Arc<LapceConfig>,
    pub images: Arc<ImageCache>,
    pub focus: Arc<WidgetId>,
    pub focus_area: FocusArea,
    #[data(ignore)]
    pub db: Arc<LapceDb>,
    pub progresses: Arc<Vec<WorkProgress>>,
    pub drag: Arc<Option<(Vec2, Vec2, DragContent)>>,
    pub latest_release: Arc<Option<ReleaseInfo>>,
    pub update_in_progress: bool,
    #[data(ignore)]
    pub log_file: Arc<Option<PathBuf>>,
}

impl GetConfig for LapceTabData {
    fn get_config(&self) -> &LapceConfig {
        &self.config
    }
}

impl LapceTabData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        workspace: LapceWorkspace,
        db: Arc<LapceDb>,
        keypress: Arc<KeyPressData>,
        latest_release: Arc<Option<ReleaseInfo>>,
        update_in_progress: bool,
        log_file: Arc<Option<PathBuf>>,
        current_panels: Option<PanelData>,
        panel_orders: PanelOrder,
        event_sink: ExtEventSink,
    ) -> Self {
        let disabled_volts = db.get_disabled_volts().unwrap_or_default();
        let workspace_disabled_volts = db
            .get_workspace_disabled_volts(&workspace)
            .unwrap_or_default();
        let mut all_disabled_volts = disabled_volts.clone();
        all_disabled_volts.extend_from_slice(&workspace_disabled_volts);

        let config = Arc::new(LapceConfig::load(&workspace, &all_disabled_volts));
        let workspace_info = if workspace.path.is_some() {
            db.get_workspace_info(&workspace).ok()
        } else {
            let mut info = db.get_workspace_info(&workspace).ok();
            if let Some(info) = info.as_mut() {
                info.split.children.clear();
                if let Some(panels) = current_panels.clone() {
                    info.panel = panels;
                }
            }
            info
        };

        let (term_sender, term_receiver) = unbounded();
        let proxy = Arc::new(LapceProxy::new(
            window_id,
            tab_id,
            workspace.clone(),
            all_disabled_volts,
            config.plugins.clone(),
            term_sender.clone(),
            event_sink.clone(),
        ));
        let title = Arc::new(TitleData::new(config.clone()));
        let palette = Arc::new(PaletteData::new(config.clone(), proxy.clone()));
        let completion = Arc::new(CompletionData::new(config.clone()));
        let hover = Arc::new(HoverData::new());
        let signature = Arc::new(SignatureData::new());
        let rename = Arc::new(RenameData::new());
        let source_control = Arc::new(SourceControlData::new());
        let settings = Arc::new(LapceSettingsPanelData::new());
        let about = Arc::new(AboutData::new());
        let alert = Arc::new(AlertData::new());
        let plugin = Arc::new(PluginData::new(
            tab_id,
            disabled_volts,
            workspace_disabled_volts,
            event_sink.clone(),
        ));
        let file_explorer = Arc::new(FileExplorerData::new(
            tab_id,
            workspace.clone(),
            proxy.clone(),
            event_sink.clone(),
        ));
        let search = Arc::new(SearchData::new());
        let file_picker = Arc::new(FilePickerData::new());

        let unsaved_buffers = match db.get_unsaved_buffers() {
            Ok(val) => val,
            Err(err) => {
                log::warn!("Error during unsaved buffer fetching : {:}", err);
                im::HashMap::new()
            }
        };

        let mut main_split = LapceMainSplitData::new(
            tab_id,
            workspace_info.as_ref(),
            palette.preview_editor,
            proxy.clone(),
            &config,
            event_sink.clone(),
            Arc::new(workspace.clone()),
            db.clone(),
            unsaved_buffers,
        );

        main_split.add_editor(
            source_control.editor_view_id,
            None,
            LocalBufferKind::SourceControl,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            settings.keymap_view_id,
            None,
            LocalBufferKind::Keymap,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            settings.settings_view_id,
            None,
            LocalBufferKind::Settings,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            search.editor_view_id,
            None,
            LocalBufferKind::Search,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            palette.input_editor,
            None,
            LocalBufferKind::Palette,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            title.branches.filter_editor,
            None,
            LocalBufferKind::BranchesFilter,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            plugin.search_editor,
            None,
            LocalBufferKind::PluginSearch,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            rename.view_id,
            None,
            LocalBufferKind::Rename,
            &config,
            event_sink.clone(),
        );
        main_split.add_editor(
            file_picker.editor_view_id,
            None,
            LocalBufferKind::FilePicker,
            &config,
            event_sink.clone(),
        );

        let terminal = Arc::new(TerminalPanelData::new(
            Arc::new(workspace.clone()),
            proxy.clone(),
            &config,
            event_sink.clone(),
        ));
        let problem = Arc::new(ProblemData::new());
        let panel = workspace_info
            .map(|i| {
                let mut panel = i.panel;
                panel.order = panel_orders.clone();
                panel
            })
            .or(current_panels)
            .unwrap_or_else(|| PanelData::new(panel_orders));

        let focus = (*main_split.active).unwrap_or(*main_split.split_id);

        let mut tab = Self {
            id: tab_id,
            multiple_tab: false,
            window_id: Arc::new(window_id),
            workspace: Arc::new(workspace),
            focus: Arc::new(focus),
            title,
            main_split,
            completion,
            signature,
            hover,
            rename,
            terminal,
            plugin,
            problem,
            search,
            find: Arc::new(Find::new(0)),
            picker: file_picker,
            source_control,
            file_explorer,
            term_rx: Some(term_receiver),
            term_tx: Arc::new(term_sender),
            palette,
            proxy,
            settings,
            about,
            alert,
            message_widget_id: Arc::new(WidgetId::next()),
            proxy_status: Arc::new(ProxyStatus::Connecting),
            keypress,
            window_origin: Rc::new(RefCell::new(Point::ZERO)),
            panel: Arc::new(panel),
            config,
            images: Arc::new(ImageCache::default()),
            focus_area: FocusArea::Editor,
            db,
            progresses: Arc::new(Vec::new()),
            drag: Arc::new(None),
            latest_release,
            update_in_progress,
            log_file,
        };
        tab.start_update_process(event_sink);
        tab
    }

    pub fn workspace_info(&self) -> WorkspaceInfo {
        let main_split_data = self
            .main_split
            .splits
            .get(&self.main_split.split_id)
            .unwrap();
        WorkspaceInfo {
            split: main_split_data.split_info(self),
            panel: (*self.panel).clone(),
        }
    }

    pub fn start_update_process(&mut self, event_sink: ExtEventSink) {
        if let Some(receiver) = self.term_rx.take() {
            let tab_id = self.id;
            let local_event_sink = event_sink.clone();
            let proxy = self.proxy.clone();
            let workspace = self.workspace.clone();
            let palette_widget_id = self.palette.widget_id;
            thread::spawn(move || {
                LapceTabData::terminal_update_process(
                    tab_id,
                    palette_widget_id,
                    receiver,
                    local_event_sink,
                    workspace,
                    proxy,
                );
            });
        }

        if let Some(receiver) = Arc::make_mut(&mut self.palette).receiver.take() {
            let widget_id = self.palette.widget_id;
            thread::spawn(move || {
                PaletteViewData::update_process(receiver, widget_id, event_sink);
            });
        }
    }

    /// Get information about the specific editor, with various data so that it can provide useful  
    /// utility functions for the editor buffer.  
    /// Note that if you edit the editor buffer or related fields, then you'll have to 'give it
    /// back' to [`LapceTabData`] so that it can update the internals.  
    /// ```rust,ignore
    /// // Get the editor before it may be modified by the `editor_data`
    /// let editor = data.main_split.editors.get(&view_id).unwrap().clone();
    /// let mut editor_data = data.editor_view_content(view_id);
    /// let doc = editor_data.doc.clone();
    /// // Do some modification
    /// Arc::make_mut(&mut editor_data.editor).cursor.set_offset(0, true, mods.alt());
    /// // Give it back so that `LapceTabData` can update the internals
    /// data.update_from_editor_buffer_data(editor_data, &editor, &doc);
    /// ```
    pub fn editor_view_content(
        &self,
        editor_view_id: WidgetId,
    ) -> LapceEditorBufferData {
        let editor = self.main_split.editors.get(&editor_view_id).unwrap();
        let doc = match &editor.content {
            BufferContent::File(path) => {
                self.main_split.open_docs.get(path).unwrap().clone()
            }
            BufferContent::Scratch(id, _) => {
                self.main_split.scratch_docs.get(id).unwrap().clone()
            }
            BufferContent::Local(kind) => {
                self.main_split.local_docs.get(kind).unwrap().clone()
            }
            BufferContent::SettingsValue(name, ..) => {
                self.main_split.value_docs.get(name).unwrap().clone()
            }
        };
        LapceEditorBufferData {
            view_id: editor_view_id,
            main_split: self.main_split.clone(),
            completion: self.completion.clone(),
            signature: self.signature.clone(),
            hover: self.hover.clone(),
            rename: self.rename.clone(),
            focus_area: self.focus_area.clone(),
            source_control: self.source_control.clone(),
            proxy: self.proxy.clone(),
            find: self.find.clone(),
            doc,
            palette: self.palette.clone(),
            editor: editor.clone(),
            command_keymaps: self.keypress.command_keymaps.clone(),
            config: self.config.clone(),
        }
    }

    pub fn code_action_size(&self, text: &mut PietText, _env: &Env) -> Size {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return Size::ZERO,
        };

        match &editor.content {
            BufferContent::Local(_) => Size::ZERO,
            BufferContent::SettingsValue(..) => Size::ZERO,
            BufferContent::File(path) => {
                let doc = self.main_split.open_docs.get(path).unwrap();
                let offset = editor.cursor.offset();
                doc.code_action_size(text, offset, &self.config)
            }
            BufferContent::Scratch(id, _) => {
                let doc = self.main_split.scratch_docs.get(id).unwrap();
                let offset = editor.cursor.offset();
                doc.code_action_size(text, offset, &self.config)
            }
        }
    }

    pub fn is_drag_editor(&self) -> bool {
        matches!(&*self.drag, Some((_, _, DragContent::EditorTab(..))))
    }

    /// Get the mode for the current editor or terminal
    pub fn mode(&self) -> Mode {
        if self.config.core.modal {
            let mode = if self.focus_area == FocusArea::Panel(PanelKind::Terminal) {
                self.terminal.active_terminal().map(|t| t.mode)
            } else {
                self.main_split.active_editor().map(|e| e.cursor.get_mode())
            };

            mode.unwrap_or(Mode::Normal)
        } else {
            Mode::Insert
        }
    }

    /// Update the stored information with the changed editor buffer data.  
    /// ```rust,ignore
    /// // Get the editor before it may be modified by the `editor_data`
    /// let editor = data.main_split.editors.get(&view_id).unwrap().clone();
    /// let mut editor_data = data.editor_view_content(view_id);
    /// let doc = editor_data.doc.clone();
    /// // Do some modification
    /// Arc::make_mut(&mut editor_data.editor).cursor.set_offset(0, true, mods.alt());
    /// // Give it back so that `LapceTabData` can update the internals
    /// data.update_from_editor_buffer_data(editor_data, &editor, &doc);
    /// ```
    pub fn update_from_editor_buffer_data(
        &mut self,
        editor_buffer_data: LapceEditorBufferData,
        editor: &Arc<LapceEditorData>,
        doc: &Arc<Document>,
    ) {
        self.completion = editor_buffer_data.completion.clone();
        self.signature = editor_buffer_data.signature.clone();
        self.hover = editor_buffer_data.hover.clone();
        self.rename = editor_buffer_data.rename.clone();
        self.main_split = editor_buffer_data.main_split.clone();
        self.find = editor_buffer_data.find.clone();
        if !editor_buffer_data.editor.same(editor) {
            self.main_split
                .editors
                .insert(editor.view_id, editor_buffer_data.editor);
        }
        if !editor_buffer_data.doc.same(doc) {
            match doc.content() {
                BufferContent::File(path) => {
                    self.main_split
                        .open_docs
                        .insert(path.clone(), editor_buffer_data.doc);
                }
                BufferContent::Scratch(id, _) => {
                    self.main_split
                        .scratch_docs
                        .insert(*id, editor_buffer_data.doc);
                }
                BufferContent::Local(kind) => {
                    self.main_split
                        .local_docs
                        .insert(kind.clone(), editor_buffer_data.doc);
                }
                BufferContent::SettingsValue(name, ..) => {
                    self.main_split
                        .value_docs
                        .insert(name.clone(), editor_buffer_data.doc);
                }
            }
        }
    }

    pub fn completion_origin(
        &self,
        text: &mut PietText,
        tab_size: Size,
        completion_size: Size,
        config: &LapceConfig,
    ) -> Point {
        let line_height = self.config.editor.line_height() as f64;

        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return Point::ZERO,
        };

        match &editor.content {
            BufferContent::Local(_) => {
                *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
            }
            BufferContent::SettingsValue(..) => {
                *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
            }
            BufferContent::File(_) | BufferContent::Scratch(..) => {
                let doc = self.main_split.editor_doc(editor.view_id);
                let offset = self.completion.offset;
                let (point_above, point_below) =
                    doc.points_of_offset(text, offset, &editor.view, config);

                let mut origin = *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
                    + Vec2::new(point_below.x - line_height - 5.0, point_below.y);
                if origin.y + completion_size.height + 1.0 > tab_size.height {
                    let height = completion_size
                        .height
                        .min(self.completion.len() as f64 * line_height);
                    origin.y = editor.window_origin.borrow().y
                        - self.window_origin.borrow().y
                        + point_above.y
                        - height;
                }
                if origin.x + completion_size.width + 1.0 > tab_size.width {
                    origin.x = tab_size.width - completion_size.width - 1.0;
                }
                if origin.x <= 0.0 {
                    origin.x = 0.0;
                }

                origin
            }
        }
    }

    pub fn signature_origin(
        &self,
        text: &mut PietText,
        tab_size: Size,
        signature_size: Size,
        label_offset: f64,
        config: &LapceConfig,
    ) -> Point {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return Point::ZERO,
        };

        match &editor.content {
            BufferContent::File(_) | BufferContent::Scratch(_, _) => {
                let doc = self.main_split.editor_doc(editor.view_id);
                let offset = self.signature.offset;
                let (point_above, _point_below) =
                    doc.points_of_offset(text, offset, &editor.view, config);

                let mut origin = *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
                    + Vec2::new(point_above.x - 5.0 - label_offset, point_above.y)
                    - Vec2::new(0.0, signature_size.height);

                // TODO: What about if the signature's position is past the tab size?

                if origin.x + signature_size.width + 1.0 > tab_size.width {
                    origin.x = tab_size.width - signature_size.width - 1.0;
                }

                if origin.x <= 0.0 {
                    origin.x = 0.0;
                }

                origin
            }
            BufferContent::SettingsValue(_) | BufferContent::Local(_) => {
                *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
            }
        }
    }

    pub fn rename_origin(
        &self,
        text: &mut PietText,
        tab_size: Size,
        rename_size: Size,
        config: &LapceConfig,
    ) -> Point {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return Point::ZERO,
        };

        match &editor.content {
            BufferContent::Local(_) => {
                *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
            }
            BufferContent::SettingsValue(..) => {
                *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
            }
            BufferContent::File(_) | BufferContent::Scratch(..) => {
                let doc = self.main_split.editor_doc(editor.view_id);
                let offset = self.rename.start;
                let (point_above, point_below) =
                    doc.points_of_offset(text, offset, &editor.view, config);

                let mut origin = *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
                    + Vec2::new(point_below.x, point_below.y);
                if origin.y + rename_size.height + 1.0 > tab_size.height {
                    origin.y = editor.window_origin.borrow().y
                        - self.window_origin.borrow().y
                        + point_above.y
                        - rename_size.height;
                }
                if origin.x + rename_size.width + 1.0 > tab_size.width {
                    origin.x = tab_size.width - rename_size.width - 1.0;
                }
                if origin.x <= 0.0 {
                    origin.x = 0.0;
                }

                origin
            }
        }
    }

    pub fn hover_origin(
        &self,
        text: &mut PietText,
        tab_size: Size,
        config: &LapceConfig,
    ) -> Point {
        let line_height = self.config.editor.line_height() as f64;

        let editor = self.main_split.editors.get(&self.hover.editor_view_id);
        let editor = match editor {
            Some(editor) => editor,
            None => return Point::ZERO,
        };

        match &editor.content {
            BufferContent::Local(_) => {
                *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
            }
            BufferContent::SettingsValue(..) => {
                *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
            }
            BufferContent::File(_) | BufferContent::Scratch(..) => {
                let doc = self.main_split.editor_doc(editor.view_id);
                let offset = self.hover.offset;
                let (point, _) =
                    doc.points_of_offset(text, offset, &editor.view, config);
                let x = point.x;
                let y = point.y;
                let hover_size = *self.hover.content_size.borrow();
                let mut origin = *editor.window_origin.borrow()
                    - self.window_origin.borrow().to_vec2()
                    + Vec2::new(x, y - hover_size.height);
                if origin.y < 0.0 {
                    origin.y += hover_size.height + line_height;
                }
                if origin.x + hover_size.width + 1.0 > tab_size.width {
                    origin.x = tab_size.width - hover_size.width - 1.0;
                }
                if origin.x <= 0.0 {
                    origin.x = 0.0;
                }

                origin
            }
        }
    }

    pub fn palette_view_data(&self) -> PaletteViewData {
        PaletteViewData {
            palette: self.palette.clone(),
            workspace: self.workspace.clone(),
            main_split: self.main_split.clone(),
            keypress: self.keypress.clone(),
            config: self.config.clone(),
            find: self.find.clone(),
            db: self.db.clone(),
            focus_area: self.focus_area.clone(),
            terminal: self.terminal.clone(),
            source_control: self.source_control.clone(),
        }
    }

    pub fn run_workbench_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceWorkbenchCommand,
        data: Option<Value>,
        _count: Option<usize>,
        _env: &Env,
    ) {
        match command {
            LapceWorkbenchCommand::RestartToUpdate => {
                if let Some(release) = (*self.latest_release).clone() {
                    if release.version != *meta::VERSION {
                        if let Ok(process_path) = env::current_exe() {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::RestartToUpdate(
                                    process_path,
                                    release,
                                ),
                                Target::Global,
                            ));
                        }
                    }
                }
            }
            LapceWorkbenchCommand::CloseFolder => {
                if self.workspace.path.is_some() {
                    let mut workspace = (*self.workspace).clone();
                    workspace.path = None;
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SetWorkspace(workspace),
                        Target::Auto,
                    ));
                }
            }
            LapceWorkbenchCommand::OpenFolder => {
                if !self.workspace.kind.is_remote() {
                    let options = FileDialogOptions::new()
                        .select_directories()
                        .accept_command(LAPCE_OPEN_FOLDER);
                    ctx.submit_command(
                        druid::commands::SHOW_OPEN_PANEL.with(options),
                    );
                } else {
                    let picker = Arc::make_mut(&mut self.picker);
                    picker.active = true;
                    if let Some(node) = picker.root.get_file_node(&picker.pwd) {
                        if !node.read {
                            let tab_id = self.id;
                            let event_sink = ctx.get_external_handle();
                            FilePickerData::read_dir(
                                &node.path_buf,
                                tab_id,
                                &self.proxy,
                                event_sink,
                            );
                        }
                    }
                }
            }
            LapceWorkbenchCommand::OpenFile => {
                if !self.workspace.kind.is_remote() {
                    let options =
                        FileDialogOptions::new().accept_command(LAPCE_OPEN_FILE);
                    ctx.submit_command(
                        druid::commands::SHOW_OPEN_PANEL.with(options),
                    );
                } else {
                    let picker = Arc::make_mut(&mut self.picker);
                    picker.active = true;
                    if let Some(node) = picker.root.get_file_node(&picker.pwd) {
                        if !node.read {
                            let tab_id = self.id;
                            let event_sink = ctx.get_external_handle();
                            FilePickerData::read_dir(
                                &node.path_buf,
                                tab_id,
                                &self.proxy,
                                event_sink,
                            );
                        }
                    }
                }
            }
            LapceWorkbenchCommand::RevealActiveFileInFileExplorer => {
                let path = if let Some(editor) = self.main_split.active_editor() {
                    match &editor.content {
                        BufferContent::File(path) => path,
                        _ => return,
                    }
                } else {
                    return;
                };

                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RevealInFileExplorer(path.to_owned()),
                    Target::Auto,
                ))
            }
            LapceWorkbenchCommand::EnableModal => {
                let config = Arc::make_mut(&mut self.config);
                config.core.modal = true;
                LapceConfig::update_file(
                    "core",
                    "modal",
                    toml_edit::Value::from(true),
                );
            }
            LapceWorkbenchCommand::DisableModal => {
                let config = Arc::make_mut(&mut self.config);
                config.core.modal = false;
                LapceConfig::update_file(
                    "core",
                    "modal",
                    toml_edit::Value::from(false),
                );
            }
            LapceWorkbenchCommand::ChangeColorTheme => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::ColorTheme)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::ChangeIconTheme => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::IconTheme)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::NewFile => {
                self.main_split.new_file(ctx, &self.config);
            }
            LapceWorkbenchCommand::OpenLogFile => {
                if let Some(path) = (*self.log_file).clone() {
                    self.main_split.jump_to_location(
                        ctx,
                        None,
                        false,
                        EditorLocation {
                            path,
                            position: None::<usize>,
                            scroll_offset: None,
                            history: None,
                        },
                        &self.config,
                    );
                }
            }
            LapceWorkbenchCommand::OpenSettings => {
                self.main_split.open_settings(ctx, false, &self.config);
            }
            LapceWorkbenchCommand::OpenSettingsFile => {
                if let Some(path) = LapceConfig::settings_file() {
                    self.main_split.jump_to_location(
                        ctx,
                        None,
                        false,
                        EditorLocation {
                            path,
                            position: None::<usize>,
                            scroll_offset: None,
                            history: None,
                        },
                        &self.config,
                    );
                }
            }
            LapceWorkbenchCommand::OpenSettingsDirectory
            | LapceWorkbenchCommand::OpenProxyDirectory
            | LapceWorkbenchCommand::OpenThemesDirectory
            | LapceWorkbenchCommand::OpenLogsDirectory
            | LapceWorkbenchCommand::OpenPluginsDirectory => {
                use LapceWorkbenchCommand::*;
                let dir = match command {
                    OpenSettingsDirectory => Directory::config_directory(),
                    OpenProxyDirectory => Directory::proxy_directory(),
                    OpenThemesDirectory => Directory::themes_directory(),
                    OpenLogsDirectory => Directory::logs_directory(),
                    OpenPluginsDirectory => Directory::plugins_directory(),
                    _ => return,
                };
                if let Some(dir) = dir {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::OpenURI(dir.to_string_lossy().to_string()),
                        Target::Auto,
                    ))
                }
            }
            LapceWorkbenchCommand::OpenKeyboardShortcuts => {
                self.main_split.open_settings(ctx, true, &self.config);
            }
            LapceWorkbenchCommand::OpenKeyboardShortcutsFile => {
                if let Some(path) = LapceConfig::keymaps_file() {
                    self.main_split.jump_to_location(
                        ctx,
                        None,
                        false,
                        EditorLocation {
                            path,
                            position: None::<usize>,
                            scroll_offset: None,
                            history: None,
                        },
                        &self.config,
                    );
                }
            }
            LapceWorkbenchCommand::Palette => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(None),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::PaletteLine => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::Line)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::PaletteSymbol => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::DocumentSymbol)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::PaletteWorkspaceSymbol => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::WorkspaceSymbol)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::PaletteCommand => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::Command)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::PaletteWorkspace => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::Workspace)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::NewWindowTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NewTab(None),
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::CloseWindowTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CloseTab,
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::NextWindowTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NextTab,
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::PreviousWindowTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PreviousTab,
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::NewWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NewWindow(*self.window_id),
                    Target::Global,
                ));
            }
            LapceWorkbenchCommand::CloseWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CloseWindow(*self.window_id),
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::ReloadWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ReloadWindow,
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::ToggleMaximizedPanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        Arc::make_mut(&mut self.panel).toggle_maximize(&kind);
                    }
                } else {
                    Arc::make_mut(&mut self.panel).toggle_active_maximize();
                }
            }
            LapceWorkbenchCommand::FocusEditor => {
                if let Some(active) = *self.main_split.active_tab {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(active),
                    ));
                }
            }
            LapceWorkbenchCommand::FocusTerminal => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(self.terminal.widget_id),
                ));
            }

            LapceWorkbenchCommand::ToggleSourceControlVisual => {
                self.toggle_panel_visual(ctx, PanelKind::SourceControl);
            }
            LapceWorkbenchCommand::TogglePluginVisual => {
                self.toggle_panel_visual(ctx, PanelKind::Plugin);
            }
            LapceWorkbenchCommand::ToggleFileExplorerVisual => {
                self.toggle_panel_visual(ctx, PanelKind::FileExplorer);
            }
            LapceWorkbenchCommand::ToggleSearchVisual => {
                self.toggle_panel_visual(ctx, PanelKind::Search);
            }
            LapceWorkbenchCommand::ToggleProblemVisual => {
                self.toggle_panel_visual(ctx, PanelKind::Problem);
            }
            LapceWorkbenchCommand::ToggleTerminalVisual => {
                self.toggle_panel_visual(ctx, PanelKind::Terminal);
            }
            LapceWorkbenchCommand::TogglePanelVisual => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.toggle_panel_visual(ctx, kind);
                    }
                }
            }
            LapceWorkbenchCommand::TogglePanelLeftVisual => {
                self.toggle_container_visual(ctx, &PanelContainerPosition::Left);
            }
            LapceWorkbenchCommand::TogglePanelRightVisual => {
                self.toggle_container_visual(ctx, &PanelContainerPosition::Right);
            }
            LapceWorkbenchCommand::TogglePanelBottomVisual => {
                self.toggle_container_visual(ctx, &PanelContainerPosition::Bottom);
            }
            LapceWorkbenchCommand::ToggleSourceControlFocus => {
                self.toggle_panel_focus(ctx, PanelKind::SourceControl);
            }
            LapceWorkbenchCommand::TogglePluginFocus => {
                self.toggle_panel_focus(ctx, PanelKind::Plugin);
            }
            LapceWorkbenchCommand::ToggleFileExplorerFocus => {
                self.toggle_panel_focus(ctx, PanelKind::FileExplorer);
            }
            LapceWorkbenchCommand::ToggleSearchFocus => {
                self.toggle_panel_focus(ctx, PanelKind::Search);
            }
            LapceWorkbenchCommand::ToggleProblemFocus => {
                self.toggle_panel_focus(ctx, PanelKind::Problem);
            }
            LapceWorkbenchCommand::ToggleTerminalFocus => {
                self.toggle_panel_focus(ctx, PanelKind::Terminal);
            }
            LapceWorkbenchCommand::TogglePanelFocus => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.toggle_panel_focus(ctx, kind);
                    }
                }
            }

            LapceWorkbenchCommand::ShowPanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.show_panel(ctx, kind);
                    }
                }
            }
            LapceWorkbenchCommand::HidePanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.hide_panel(ctx, kind);
                    }
                }
            }
            LapceWorkbenchCommand::SourceControlInit => {
                self.proxy.proxy_rpc.git_init();
            }
            LapceWorkbenchCommand::SourceControlCommit => {
                let diffs: Vec<FileDiff> = self
                    .source_control
                    .file_diffs
                    .iter()
                    .filter_map(
                        |(_, (diff, checked))| {
                            if *checked {
                                Some(diff)
                            } else {
                                None
                            }
                        },
                    )
                    .cloned()
                    .collect();
                if diffs.is_empty() {
                    return;
                }
                let doc = self
                    .main_split
                    .local_docs
                    .get_mut(&LocalBufferKind::SourceControl)
                    .unwrap();
                let message = doc.buffer().to_string();
                let message = message.trim();
                if message.is_empty() {
                    return;
                }
                self.proxy.proxy_rpc.git_commit(message.to_string(), diffs);
                Arc::make_mut(doc).reload(Rope::from(""), true);
                let editor = self
                    .main_split
                    .editors
                    .get_mut(&self.source_control.editor_view_id)
                    .unwrap();
                Arc::make_mut(editor).cursor = if self.config.core.modal {
                    Cursor::new(CursorMode::Normal(0), None, None)
                } else {
                    Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None)
                };
            }
            LapceWorkbenchCommand::SourceControlCopyActiveFileRemoteUrl => {
                if let Some(editor) = self.main_split.active_editor() {
                    if let BufferContent::File(path) = &editor.content {
                        let event_sink = ctx.get_external_handle();

                        self.proxy.proxy_rpc.git_get_remote_file_url(
                            path.clone(),
                            move |result| {
                                if let Ok(ProxyResponse::GitGetRemoteFileUrl {
                                    file_url,
                                }) = result
                                {
                                    let _ = event_sink.submit_command(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::PutToClipboard(file_url),
                                        Target::Auto,
                                    );
                                }
                            },
                        )
                    }
                }
            }
            LapceWorkbenchCommand::SourceControlDiscardActiveFileChanges => {
                if let Some(editor) = self.main_split.active_editor() {
                    if let BufferContent::File(path) = &editor.content {
                        self.proxy
                            .proxy_rpc
                            .git_discard_files_changes(vec![path.clone()]);
                    }
                }
            }
            LapceWorkbenchCommand::SourceControlDiscardTargetFileChanges => {
                if let Ok(v) = serde_json::from_value::<FileDiff>(data.unwrap()) {
                    match v {
                        FileDiff::Added(path) => {
                            self.proxy.proxy_rpc.trash_path(
                                path,
                                Box::new(move |res| {
                                    if let Err(err) = res {
                                        log::warn!(
                                            "Failed to trash path: {:?}",
                                            err
                                        );
                                    }
                                }),
                            );
                        }
                        FileDiff::Deleted(path) => {
                            self.proxy
                                .proxy_rpc
                                .git_discard_files_changes(vec![path]);
                        }
                        FileDiff::Modified(path) => {
                            self.proxy
                                .proxy_rpc
                                .git_discard_files_changes(vec![path]);
                        }
                        FileDiff::Renamed(old_path, new_path) => {
                            self.proxy
                                .proxy_rpc
                                .git_discard_files_changes(vec![old_path]);

                            self.proxy.proxy_rpc.trash_path(
                                new_path,
                                Box::new(move |res| {
                                    if let Err(err) = res {
                                        log::warn!(
                                            "Failed to trash path: {:?}",
                                            err
                                        );
                                    }
                                }),
                            );
                        }
                    }
                } else {
                    log::error!("discard target file called without a target file");
                }
            }
            LapceWorkbenchCommand::SourceControlDiscardWorkspaceChanges => {
                self.proxy.proxy_rpc.git_discard_workspace_changes();
            }
            LapceWorkbenchCommand::CheckoutBranch => match data {
                Some(Value::String(branch)) => {
                    self.proxy.proxy_rpc.git_checkout(branch)
                }
                _ => log::error!("checkout called without a branch"), // TODO: How do I show a result to the user here?
            },

            LapceWorkbenchCommand::ConnectSshHost => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::SshHost)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            #[cfg(windows)]
            LapceWorkbenchCommand::ConnectWsl => ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SetWorkspace(LapceWorkspace {
                    kind: LapceWorkspaceType::RemoteWSL,
                    path: None,
                    last_open: 0,
                }),
                Target::Auto,
            )),
            LapceWorkbenchCommand::DisconnectRemote => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SetWorkspace(LapceWorkspace {
                        kind: LapceWorkspaceType::Local,
                        path: None,
                        last_open: 0,
                    }),
                    Target::Auto,
                ))
            }
            LapceWorkbenchCommand::ExportCurrentThemeSettings => {
                self.main_split.export_theme(ctx, &self.config);
            }
            LapceWorkbenchCommand::InstallTheme => {
                self.main_split.install_theme(ctx, &self.config);
            }
            LapceWorkbenchCommand::ChangeFileLanguage => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::Language)),
                    Target::Auto,
                ))
            }
            LapceWorkbenchCommand::NextEditorTab => {
                if let Some(active) = *self.main_split.active_tab {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::NextEditorTab,
                        Target::Widget(active),
                    ));
                }
            }
            LapceWorkbenchCommand::PreviousEditorTab => {
                if let Some(active) = *self.main_split.active_tab {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::PreviousEditorTab,
                        Target::Widget(active),
                    ));
                }
            }
            LapceWorkbenchCommand::ToggleInlayHints => {
                let config = Arc::make_mut(&mut self.config);
                config.editor.enable_inlay_hints = !config.editor.enable_inlay_hints;
                LapceConfig::update_file(
                    "editor",
                    "enable-inlay-hints",
                    toml_edit::Value::from(config.editor.enable_inlay_hints),
                );
            }
            LapceWorkbenchCommand::NewTerminalTab => {
                let terminal_panel = Arc::make_mut(&mut self.terminal);
                terminal_panel.new_tab(
                    self.workspace.clone(),
                    self.proxy.clone(),
                    &self.config,
                    ctx.get_external_handle(),
                );
                if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                    Arc::make_mut(&mut self.panel).show_panel(&PanelKind::Terminal);
                }
            }
            LapceWorkbenchCommand::NextTerminalTab => {
                let terminal_panel = Arc::make_mut(&mut self.terminal);
                if terminal_panel.active >= terminal_panel.tabs_order.len() - 1 {
                    terminal_panel.active = 0;
                } else {
                    terminal_panel.active += 1;
                }
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(terminal_panel.widget_id),
                ));
            }
            LapceWorkbenchCommand::PreviousTerminalTab => {
                let terminal_panel = Arc::make_mut(&mut self.terminal);
                if terminal_panel.active == 0 {
                    terminal_panel.active =
                        terminal_panel.tabs_order.len().saturating_sub(1);
                } else {
                    terminal_panel.active -= 1;
                }
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(terminal_panel.widget_id),
                ));
            }
            LapceWorkbenchCommand::CloseTerminalTab => {
                let split_id = data
                    .and_then(|d| serde_json::from_value::<usize>(d).ok())
                    .map(WidgetId::from_usize);
                let split_id = split_id
                    .or_else(|| {
                        self.terminal.active_terminal_split().map(|s| s.split_id)
                    })
                    .unwrap_or_else(WidgetId::next);
                let terminal_panel = Arc::make_mut(&mut self.terminal);
                terminal_panel.tabs.remove(&split_id);
                terminal_panel.tabs_order = Arc::new(
                    terminal_panel
                        .tabs_order
                        .iter()
                        .filter(|w| *w != &split_id)
                        .copied()
                        .collect(),
                );
                if terminal_panel.tabs_order.is_empty() {
                    if self.panel.is_panel_visible(&PanelKind::Terminal) {
                        Arc::make_mut(&mut self.panel)
                            .hide_panel(&PanelKind::Terminal);
                    }
                    if let Some(active) = *self.main_split.active_tab {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(active),
                        ));
                    }
                } else {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(terminal_panel.widget_id),
                    ));
                }
            }
            LapceWorkbenchCommand::ShowAbout => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ShowAbout,
                    Target::Widget(self.id),
                ));
            }
            LapceWorkbenchCommand::SaveAll => {
                let mut paths = HashSet::new();
                for (_, editor) in self.main_split.editors.iter() {
                    if let BufferContent::File(path) = &editor.content {
                        if paths.contains(path) {
                            continue;
                        }
                        paths.insert(path.to_path_buf());
                        if let Some(doc) = self.main_split.open_docs.get(path) {
                            if !doc.buffer().is_pristine() {
                                ctx.submit_command(Command::new(
                                    LAPCE_COMMAND,
                                    LapceCommand {
                                        kind: CommandKind::Focus(FocusCommand::Save),
                                        data: None,
                                    },
                                    Target::Widget(editor.view_id),
                                ));
                            }
                        }
                    }
                }
            }
            #[cfg(target_os = "macos")]
            LapceWorkbenchCommand::InstallToPATH => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunCommand(
                        String::from("osascript"),
                        vec![
                            String::from("-e"),
                            format!("do shell script \"ln -sf '{}' /usr/local/bin/lapce\" with administrator privileges", std::env::args().next().unwrap())
                        ]),
                    Target::Widget(self.id),
                ));
            }
            #[cfg(target_os = "macos")]
            LapceWorkbenchCommand::UninstallFromPATH => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunCommand(
                        String::from("osascript"),
                        vec![
                            String::from("-e"),
                            String::from("do shell script \"rm /usr/local/bin/lapce\" with administrator privileges")
                        ]),
                    Target::Widget(self.id),
                ));
            }
            LapceWorkbenchCommand::Quit => {
                ctx.submit_command(druid::commands::QUIT_APP);
            }
        }
    }

    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) {
        match &command.kind {
            CommandKind::Workbench(cmd) => {
                self.run_workbench_command(
                    ctx,
                    cmd,
                    command.data.clone(),
                    count,
                    env,
                );
            }
            CommandKind::Focus(_) | CommandKind::Edit(_) | CommandKind::Move(_) => {
                let widget_id = if *self.focus != self.palette.input_editor {
                    *self.focus
                } else if let Some(active_tab) = self.main_split.active_tab.as_ref()
                {
                    self.main_split
                        .editor_tabs
                        .get(active_tab)
                        .unwrap()
                        .active_child()
                        .map(|c| c.widget_id())
                        .unwrap_or(*self.focus)
                } else {
                    *self.focus
                };

                ctx.submit_command(Command::new(
                    LAPCE_COMMAND,
                    command.clone(),
                    Target::Widget(widget_id),
                ));
            }
            _ => {}
        }
    }

    pub fn terminal_update_process(
        tab_id: WidgetId,
        _palette_widget_id: WidgetId,
        receiver: Receiver<(TermId, TermEvent)>,
        event_sink: ExtEventSink,
        _workspace: Arc<LapceWorkspace>,
        _proxy: Arc<LapceProxy>,
    ) {
        let mut terminals = HashMap::new();
        let mut last_redraw = Instant::now();
        let mut last_event = None;
        loop {
            let (term_id, event) = if let Some((term_id, event)) = last_event.take()
            {
                (term_id, event)
            } else {
                match receiver.recv() {
                    Ok((term_id, event)) => (term_id, event),
                    Err(_) => return,
                }
            };
            match event {
                TermEvent::CloseTerminal => {
                    terminals.remove(&term_id);
                }
                TermEvent::NewTerminal(raw) => {
                    terminals.insert(term_id, raw);
                }
                TermEvent::UpdateContent(content) => {
                    if let Some(raw) = terminals.get_mut(&term_id) {
                        raw.lock().update_content(content);
                        last_event = receiver.try_recv().ok();
                        if last_event.is_some() {
                            if last_redraw.elapsed().as_millis() > 10 {
                                last_redraw = Instant::now();
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::RequestPaint,
                                    Target::Widget(tab_id),
                                );
                            }
                        } else {
                            last_redraw = Instant::now();
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::RequestPaint,
                                Target::Widget(tab_id),
                            );
                        }
                    }
                }
            }
        }
    }

    fn is_panel_focused(&self, kind: PanelKind) -> bool {
        // Moving between e.g. Search and Problems doesn't affect focus, so we need to also check
        // visibility.
        self.focus_area == FocusArea::Panel(kind)
            && self.panel.is_panel_visible(&kind)
    }

    fn hide_panel(&mut self, ctx: &mut EventCtx, kind: PanelKind) {
        Arc::make_mut(&mut self.panel).hide_panel(&kind);
        if let Some(active) = *self.main_split.active_tab {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(active),
            ));
        }
    }

    pub fn show_panel(&mut self, ctx: &mut EventCtx, kind: PanelKind) {
        Arc::make_mut(&mut self.panel).show_panel(&kind);
        let focus_id = match kind {
            PanelKind::FileExplorer => self.file_explorer.widget_id,
            PanelKind::SourceControl => self.source_control.active,
            PanelKind::Plugin => self.plugin.widget_id,
            PanelKind::Terminal => self.terminal.widget_id,
            PanelKind::Search => self.search.active,
            PanelKind::Problem => self.problem.widget_id,
        };
        if let PanelKind::Search = kind {
            ctx.submit_command(Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::MultiSelection(
                        MultiSelectionCommand::SelectAll,
                    ),
                    data: None,
                },
                Target::Widget(focus_id),
            ));
        }
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(focus_id),
        ));
    }

    fn toggle_panel_visual(&mut self, ctx: &mut EventCtx, kind: PanelKind) {
        if self.panel.is_panel_visible(&kind) {
            self.hide_panel(ctx, kind);
        } else {
            self.show_panel(ctx, kind);
        }
    }

    fn toggle_panel_focus(&mut self, ctx: &mut EventCtx, kind: PanelKind) {
        let should_hide = match kind {
            PanelKind::FileExplorer | PanelKind::Plugin | PanelKind::Problem => {
                // Some panels don't accept focus (yet). Fall back to visibility check
                // in those cases.
                self.panel.is_panel_visible(&kind)
            }
            PanelKind::Terminal | PanelKind::SourceControl | PanelKind::Search => {
                self.is_panel_focused(kind)
            }
        };
        if should_hide {
            self.hide_panel(ctx, kind);
        } else {
            self.show_panel(ctx, kind);
        }
    }

    pub fn toggle_container_visual(
        &mut self,
        ctx: &mut EventCtx,
        position: &PanelContainerPosition,
    ) {
        let shown = !self.panel.is_container_shown(position);
        let panel = Arc::make_mut(&mut self.panel);
        panel.set_shown(&position.first(), shown);
        panel.set_shown(&position.second(), shown);
        if shown {
            if let Some((kind, _)) =
                self.panel.active_panel_at_position(&position.second())
            {
                self.show_panel(ctx, kind);
            }
            if let Some((kind, _)) =
                self.panel.active_panel_at_position(&position.first())
            {
                self.show_panel(ctx, kind);
            }
        } else {
            if let Some((kind, _)) =
                self.panel.active_panel_at_position(&position.second())
            {
                self.hide_panel(ctx, kind);
            }
            if let Some((kind, _)) =
                self.panel.active_panel_at_position(&position.first())
            {
                self.hide_panel(ctx, kind);
            }
        }
    }

    pub fn read_picker_pwd(&mut self, ctx: &mut EventCtx) {
        let path = self.picker.pwd.clone();
        let event_sink = ctx.get_external_handle();
        let tab_id = self.id;
        FilePickerData::read_dir(&path, tab_id, &self.proxy, event_sink);
    }

    pub fn set_picker_pwd(&mut self, pwd: PathBuf) {
        let picker = Arc::make_mut(&mut self.picker);
        picker.pwd = pwd.clone();
        if let Some(s) = pwd.to_str() {
            let doc = self
                .main_split
                .local_docs
                .get_mut(&LocalBufferKind::FilePicker)
                .unwrap();
            let doc = Arc::make_mut(doc);
            doc.reload(Rope::from(s), true);
            let editor = self
                .main_split
                .editors
                .get_mut(&self.picker.editor_view_id)
                .unwrap();
            let editor = Arc::make_mut(editor);
            editor.cursor = if self.config.core.modal {
                Cursor::new(
                    CursorMode::Normal(doc.buffer().line_end_offset(0, false)),
                    None,
                    None,
                )
            } else {
                Cursor::new(
                    CursorMode::Insert(Selection::caret(
                        doc.buffer().line_end_offset(0, true),
                    )),
                    None,
                    None,
                )
            };
        }
    }

    pub fn handle_workspace_file_change(&self, _ctx: &mut EventCtx) {
        self.file_explorer.reload();
    }
}

pub struct LapceTabLens(pub WidgetId);

impl Lens<LapceWindowData, LapceTabData> for LapceTabLens {
    fn with<V, F: FnOnce(&LapceTabData) -> V>(
        &self,
        data: &LapceWindowData,
        f: F,
    ) -> V {
        let tab = data.tabs.get(&self.0).unwrap();
        f(tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceTabData) -> V>(
        &self,
        data: &mut LapceWindowData,
        f: F,
    ) -> V {
        let mut tab = data.tabs.get(&self.0).unwrap().clone();
        tab.keypress = data.keypress.clone();
        tab.latest_release = data.latest_release.clone();
        tab.log_file = data.log_file.clone();
        tab.update_in_progress = data.update_in_progress;
        tab.log_file = data.log_file.clone();
        tab.multiple_tab = data.tabs.len() > 1;
        if !tab.panel.order.same(&data.panel_orders) {
            Arc::make_mut(&mut tab.panel).order = data.panel_orders.clone();
        }
        let result = f(&mut tab);
        data.keypress = tab.keypress.clone();
        if !tab.panel.order.same(&data.panel_orders) {
            data.panel_orders = tab.panel.order.clone();
        }
        if !tab.same(data.tabs.get(&self.0).unwrap()) {
            data.tabs.insert(self.0, tab);
        }
        result
    }
}

pub struct LapceWindowLens(pub WindowId);

impl Lens<LapceData, LapceWindowData> for LapceWindowLens {
    fn with<V, F: FnOnce(&LapceWindowData) -> V>(
        &self,
        data: &LapceData,
        f: F,
    ) -> V {
        let tab = data.windows.get(&self.0).unwrap();
        f(tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceWindowData) -> V>(
        &self,
        data: &mut LapceData,
        f: F,
    ) -> V {
        let mut win = data.windows.get(&self.0).unwrap().clone();
        win.keypress = data.keypress.clone();
        win.latest_release = data.latest_release.clone();
        win.update_in_progress = data.update_in_process;
        win.panel_orders = data.panel_orders.clone();
        let result = f(&mut win);
        data.keypress = win.keypress.clone();
        data.panel_orders = win.panel_orders.clone();
        if !win.same(data.windows.get(&self.0).unwrap()) {
            data.windows.insert(self.0, win);
        }
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitContent {
    EditorTab(WidgetId),
    Split(WidgetId),
}

impl SplitContent {
    pub fn widget_id(&self) -> WidgetId {
        match &self {
            SplitContent::EditorTab(widget_id) => *widget_id,
            SplitContent::Split(split_id) => *split_id,
        }
    }

    pub fn content_info(&self, data: &LapceTabData) -> SplitContentInfo {
        match &self {
            SplitContent::EditorTab(widget_id) => {
                let editor_tab_data =
                    data.main_split.editor_tabs.get(widget_id).unwrap();
                SplitContentInfo::EditorTab(editor_tab_data.tab_info(data))
            }
            SplitContent::Split(split_id) => {
                let split_data = data.main_split.splits.get(split_id).unwrap();
                SplitContentInfo::Split(split_data.split_info(data))
            }
        }
    }

    pub fn set_split_id(&self, data: &mut LapceMainSplitData, split_id: WidgetId) {
        match &self {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data =
                    data.editor_tabs.get_mut(editor_tab_id).unwrap();
                Arc::make_mut(editor_tab_data).split = split_id;
            }
            SplitContent::Split(id) => {
                let split_data = data.splits.get_mut(id).unwrap();
                Arc::make_mut(split_data).parent_split = Some(split_id);
            }
        }
    }

    pub fn split_id(&self, data: &LapceMainSplitData) -> Option<WidgetId> {
        match &self {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data = data.editor_tabs.get(editor_tab_id).unwrap();
                Some(editor_tab_data.split)
            }
            SplitContent::Split(split_id) => {
                let split_data = data.splits.get(split_id).unwrap();
                split_data.parent_split
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorSplitContent {}

#[derive(Clone, Debug)]
pub struct EditorSplitData {
    pub widget_id: WidgetId,
    pub children: Vec<EditorSplitContent>,
    pub direction: SplitDirection,
}

#[derive(Clone, Debug)]
pub struct SplitData {
    pub parent_split: Option<WidgetId>,
    pub widget_id: WidgetId,
    pub children: Vec<SplitContent>,
    pub direction: SplitDirection,
    pub layout_rect: Rc<RefCell<Rect>>,
}

impl SplitData {
    pub fn split_info(&self, data: &LapceTabData) -> SplitInfo {
        let info = SplitInfo {
            direction: self.direction,
            children: self
                .children
                .iter()
                .map(|child| child.content_info(data))
                .collect(),
        };
        info
    }
}

// #[derive(Clone, Debug)]
// pub enum EditorKind {
//     PalettePreview,
//     SplitActive,
// }

#[derive(Clone, Data, Lens)]
pub struct LapceMainSplitData {
    pub tab_id: Arc<WidgetId>,
    pub split_id: Arc<WidgetId>,
    pub active_tab: Arc<Option<WidgetId>>,
    pub active: Arc<Option<WidgetId>>,
    pub editors: im::HashMap<WidgetId, Arc<LapceEditorData>>,
    pub editor_tabs: im::HashMap<WidgetId, Arc<LapceEditorTabData>>,
    pub splits: im::HashMap<WidgetId, Arc<SplitData>>,
    pub open_docs: im::HashMap<PathBuf, Arc<Document>>,
    pub local_docs: im::HashMap<LocalBufferKind, Arc<Document>>,
    pub value_docs: im::HashMap<String, Arc<Document>>,
    pub scratch_docs: im::HashMap<BufferId, Arc<Document>>,
    pub current_save_as: Option<Arc<(BufferContent, WidgetId, bool)>>,
    pub register: Arc<Register>,
    pub proxy: Arc<LapceProxy>,
    pub palette_preview_editor: Arc<WidgetId>,
    pub diagnostics: im::HashMap<PathBuf, Arc<Vec<EditorDiagnostic>>>,
    pub error_count: usize,
    pub warning_count: usize,
    pub workspace: Arc<LapceWorkspace>,
    pub db: Arc<LapceDb>,
    pub locations: Arc<Vec<EditorLocation>>,
    pub current_location: usize,
}

impl LapceMainSplitData {
    pub fn active_editor(&self) -> Option<&LapceEditorData> {
        let id = (*self.active)?;
        Some(self.editors.get(&id)?.as_ref())
    }

    pub fn content_doc(&self, content: &BufferContent) -> Arc<Document> {
        match content {
            BufferContent::File(path) => self.open_docs.get(path).unwrap().clone(),
            BufferContent::Local(kind) => self.local_docs.get(kind).unwrap().clone(),
            BufferContent::SettingsValue(name, ..) => {
                self.value_docs.get(name).unwrap().clone()
            }
            BufferContent::Scratch(id, _) => {
                self.scratch_docs.get(id).unwrap().clone()
            }
        }
    }

    pub fn content_doc_mut(
        &mut self,
        content: &BufferContent,
    ) -> &mut Arc<Document> {
        match content {
            BufferContent::File(path) => self.open_docs.get_mut(path).unwrap(),
            BufferContent::Local(kind) => self.local_docs.get_mut(kind).unwrap(),
            BufferContent::SettingsValue(name, ..) => {
                self.value_docs.get_mut(name).unwrap()
            }
            BufferContent::Scratch(id, _) => self.scratch_docs.get_mut(id).unwrap(),
        }
    }

    pub fn editor_doc(&self, editor_view_id: WidgetId) -> Arc<Document> {
        let editor = self.editors.get(&editor_view_id).unwrap();
        self.content_doc(&editor.content)
    }

    pub fn document_format(
        &mut self,
        path: &Path,
        rev: u64,
        edits: &Result<Vec<TextEdit>>,
    ) {
        let doc = self.open_docs.get(path).unwrap();
        if doc.rev() != rev {
            return;
        }

        if let Ok(edits) = edits {
            if !edits.is_empty() {
                let doc = self.open_docs.get_mut(path).unwrap();

                let edits = edits
                    .iter()
                    .map(|edit| {
                        let start =
                            doc.buffer().offset_of_position(&edit.range.start);
                        let end = doc.buffer().offset_of_position(&edit.range.end);
                        let selection = Selection::region(start, end);
                        (selection, edit.new_text.as_str())
                    })
                    .collect::<Vec<(Selection, &str)>>();

                self.edit(path, &edits, EditType::Other);
            }
        }
    }

    pub fn document_format_and_save(
        &mut self,
        ctx: &mut EventCtx,
        path: &Path,
        rev: u64,
        result: &Result<Vec<TextEdit>>,
        exit_widget_id: Option<WidgetId>,
    ) {
        self.document_format(path, rev, result);
        self.document_save(ctx, path, exit_widget_id);
    }

    pub fn document_save(
        &mut self,
        ctx: &mut EventCtx,
        path: &Path,
        exit_widget_id: Option<WidgetId>,
    ) {
        let doc = self.open_docs.get(path).unwrap();
        let rev = doc.rev();
        let event_sink = ctx.get_external_handle();
        let path = PathBuf::from(path);
        let tab_id = *self.tab_id;
        self.proxy.proxy_rpc.save(
            rev,
            path.clone(),
            Box::new(move |result| {
                if let Ok(ProxyResponse::SaveResponse {}) = result {
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::BufferSave {
                            path,
                            rev,
                            exit: exit_widget_id,
                        },
                        Target::Widget(tab_id),
                    );
                }
            }),
        );
    }

    pub fn diagnostics_items(
        &self,
        severity: DiagnosticSeverity,
    ) -> Vec<(&PathBuf, Vec<&EditorDiagnostic>)> {
        self.diagnostics
            .iter()
            .filter_map(|(path, diagnostic)| {
                if let Some(doc) = self.open_docs.get(path) {
                    return match doc.diagnostics.as_ref() {
                        Some(d) => {
                            let diagnostics: Vec<&EditorDiagnostic> = d
                                .iter()
                                .filter(|d| d.diagnostic.severity == Some(severity))
                                .collect();
                            if !diagnostics.is_empty() {
                                Some((path, diagnostics))
                            } else {
                                None
                            }
                        }
                        None => None,
                    };
                }
                let diagnostics: Vec<&EditorDiagnostic> = diagnostic
                    .iter()
                    .filter(|d| d.diagnostic.severity == Some(severity))
                    .collect();
                if !diagnostics.is_empty() {
                    Some((path, diagnostics))
                } else {
                    None
                }
            })
            .sorted_by_key(|(path, _)| *path)
            .collect()
    }

    fn cursor_apply_delta(&mut self, path: &Path, delta: &RopeDelta) {
        for (_view_id, editor) in self.editors.iter_mut() {
            if let BufferContent::File(current_path) = &editor.content {
                if current_path == path {
                    Arc::make_mut(editor).cursor.apply_delta(delta);
                }
            }
        }
    }

    pub fn edit(
        &mut self,
        path: &Path,
        edits: &[(impl AsRef<Selection>, &str)],
        edit_type: EditType,
    ) -> Option<RopeDelta> {
        let doc = self.open_docs.get_mut(path)?;

        let buffer_len = doc.buffer().len();
        let mut move_cursor = true;
        for (selection, _) in edits.iter() {
            let selection = selection.as_ref();
            if selection.min_offset() == 0
                && selection.max_offset() >= buffer_len - 1
            {
                move_cursor = false;
                break;
            }
        }

        let (delta, _, _) = Arc::make_mut(doc).do_raw_edit(edits, edit_type);
        if move_cursor {
            self.cursor_apply_delta(path, &delta);
        }
        Some(delta)
    }

    pub fn get_active_tab_mut(
        &mut self,
        ctx: &mut EventCtx,
    ) -> &mut LapceEditorTabData {
        if self.active_tab.is_none() {
            let split = self.splits.get_mut(&self.split_id).unwrap();
            let split = Arc::make_mut(split);

            let editor_tab = LapceEditorTabData {
                widget_id: WidgetId::next(),
                split: *self.split_id,
                active: 0,
                children: Vector::new(),
                layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
                content_is_hot: Rc::new(RefCell::new(false)),
            };

            self.active_tab = Arc::new(Some(editor_tab.widget_id));
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitAdd(
                    0,
                    SplitContent::EditorTab(editor_tab.widget_id),
                    true,
                ),
                Target::Widget(*self.split_id),
            ));
            split
                .children
                .push(SplitContent::EditorTab(editor_tab.widget_id));
            self.editor_tabs
                .insert(editor_tab.widget_id, Arc::new(editor_tab));
        }

        Arc::make_mut(
            self.editor_tabs
                .get_mut(&(*self.active_tab.clone()).unwrap())
                .unwrap(),
        )
    }

    fn new_editor_tab(
        &mut self,
        ctx: &mut EventCtx,
        split_id: WidgetId,
    ) -> WidgetId {
        let split = self.splits.get_mut(&split_id).unwrap();
        let split = Arc::make_mut(split);

        let editor_tab_id = WidgetId::next();
        let editor_tab = LapceEditorTabData {
            widget_id: editor_tab_id,
            split: split_id,
            active: 0,
            children: Vector::new(),
            layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
            content_is_hot: Rc::new(RefCell::new(false)),
        };
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::SplitAdd(
                0,
                SplitContent::EditorTab(editor_tab.widget_id),
                true,
            ),
            Target::Widget(split_id),
        ));
        self.active_tab = Arc::new(Some(editor_tab.widget_id));
        split
            .children
            .push(SplitContent::EditorTab(editor_tab.widget_id));
        self.editor_tabs.insert(editor_tab_id, Arc::new(editor_tab));
        editor_tab_id
    }

    fn editor_tab_new_settings(
        &mut self,
        _ctx: &mut EventCtx,
        editor_tab_id: WidgetId,
        config: &LapceConfig,
    ) -> WidgetId {
        let editor_tab = self.editor_tabs.get_mut(&editor_tab_id).unwrap();
        let editor_tab = Arc::make_mut(editor_tab);
        let editor = LapceEditorData::new(
            None,
            None,
            None,
            BufferContent::Local(LocalBufferKind::Keymap),
            config,
        );
        let keymap_input_view_id = editor.view_id;
        self.editors.insert(editor.view_id, Arc::new(editor));
        let child = EditorTabChild::Settings {
            settings_widget_id: WidgetId::next(),
            editor_tab_id,
            keymap_input_view_id,
        };
        editor_tab.children.push_back(child.clone());
        child.widget_id()
    }

    fn editor_tab_new_editor(
        &mut self,
        _ctx: &mut EventCtx,
        editor_tab_id: WidgetId,
        config: &LapceConfig,
    ) -> WidgetId {
        let editor_tab = self.editor_tabs.get_mut(&editor_tab_id).unwrap();
        let editor_tab = Arc::make_mut(editor_tab);
        let editor = Arc::new(LapceEditorData::new(
            None,
            None,
            Some(editor_tab.widget_id),
            BufferContent::Local(LocalBufferKind::Empty),
            config,
        ));
        editor_tab.children.push_back(EditorTabChild::Editor(
            editor.view_id,
            editor.editor_id,
            editor.find_view_id,
        ));
        self.insert_editor(editor.clone(), config);
        editor.view_id
    }

    fn get_editor_from_tab(
        &mut self,
        ctx: &mut EventCtx,
        editor_tab_id: WidgetId,
        same_tab: bool,
        path: Option<PathBuf>,
        scratch: bool,
        config: &LapceConfig,
    ) -> &mut LapceEditorData {
        // If you're asking for no specific path, and you don't want scratch, then we just give you
        // the editor data for the tab id you're asking for.
        if path.is_none() && !scratch {
            let editor_tab = self.editor_tabs.get(&editor_tab_id).unwrap();
            if let Some(EditorTabChild::Editor(id, _, _)) = editor_tab.active_child()
            {
                return Arc::make_mut(self.editors.get_mut(id).unwrap());
            }
        }

        let mut editor_size = Size::ZERO;
        let editor_tabs: Box<
            dyn Iterator<Item = (&WidgetId, &mut Arc<LapceEditorTabData>)>,
        > = if same_tab {
            // If you want the same tab, then we'll only look at the tab you're asking for.
            Box::new(
                vec![(
                    &editor_tab_id,
                    self.editor_tabs.get_mut(&editor_tab_id).unwrap(),
                )]
                .into_iter(),
            )
        } else {
            // Otherwise, we look at all the open tabs available
            Box::new(self.editor_tabs.iter_mut().sorted_by(|(_, a), (_, b)| {
                // Sort the active tab to the start of the iterator
                if Some(a.widget_id) == *self.active_tab {
                    return Ordering::Less;
                }
                if Some(b.widget_id) == *self.active_tab {
                    return Ordering::Greater;
                }
                let a_rect = a.layout_rect.borrow();
                let b_rect = b.layout_rect.borrow();

                // Sort by the start position of the tab
                if a_rect.y0 == b_rect.y0 {
                    a_rect.x0.total_cmp(&b_rect.x0)
                } else {
                    a_rect.y0.total_cmp(&b_rect.y0)
                }
            }))
        };

        // Look for any editor tabs in our 'allowed set' that have the same path, and simply use
        // that editor tab instead of creating a new one.
        for (_, editor_tab) in editor_tabs {
            let editor_tab = Arc::make_mut(editor_tab);
            for (i, child) in editor_tab.children.iter().enumerate() {
                if let EditorTabChild::Editor(id, _, _) = child {
                    let editor = self.editors.get(id).unwrap();
                    let current_size = *editor.size.borrow();
                    if current_size.height > 0.0 {
                        editor_size = current_size;
                    }

                    if let Some(path) = path.as_ref() {
                        if editor.content == BufferContent::File(path.clone()) {
                            editor_tab.active = i;
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(*id),
                            ));
                            return Arc::make_mut(self.editors.get_mut(id).unwrap());
                        }
                    }
                }
            }
        }

        // If we're not showing tabs, or we're just asking for non-specific-path and non-scratch
        // editors, then we'll return the active editor if it is pristine.
        if !config.editor.show_tab || (path.is_none() && !scratch) {
            let editor_tab =
                Arc::make_mut(self.editor_tabs.get_mut(&editor_tab_id).unwrap());
            if let Some(EditorTabChild::Editor(id, _, _)) = editor_tab.active_child()
            {
                let editor = self.editors.get_mut(id).unwrap();
                if let BufferContent::File(path) = &editor.content {
                    if let Some(doc) = self.open_docs.get(path) {
                        if doc.buffer().is_pristine() {
                            return Arc::make_mut(self.editors.get_mut(id).unwrap());
                        }
                    }
                }
            }
        }

        // We didn't find an open editor that met our specifications, so we'll just have to create
        // one.
        let editor_tab =
            Arc::make_mut(self.editor_tabs.get_mut(&editor_tab_id).unwrap());
        let new_editor = Arc::new(LapceEditorData::new(
            None,
            None,
            Some(editor_tab.widget_id),
            BufferContent::Local(LocalBufferKind::Empty),
            config,
        ));
        *new_editor.size.borrow_mut() = editor_size;
        editor_tab.children.insert(
            editor_tab.active + 1,
            EditorTabChild::Editor(
                new_editor.view_id,
                new_editor.editor_id,
                new_editor.find_view_id,
            ),
        );
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EditorTabAdd(
                editor_tab.active + 1,
                EditorTabChild::Editor(
                    new_editor.view_id,
                    new_editor.editor_id,
                    new_editor.find_view_id,
                ),
            ),
            Target::Widget(editor_tab.widget_id),
        ));
        editor_tab.active += 1;
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(new_editor.view_id),
        ));
        self.insert_editor(new_editor.clone(), config);

        return Arc::make_mut(self.editors.get_mut(&new_editor.view_id).unwrap());
    }

    /// If the supplied `editor_view_id` is some, then this simply returns the editor data for it.  
    /// Otherwise, we check the active tab (and friends if `same_tab` is false) to see if there is
    /// an existing editor that matches the parameters. If not, we create a new editor.  
    /// Note that this does not load the file into the editor. See
    /// [`LapceMainSplitData::jump_to_location`] or [`LapceMainSplitData::go_to_location`] for
    /// creating the editor and loading the file.
    fn get_editor_or_new(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        same_tab: bool,
        path: Option<PathBuf>,
        scratch: bool,
        config: &LapceConfig,
    ) -> &mut LapceEditorData {
        match editor_view_id {
            Some(view_id) => Arc::make_mut(self.editors.get_mut(&view_id).unwrap()),
            None => match *self.active_tab {
                Some(active) => self.get_editor_from_tab(
                    ctx, active, same_tab, path, scratch, config,
                ),
                None => {
                    let editor_tab_id = self.new_editor_tab(ctx, *self.split_id);
                    let view_id =
                        self.editor_tab_new_editor(ctx, editor_tab_id, config);
                    Arc::make_mut(self.editors.get_mut(&view_id).unwrap())
                }
            },
        }
    }

    pub fn jump_to_position(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        same_tab: bool,
        position: Position,
        config: &LapceConfig,
    ) {
        let editor = self.get_editor_or_new(
            ctx,
            editor_view_id,
            same_tab,
            None,
            false,
            config,
        );
        let path = if let BufferContent::File(path) = &editor.content {
            Some(path.clone())
        } else {
            None
        };

        if let Some(path) = path {
            let location = EditorLocation {
                path,
                position: Some(position),
                scroll_offset: None,
                history: None,
            };
            self.jump_to_location(ctx, editor_view_id, same_tab, location, config);
        }
    }

    /// Open the plugin information view, which display's the plugins readme, repo, and other
    /// related bits
    pub fn open_plugin_info(&mut self, ctx: &mut EventCtx, volt: &VoltInfo) {
        let editor_tab_id = self
            .active_tab
            .as_ref()
            .unwrap_or_else(|| self.new_editor_tab(ctx, *self.split_id));

        let editor_tab =
            Arc::make_mut(self.editor_tabs.get_mut(&editor_tab_id).unwrap());

        let open_volt_id = volt.id();
        let mut existing: Option<WidgetId> = None;
        for (i, child) in editor_tab.children.iter().enumerate() {
            if let EditorTabChild::Plugin { volt_id, .. } = child {
                if &open_volt_id == volt_id {
                    editor_tab.active = i;
                    existing = Some(child.widget_id());
                    break;
                }
            }
        }

        let widget_id = existing.unwrap_or_else(|| {
            let child = EditorTabChild::Plugin {
                widget_id: WidgetId::next(),
                volt_id: volt.id(),
                volt_name: volt.display_name.clone(),
                editor_tab_id: editor_tab.widget_id,
            };

            let new_tab = editor_tab.children.is_empty();
            let index = if new_tab { 0 } else { editor_tab.active + 1 };
            editor_tab.children.insert(index, child.clone());
            if !new_tab {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EditorTabAdd(index, child.clone()),
                    Target::Widget(editor_tab.widget_id),
                ));
            }
            editor_tab.active = index;
            child.widget_id()
        });
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(widget_id),
        ));
    }

    pub fn open_settings(
        &mut self,
        ctx: &mut EventCtx,
        show_key_bindings: bool,
        config: &LapceConfig,
    ) {
        let widget_id = match *self.active_tab {
            Some(active) => {
                let editor_tab =
                    Arc::make_mut(self.editor_tabs.get_mut(&active).unwrap());
                let mut existing: Option<WidgetId> = None;
                for (i, child) in editor_tab.children.iter().enumerate() {
                    if let EditorTabChild::Settings { .. } = child {
                        editor_tab.active = i;
                        existing = Some(child.widget_id());
                        break;
                    }
                }

                if let Some(widget_id) = existing {
                    widget_id
                } else {
                    let editor = LapceEditorData::new(
                        None,
                        None,
                        None,
                        BufferContent::Local(LocalBufferKind::Keymap),
                        config,
                    );
                    let keymap_input_view_id = editor.view_id;
                    self.editors.insert(editor.view_id, Arc::new(editor));
                    let child = EditorTabChild::Settings {
                        settings_widget_id: WidgetId::next(),
                        editor_tab_id: editor_tab.widget_id,
                        keymap_input_view_id,
                    };
                    editor_tab
                        .children
                        .insert(editor_tab.active + 1, child.clone());
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EditorTabAdd(
                            editor_tab.active + 1,
                            child.clone(),
                        ),
                        Target::Widget(editor_tab.widget_id),
                    ));
                    editor_tab.active += 1;
                    child.widget_id()
                }
            }
            None => {
                let editor_tab_id = self.new_editor_tab(ctx, *self.split_id);
                self.editor_tab_new_settings(ctx, editor_tab_id, config)
            }
        };
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(widget_id),
        ));
        if show_key_bindings {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowKeybindings,
                Target::Widget(widget_id),
            ));
        } else {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ShowSettings,
                Target::Widget(widget_id),
            ));
        }
    }

    /// Jump to a specific location, getting/creating the editor as needed.
    pub fn jump_to_location<P: EditorPosition + Send + 'static>(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        same_tab: bool,
        location: EditorLocation<P>,
        config: &LapceConfig,
    ) -> WidgetId {
        self.jump_to_location_cb::<P, fn(&mut EventCtx, &mut LapceMainSplitData)>(
            ctx,
            editor_view_id,
            same_tab,
            location,
            config,
            None,
        )
    }

    /// Jump to a specific location, getting/creating the editor as needed.  
    /// This version allows a callback which will be called once the buffer is loaded.
    pub fn jump_to_location_cb<
        P: EditorPosition + Send + 'static,
        F: Fn(&mut EventCtx, &mut LapceMainSplitData) + Send + 'static,
    >(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        same_tab: bool,
        location: EditorLocation<P>,
        config: &LapceConfig,
        cb: Option<F>,
    ) -> WidgetId {
        // If there's an active editor tab, save the jump location so that the user can quickly go
        // back to it using the jump commands
        if let Some(active_tab) = self.active_tab.as_ref() {
            let editor_tab = self.editor_tabs.get(active_tab).unwrap();
            if let Some(EditorTabChild::Editor(view_id, _, _)) =
                editor_tab.active_child()
            {
                let editor = self.editors.get(view_id).unwrap();
                if let BufferContent::File(path) = &editor.content {
                    self.save_jump_location(
                        path.to_path_buf(),
                        editor.cursor.offset(),
                        editor.scroll_offset,
                    );
                }
            }
        }
        // Get an existing editor for the file, if it exists, otherwise create a new one
        let editor_view_id = self
            .get_editor_or_new(
                ctx,
                editor_view_id,
                same_tab,
                Some(location.path.clone()),
                false,
                config,
            )
            .view_id;
        // Actually jump to the requisite location in our constructed editor
        self.go_to_location_cb::<P, F>(
            ctx,
            Some(editor_view_id),
            same_tab,
            location,
            config,
            cb,
        );
        editor_view_id
    }

    pub fn can_jump_location_backward(&self) -> bool {
        self.current_location >= 1
    }

    pub fn can_jump_location_forward(&self) -> bool {
        !(self.locations.is_empty()
            || self.current_location >= self.locations.len() - 1)
    }

    pub fn save_jump_location(
        &mut self,
        path: PathBuf,
        offset: usize,
        scroll_offset: Vec2,
    ) {
        if let Some(last_location) = self.locations.last() {
            if last_location.path == path
                && last_location.position == Some(offset)
                && last_location.scroll_offset == Some(scroll_offset)
            {
                return;
            }
        }
        let location = EditorLocation {
            path,
            position: Some(offset),
            scroll_offset: Some(scroll_offset),
            history: None,
        };
        Arc::make_mut(&mut self.locations).push(location);
        self.current_location = self.locations.len();
    }

    fn get_name_for_new_file(&self) -> String {
        const PREFIX: &str = "Untitled-";

        // Checking just the current scratch_docs rather than all the different document
        // collections seems to be the right thing to do. The user may have genuine 'new N'
        // files tucked away somewhere in their workspace.
        let new_num = self
            .scratch_docs
            .values()
            .filter_map(|doc| match doc.content() {
                BufferContent::Scratch(_, existing_name) => {
                    // The unwraps are safe because scratch docs are always
                    // titled the same format and the user cannot change the name.
                    let num_part = existing_name.strip_prefix(PREFIX).unwrap();
                    let num = num_part.parse::<i32>().unwrap();
                    Some(num)
                }
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1;

        format!("{PREFIX}{new_num}")
    }

    pub fn install_theme(&mut self, ctx: &mut EventCtx, _config: &LapceConfig) {
        let tab = self.get_active_tab_mut(ctx);
        let child = tab.active_child().cloned();
        if let Some(child) = child {
            match child {
                EditorTabChild::Editor(view_id, _, _) => {
                    let editor = self.editors.get(&view_id).unwrap();
                    if let BufferContent::File(ref path) = editor.content {
                        if let Some(folder) = Directory::themes_directory() {
                            if let Some(file_name) = path.file_name() {
                                let _ = std::fs::copy(path, folder.join(file_name));
                            }
                        }
                    }
                }
                EditorTabChild::Settings { .. } => {}
                EditorTabChild::Plugin { .. } => {}
            }
        }
    }

    pub fn export_theme(&mut self, ctx: &mut EventCtx, config: &LapceConfig) {
        let id = self.new_file(ctx, config);
        let doc = self.scratch_docs.get_mut(&id).unwrap();
        let doc = Arc::make_mut(doc);

        #[cfg(feature = "lang-toml")]
        doc.set_language(lapce_core::language::LapceLanguage::Toml);

        doc.reload(Rope::from(config.export_theme()), true);
    }

    pub fn new_file(
        &mut self,
        ctx: &mut EventCtx,
        config: &LapceConfig,
    ) -> BufferId {
        let tab_id = *self.tab_id;
        let proxy = self.proxy.clone();
        let buffer_id = BufferId::next();
        let content =
            BufferContent::Scratch(buffer_id, self.get_name_for_new_file());
        let doc =
            Document::new(content.clone(), tab_id, ctx.get_external_handle(), proxy);
        self.scratch_docs.insert(buffer_id, Arc::new(doc));

        let editor = self.get_editor_or_new(ctx, None, true, None, true, config);
        editor.content = content;
        editor.cursor = if config.core.modal {
            Cursor::new(CursorMode::Normal(0), None, None)
        } else {
            Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None)
        };
        buffer_id
    }

    pub fn go_to_location<P: EditorPosition + Send + 'static>(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        same_tab: bool,
        location: EditorLocation<P>,
        config: &LapceConfig,
    ) {
        // Unfortunately this is the 'nicest' way I know to pass in no callback to an Option<F>
        self.go_to_location_cb::<P, fn(&mut EventCtx, &mut LapceMainSplitData)>(
            ctx,
            editor_view_id,
            same_tab,
            location,
            config,
            None,
        );
    }

    /// Go to the location in the editor
    /// `cb` is called when the buffer is loaded, or immediately if it is already loaded.
    pub fn go_to_location_cb<
        P: EditorPosition + Send + 'static,
        F: Fn(&mut EventCtx, &mut LapceMainSplitData) + Send + 'static,
    >(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        same_tab: bool,
        location: EditorLocation<P>,
        config: &LapceConfig,
        cb: Option<F>,
    ) {
        let editor_view_id = self
            .get_editor_or_new(
                ctx,
                editor_view_id,
                same_tab,
                Some(location.path.clone()),
                false,
                config,
            )
            .view_id;
        let doc = self.editor_doc(editor_view_id);

        // Whether we're swapping to a different file/kind-of-buffer
        let new_buffer = match doc.content() {
            BufferContent::File(path) => {
                if path != &location.path {
                    // different path
                    true
                } else {
                    // same path, then check history version and EditorView change
                    let editor = self.editors.get(&editor_view_id).unwrap();
                    if let EditorView::Diff(old_version) = editor.view.clone() {
                        if let Some(new_version) = location.history.clone() {
                            // old editor is DiffView, and OpenFileDiff with 'history version'
                            // check history version
                            new_version != old_version
                        } else {
                            // old editor is DiffView, but OpenFile without 'history version'
                            true
                        }
                    } else {
                        // old editor is NormalView, but OpenFileDiff with 'history version'
                        location.history.is_some()
                    }
                }
            }
            BufferContent::Local(_) => true,
            BufferContent::SettingsValue(..) => true,
            BufferContent::Scratch(..) => true,
        };

        if new_buffer {
            // Save the position in the document so that when the user reopens it, they'll
            // return to the same place
            self.db.save_doc_position(&self.workspace, &doc);
        } else if location.position.is_none()
            && location.scroll_offset.is_none()
            && location.history.is_none()
        {
            // If it is not a new buffer (so it is the same file); and there's no positioning,
            // scrolling, or history, then we don't need to do anything to the editor at all.
            return;
        }
        let path = location.path.clone();
        // TODO: Could this just be done via an if let Some()? Would have to reorder the if/else
        let doc_exists = self.open_docs.contains_key(&path);
        if !doc_exists {
            // There's no existing document for the path, so we need to construct a new one.
            let mut doc = Document::new(
                BufferContent::File(path.clone()),
                *self.tab_id,
                ctx.get_external_handle(),
                self.proxy.clone(),
            );

            // Acquire information about the buffer when it was last accessed, restoring their
            // scroll & cursor position.
            if let Ok(info) = self.db.get_buffer_info(&self.workspace, &path) {
                doc.scroll_offset =
                    Vec2::new(info.scroll_offset.0, info.scroll_offset.1);
                doc.cursor_offset = info.cursor_offset;
            }

            let cb: Option<InitBufferContentCb> = cb.map(|cb| Box::new(cb) as _);

            // Since we don't have document loaded, we'll have to retrieve it from the proxy
            // So, the document is not immediately filled with content!
            doc.retrieve_file(vec![(editor_view_id, location)], None, cb, config);
            self.open_docs.insert(path.clone(), Arc::new(doc));
        } else {
            let doc = self.open_docs.get_mut(&path).unwrap().clone();

            let (offset, scroll_offset) = match &location.position {
                Some(offset) => {
                    let doc = self.open_docs.get_mut(&path).unwrap();
                    let doc = Arc::make_mut(doc);

                    // Convert the offset into a utf8 form for us to use
                    let offset = {
                        let offset = offset.to_utf8_offset(doc.buffer());
                        doc.cursor_offset = offset;
                        offset
                    };

                    if let Some(scroll_offset) = location.scroll_offset.as_ref() {
                        doc.scroll_offset = *scroll_offset;
                    }

                    (offset, location.scroll_offset.as_ref())
                }
                // No custom position, so we'll simply keep them where they were
                None => (doc.cursor_offset, Some(&doc.scroll_offset)),
            };
            // Ensure that the offset is within the bounds of the document
            let offset = offset.min(doc.buffer().len());

            // Update the document's source control history with the given version
            if let Some(version) = location.history.as_ref() {
                let doc = self.open_docs.get_mut(&path).unwrap();
                // TODO(minor): Could we avoid this make mut definitely cloning the `Document` by
                // early-dropping our held doc above?
                Arc::make_mut(doc)
                    .retrieve_history(version, config.editor.diff_context_lines);
            }

            let editor = self.get_editor_or_new(
                ctx,
                Some(editor_view_id),
                same_tab,
                Some(location.path.clone()),
                false,
                config,
            );
            editor.view = if let Some(version) = location.history.as_ref() {
                // If they've provided us with a history version, then we're comparing with that
                // version of the file.
                EditorView::Diff(version.to_string())
            } else {
                EditorView::Normal
            };
            editor.content = BufferContent::File(path.clone());
            editor.compare = location.history.clone();
            editor.cursor = if config.core.modal {
                Cursor::new(CursorMode::Normal(offset), None, None)
            } else {
                Cursor::new(CursorMode::Insert(Selection::caret(offset)), None, None)
            };

            if let Some(scroll_offset) = scroll_offset {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ForceScrollTo(scroll_offset.x, scroll_offset.y),
                    Target::Widget(editor.view_id),
                ));
            } else if new_buffer || editor_view_id == *self.palette_preview_editor {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorPosition(
                        EnsureVisiblePosition::CenterOfWindow,
                    ),
                    Target::Widget(editor_view_id),
                ));
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorVisible(Some(
                        EnsureVisiblePosition::CenterOfWindow,
                    )),
                    Target::Widget(editor_view_id),
                ));
            }

            // Finished setting up the file, so alert the caller
            if let Some(cb) = cb {
                (cb)(ctx, self);
            }
        }
    }

    pub fn jump_to_line(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        line: usize,
        config: &LapceConfig,
    ) {
        let editor =
            self.get_editor_or_new(ctx, editor_view_id, true, None, false, config);
        let path = if let BufferContent::File(path) = &editor.content {
            Some(path.clone())
        } else {
            None
        };

        let position = Line(line);

        if let Some(path) = path {
            let location = EditorLocation {
                path,
                position: Some(position),
                scroll_offset: None,
                history: None,
            };
            self.jump_to_location(ctx, editor_view_id, true, location, config);
        }
    }
}

impl LapceMainSplitData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tab_id: WidgetId,
        workspace_info: Option<&WorkspaceInfo>,
        palette_preview_editor: WidgetId,
        proxy: Arc<LapceProxy>,
        config: &LapceConfig,
        event_sink: ExtEventSink,
        workspace: Arc<LapceWorkspace>,
        db: Arc<LapceDb>,
        unsaved_buffers: im::HashMap<String, String>,
    ) -> Self {
        let split_id = Arc::new(WidgetId::next());

        let mut editors = im::HashMap::new();
        let editor_tabs = im::HashMap::new();
        let splits = im::HashMap::new();

        let open_docs = im::HashMap::new();
        let mut local_docs = im::HashMap::new();
        local_docs.insert(
            LocalBufferKind::Empty,
            Arc::new(Document::new(
                BufferContent::Local(LocalBufferKind::Empty),
                tab_id,
                event_sink.clone(),
                proxy.clone(),
            )),
        );
        local_docs.insert(
            LocalBufferKind::PathName,
            Arc::new(Document::new(
                BufferContent::Local(LocalBufferKind::PathName),
                tab_id,
                event_sink.clone(),
                proxy.clone(),
            )),
        );
        let value_docs = im::HashMap::new();
        let scratch_docs = im::HashMap::new();

        let editor = LapceEditorData::new(
            Some(palette_preview_editor),
            None,
            None,
            BufferContent::Local(LocalBufferKind::Empty),
            config,
        );
        editors.insert(editor.view_id, Arc::new(editor));

        let mut main_split_data = Self {
            tab_id: Arc::new(tab_id),
            split_id,
            editors,
            editor_tabs,
            splits,
            open_docs,
            local_docs,
            value_docs,
            scratch_docs,
            active: Arc::new(None),
            active_tab: Arc::new(None),
            register: Arc::new(Register::default()),
            current_save_as: None,
            proxy,
            palette_preview_editor: Arc::new(palette_preview_editor),
            diagnostics: im::HashMap::new(),
            error_count: 0,
            warning_count: 0,
            workspace,
            db,
            locations: Arc::new(Vec::new()),
            current_location: 0,
        };

        if let Some(info) = workspace_info {
            let mut positions = HashMap::new();
            let split_data = info.split.to_data(
                &mut main_split_data,
                None,
                &mut positions,
                tab_id,
                config,
                event_sink,
            );
            main_split_data.split_id = Arc::new(split_data.widget_id);
            for (path, locations) in positions.into_iter() {
                let unsaved_buffer = unsaved_buffers
                    .get(&path.to_str().unwrap().to_string())
                    .map(Rope::from);
                Arc::make_mut(main_split_data.open_docs.get_mut(&path).unwrap())
                    .retrieve_file(locations.clone(), unsaved_buffer, None, config);
            }
        } else {
            main_split_data.splits.insert(
                *main_split_data.split_id,
                Arc::new(SplitData {
                    parent_split: None,
                    widget_id: *main_split_data.split_id,
                    children: Vec::new(),
                    direction: SplitDirection::Vertical,
                    layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
                }),
            );
        }
        main_split_data
    }

    pub fn insert_editor(
        &mut self,
        editor: Arc<LapceEditorData>,
        config: &LapceConfig,
    ) {
        if let Some((find_view_id, find_editor_id)) = editor.find_view_id {
            let mut find_editor = LapceEditorData::new(
                Some(find_view_id),
                Some(find_editor_id),
                None,
                BufferContent::Local(LocalBufferKind::Search),
                config,
            );
            find_editor.parent_view_id = Some(editor.view_id);
            self.editors
                .insert(find_editor.view_id, Arc::new(find_editor));
        }
        self.editors.insert(editor.view_id, editor);
    }

    pub fn add_editor(
        &mut self,
        view_id: WidgetId,
        split_id: Option<WidgetId>,
        buffer_kind: LocalBufferKind,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) {
        let doc = Document::new(
            BufferContent::Local(buffer_kind.clone()),
            *self.tab_id,
            event_sink,
            self.proxy.clone(),
        );
        self.local_docs.insert(buffer_kind.clone(), Arc::new(doc));

        let editor = LapceEditorData::new(
            Some(view_id),
            None,
            split_id,
            BufferContent::Local(buffer_kind),
            config,
        );
        self.editors.insert(editor.view_id, Arc::new(editor));
    }

    pub fn split_close(
        &mut self,
        _ctx: &mut EventCtx,
        split_id: WidgetId,
        from_content: SplitContent,
    ) {
        let split = self.splits.get_mut(&split_id).unwrap();
        let split = Arc::make_mut(split);

        let mut index = 0;
        for (i, content) in split.children.iter().enumerate() {
            if content == &from_content {
                index = i;
                break;
            }
        }
        split.children.remove(index);
    }

    pub fn update_split_content_layout_rect(
        &self,
        content: SplitContent,
        rect: Rect,
    ) {
        match content {
            SplitContent::EditorTab(widget_id) => {
                self.update_editor_tab_layout_rect(widget_id, rect);
            }
            SplitContent::Split(split_id) => {
                self.update_split_layout_rect(split_id, rect);
            }
        }
    }

    pub fn update_editor_tab_layout_rect(&self, widget_id: WidgetId, rect: Rect) {
        let editor_tab = self.editor_tabs.get(&widget_id).unwrap();
        *editor_tab.layout_rect.borrow_mut() = rect;
    }

    pub fn update_split_layout_rect(&self, split_id: WidgetId, rect: Rect) {
        let split = self.splits.get(&split_id).unwrap();
        *split.layout_rect.borrow_mut() = rect;
    }

    pub fn save_as_success(
        &mut self,
        ctx: &mut EventCtx,
        content: &BufferContent,
        rev: u64,
        path: &Path,
        view_id: WidgetId,
        exit: bool,
    ) {
        match content {
            BufferContent::Scratch(id, scratch_doc_name) => {
                let doc = self.scratch_docs.get(id).unwrap();
                if doc.rev() == rev {
                    let new_content = BufferContent::File(path.to_path_buf());
                    for (_, editor) in self.editors.iter_mut() {
                        if editor.content
                            == BufferContent::Scratch(
                                *id,
                                scratch_doc_name.to_string(),
                            )
                        {
                            Arc::make_mut(editor).content = new_content.clone();
                        }
                    }

                    let mut doc = self.scratch_docs.remove(id).unwrap();
                    let mut_doc = Arc::make_mut(&mut doc);
                    mut_doc.buffer_mut().set_pristine();
                    mut_doc.set_content(new_content);
                    self.open_docs.insert(path.to_path_buf(), doc);
                    if exit {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Focus(FocusCommand::SplitClose),
                                data: None,
                            },
                            Target::Widget(view_id),
                        ));
                    }
                }
            }
            BufferContent::File(_) => {}
            _ => {}
        }
    }

    pub fn save_as(
        &mut self,
        ctx: &mut EventCtx,
        content: &BufferContent,
        path: &Path,
        view_id: WidgetId,
        exit: bool,
    ) {
        match content {
            BufferContent::Scratch(id, _) => {
                let event_sink = ctx.get_external_handle();
                let doc = self.scratch_docs.get(id).unwrap();
                let rev = doc.rev();
                let path = path.to_path_buf();
                let content = content.clone();
                self.proxy.proxy_rpc.save_buffer_as(
                    doc.id(),
                    path.to_path_buf(),
                    doc.rev(),
                    doc.buffer().to_string(),
                    Box::new(move |result| {
                        if let Ok(_r) = result {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::SaveAsSuccess {
                                    content,
                                    rev,
                                    path,
                                    view_id,
                                    exit,
                                },
                                Target::Auto,
                            );
                        }
                    }),
                );
            }
            BufferContent::File(_) => {}
            _ => {}
        }
    }

    pub fn widget_close(
        &mut self,
        ctx: &mut EventCtx,
        widget_id: WidgetId,
        editor_tab_id: WidgetId,
    ) {
        let editor_tab = self.editor_tabs.get(&editor_tab_id).unwrap();
        let index = editor_tab
            .children
            .iter()
            .position(|child| child.widget_id() == widget_id)
            .unwrap_or(0);
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EditorTabRemove(index, true, true),
            Target::Widget(editor_tab_id),
        ));
    }

    pub fn editor_close(
        &mut self,
        ctx: &mut EventCtx,
        view_id: WidgetId,
        force: bool,
    ) {
        let editor = self.editors.get(&view_id).unwrap();
        if let BufferContent::File(_) | BufferContent::Scratch(..) = &editor.content
        {
            let doc = self.editor_doc(view_id);
            if !force && !doc.buffer().is_pristine() {
                let exits = self.editors.iter().any(|(_, e)| {
                    &e.content == doc.content() && e.view_id != view_id
                });
                if !exits {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::ShowAlert(AlertContentData {
                            title: format!(
                                "Do you want to save the changes you made to {}?",
                                doc.content().file_name()
                            ),
                            msg: "Your changes will be lost if you don't save them."
                                .to_string(),
                            buttons: vec![
                                (
                                    "Save".to_string(),
                                    view_id,
                                    LapceCommand {
                                        kind: CommandKind::Focus(
                                            FocusCommand::SaveAndExit,
                                        ),
                                        data: None,
                                    },
                                ),
                                (
                                    "Don't Save".to_string(),
                                    view_id,
                                    LapceCommand {
                                        kind: CommandKind::Focus(
                                            FocusCommand::ForceExit,
                                        ),
                                        data: None,
                                    },
                                ),
                            ],
                        }),
                        Target::Widget(*self.tab_id),
                    ));
                    return;
                }
            }
            self.db.save_doc_position(&self.workspace, &doc);
        }
        if let Some(tab_id) = editor.tab_id {
            let editor_tab = self.editor_tabs.get(&tab_id).unwrap();
            let mut index = 0;
            for (i, child) in editor_tab.children.iter().enumerate() {
                if child.widget_id() == view_id {
                    index = i;
                }
            }
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::EditorTabRemove(index, true, true),
                Target::Widget(tab_id),
            ));
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn split(
        &mut self,
        ctx: &mut EventCtx,
        split_id: WidgetId,
        from_content: SplitContent,
        new_content: SplitContent,
        direction: SplitDirection,
        shift_current: bool,
        focus_new: bool,
    ) -> WidgetId {
        let split = self.splits.get_mut(&split_id).unwrap();
        let split = Arc::make_mut(split);

        let mut index = 0;
        for (i, content) in split.children.iter().enumerate() {
            if content == &from_content {
                index = i;
                break;
            }
        }

        if direction != split.direction && split.children.len() == 1 {
            split.direction = direction;
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitChangeDirection(direction),
                Target::Widget(split_id),
            ));
        }

        if direction == split.direction {
            let new_index = if shift_current { index } else { index + 1 };
            split.children.insert(new_index, new_content);
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitAdd(new_index, new_content, false),
                Target::Widget(split_id),
            ));
            split_id
        } else {
            let children = if shift_current {
                vec![new_content, from_content]
            } else {
                vec![from_content, new_content]
            };
            let new_split = SplitData {
                parent_split: Some(split.widget_id),
                widget_id: WidgetId::next(),
                children,
                direction,
                layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
            };
            let new_split_id = new_split.widget_id;
            split.children[index] = SplitContent::Split(new_split.widget_id);
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitReplace(
                    index,
                    SplitContent::Split(new_split.widget_id),
                ),
                Target::Widget(split_id),
            ));
            if focus_new {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(new_content.widget_id()),
                ));
            } else {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(from_content.widget_id()),
                ));
            }
            self.splits.insert(new_split.widget_id, Arc::new(new_split));
            new_split_id
        }
    }

    pub fn split_exchange(&mut self, ctx: &mut EventCtx, content: SplitContent) {
        if let Some(split_id) = content.split_id(self) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitExchange(content),
                Target::Widget(split_id),
            ));
        }
    }

    pub fn split_move(
        &mut self,
        ctx: &mut EventCtx,
        content: SplitContent,
        direction: SplitMoveDirection,
    ) {
        match content {
            SplitContent::EditorTab(widget_id) => {
                let editor_tab = self.editor_tabs.get(&widget_id).unwrap();
                let rect = editor_tab.layout_rect.borrow();
                match direction {
                    SplitMoveDirection::Up => {
                        for (_, e) in self.editor_tabs.iter() {
                            let current_rect = e.layout_rect.borrow();
                            if current_rect.y1 == rect.y0
                                && current_rect.x0 <= rect.x0
                                && rect.x0 < current_rect.x1
                            {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(e.children[e.active].widget_id()),
                                ));
                                return;
                            }
                        }
                    }
                    SplitMoveDirection::Down => {
                        for (_, e) in self.editor_tabs.iter() {
                            let current_rect = e.layout_rect.borrow();
                            if current_rect.y0 == rect.y1
                                && current_rect.x0 <= rect.x0
                                && rect.x0 < current_rect.x1
                            {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(e.children[e.active].widget_id()),
                                ));
                                return;
                            }
                        }
                    }
                    SplitMoveDirection::Right => {
                        for (_, e) in self.editor_tabs.iter() {
                            let current_rect = e.layout_rect.borrow();
                            if current_rect.x0 == rect.x1
                                && current_rect.y0 <= rect.y0
                                && rect.y0 < current_rect.y1
                            {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(e.children[e.active].widget_id()),
                                ));
                                return;
                            }
                        }
                    }
                    SplitMoveDirection::Left => {
                        for (_, e) in self.editor_tabs.iter() {
                            let current_rect = e.layout_rect.borrow();
                            if current_rect.x1 == rect.x0
                                && current_rect.y0 <= rect.y0
                                && rect.y0 < current_rect.y1
                            {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::Focus,
                                    Target::Widget(e.children[e.active].widget_id()),
                                ));
                                return;
                            }
                        }
                    }
                }
            }
            SplitContent::Split(_) => {}
        }
    }

    fn split_plugin(
        &mut self,
        ctx: &mut EventCtx,
        editor_tab_id: WidgetId,
        volt_id: VoltID,
        volt_name: String,
        direction: SplitDirection,
        _config: &LapceConfig,
    ) {
        let editor_tab = self.editor_tabs.get(&editor_tab_id).unwrap();
        let split_id = editor_tab.split;

        let new_editor_tab_id = WidgetId::next();
        let mut new_editor_tab = LapceEditorTabData {
            widget_id: new_editor_tab_id,
            split: split_id,
            active: 0,
            children: vec![EditorTabChild::Plugin {
                widget_id: WidgetId::next(),
                editor_tab_id: new_editor_tab_id,
                volt_id,
                volt_name,
            }]
            .into(),
            layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
            content_is_hot: Rc::new(RefCell::new(false)),
        };

        let new_split_id = self.split(
            ctx,
            split_id,
            SplitContent::EditorTab(editor_tab_id),
            SplitContent::EditorTab(new_editor_tab.widget_id),
            direction,
            false,
            false,
        );

        new_editor_tab.split = new_split_id;
        if split_id != new_split_id {
            let editor_tab = self.editor_tabs.get_mut(&editor_tab_id).unwrap();
            let editor_tab = Arc::make_mut(editor_tab);
            editor_tab.split = new_split_id;
        }
        self.editor_tabs
            .insert(new_editor_tab.widget_id, Arc::new(new_editor_tab));
    }

    pub fn split_settings(
        &mut self,
        ctx: &mut EventCtx,
        editor_tab_id: WidgetId,
        direction: SplitDirection,
        config: &LapceConfig,
    ) {
        let editor_tab = self.editor_tabs.get(&editor_tab_id).unwrap();
        let split_id = editor_tab.split;

        let editor = LapceEditorData::new(
            None,
            None,
            None,
            BufferContent::Local(LocalBufferKind::Keymap),
            config,
        );
        let keymap_input_view_id = editor.view_id;
        self.editors.insert(editor.view_id, Arc::new(editor));

        let new_editor_tab_id = WidgetId::next();
        let mut new_editor_tab = LapceEditorTabData {
            widget_id: new_editor_tab_id,
            split: split_id,
            active: 0,
            children: vec![EditorTabChild::Settings {
                settings_widget_id: WidgetId::next(),
                editor_tab_id: new_editor_tab_id,
                keymap_input_view_id,
            }]
            .into(),
            layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
            content_is_hot: Rc::new(RefCell::new(false)),
        };

        let new_split_id = self.split(
            ctx,
            split_id,
            SplitContent::EditorTab(editor_tab_id),
            SplitContent::EditorTab(new_editor_tab.widget_id),
            direction,
            false,
            false,
        );

        new_editor_tab.split = new_split_id;
        if split_id != new_split_id {
            let editor_tab = self.editor_tabs.get_mut(&editor_tab_id).unwrap();
            let editor_tab = Arc::make_mut(editor_tab);
            editor_tab.split = new_split_id;
        }
        self.editor_tabs
            .insert(new_editor_tab.widget_id, Arc::new(new_editor_tab));
    }

    pub fn tab_split(
        &mut self,
        ctx: &mut EventCtx,
        editor_tab_id: WidgetId,
        direction: SplitDirection,
        config: &LapceConfig,
    ) {
        let editor_tab = self.editor_tabs.get(&editor_tab_id).unwrap();
        if let Some(active) = editor_tab.children.get(editor_tab.active) {
            match active {
                EditorTabChild::Editor(view_id, _, _) => {
                    self.split_editor(ctx, *view_id, direction, config);
                }
                EditorTabChild::Settings { editor_tab_id, .. } => {
                    self.split_settings(ctx, *editor_tab_id, direction, config);
                }
                EditorTabChild::Plugin {
                    editor_tab_id,
                    volt_id,
                    volt_name,
                    ..
                } => {
                    self.split_plugin(
                        ctx,
                        *editor_tab_id,
                        volt_id.clone(),
                        volt_name.clone(),
                        direction,
                        config,
                    );
                }
            }
        }
    }

    pub fn split_editor(
        &mut self,
        ctx: &mut EventCtx,
        view_id: WidgetId,
        direction: SplitDirection,
        config: &LapceConfig,
    ) {
        let editor = self.editors.get(&view_id).unwrap();
        if let Some(editor_tab_id) = editor.tab_id {
            let editor_tab = self.editor_tabs.get(&editor_tab_id).unwrap();
            let split_id = editor_tab.split;
            let mut new_editor = editor.copy();
            let mut new_editor_tab = LapceEditorTabData {
                widget_id: WidgetId::next(),
                split: split_id,
                active: 0,
                children: vec![EditorTabChild::Editor(
                    new_editor.view_id,
                    new_editor.editor_id,
                    new_editor.find_view_id,
                )]
                .into(),
                layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
                content_is_hot: Rc::new(RefCell::new(false)),
            };
            new_editor.tab_id = Some(new_editor_tab.widget_id);

            let new_split_id = self.split(
                ctx,
                split_id,
                SplitContent::EditorTab(editor_tab_id),
                SplitContent::EditorTab(new_editor_tab.widget_id),
                direction,
                false,
                false,
            );

            new_editor_tab.split = new_split_id;
            if split_id != new_split_id {
                let editor_tab = self.editor_tabs.get_mut(&editor_tab_id).unwrap();
                let editor_tab = Arc::make_mut(editor_tab);
                editor_tab.split = new_split_id;
            }

            self.insert_editor(Arc::new(new_editor), config);
            self.editor_tabs
                .insert(new_editor_tab.widget_id, Arc::new(new_editor_tab));
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum EditorContent {
    File(PathBuf),
    None,
}

#[derive(Clone, Debug)]
pub enum InlineFindDirection {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorTabChild {
    Editor(WidgetId, WidgetId, Option<(WidgetId, WidgetId)>),
    Settings {
        settings_widget_id: WidgetId,
        editor_tab_id: WidgetId,
        keymap_input_view_id: WidgetId,
    },
    Plugin {
        widget_id: WidgetId,
        volt_id: VoltID,
        volt_name: String,
        editor_tab_id: WidgetId,
    },
}

impl EditorTabChild {
    pub fn widget_id(&self) -> WidgetId {
        match &self {
            EditorTabChild::Editor(widget_id, _, _) => *widget_id,
            EditorTabChild::Settings {
                settings_widget_id, ..
            } => *settings_widget_id,
            EditorTabChild::Plugin { widget_id, .. } => *widget_id,
        }
    }

    pub fn child_info(&self, data: &LapceTabData) -> EditorTabChildInfo {
        match &self {
            EditorTabChild::Editor(view_id, _, _) => {
                let editor_data = data.main_split.editors.get(view_id).unwrap();
                EditorTabChildInfo::Editor(editor_data.editor_info(data))
            }
            EditorTabChild::Settings { .. } => EditorTabChildInfo::Settings,
            EditorTabChild::Plugin {
                volt_id, volt_name, ..
            } => EditorTabChildInfo::Plugin {
                volt_id: volt_id.clone(),
                volt_name: volt_name.to_string(),
            },
        }
    }

    pub fn set_editor_tab(
        &mut self,
        data: &mut LapceTabData,
        editor_tab_widget_id: WidgetId,
    ) {
        match self {
            EditorTabChild::Editor(view_id, _, _) => {
                let editor_data = data.main_split.editors.get_mut(view_id).unwrap();
                let editor_data = Arc::make_mut(editor_data);
                editor_data.tab_id = Some(editor_tab_widget_id);
            }
            EditorTabChild::Settings { editor_tab_id, .. } => {
                *editor_tab_id = editor_tab_widget_id;
            }
            EditorTabChild::Plugin { editor_tab_id, .. } => {
                *editor_tab_id = editor_tab_widget_id;
            }
        }
    }
}

/// The actual Editor tab structure, holding the windows.
#[derive(Clone, Debug)]
pub struct LapceEditorTabData {
    pub widget_id: WidgetId,
    pub split: WidgetId,
    pub active: usize,
    pub children: Vector<EditorTabChild>,
    pub layout_rect: Rc<RefCell<Rect>>,
    pub content_is_hot: Rc<RefCell<bool>>,
}

impl LapceEditorTabData {
    pub fn tab_info(&self, data: &LapceTabData) -> EditorTabInfo {
        let info = EditorTabInfo {
            active: self.active,
            is_focus: *data.main_split.active_tab == Some(self.widget_id),
            children: self
                .children
                .iter()
                .map(|child| child.child_info(data))
                .collect(),
        };
        info
    }

    pub fn active_child(&self) -> Option<&EditorTabChild> {
        self.children.get(self.active)
    }
}

#[derive(Clone, Debug)]
pub struct SelectionHistory {
    pub rev: u64,
    pub content: BufferContent,
    pub selections: im::Vector<Selection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorView {
    Normal,
    /// Source Control Diffing
    Diff(String),
    /// Code Lens
    Lens,
}

impl EditorView {
    pub fn is_normal(&self) -> bool {
        matches!(self, EditorView::Normal)
    }
}

#[derive(Debug)]
pub struct StickyHeaderInfo {
    pub height: f64,
    pub lines: Vec<usize>,
    pub last_y_diff: f64,
}

#[derive(Clone, Debug)]
pub struct LapceEditorData {
    pub tab_id: Option<WidgetId>,
    pub view_id: WidgetId,
    pub editor_id: WidgetId,
    pub parent_view_id: Option<WidgetId>,
    pub find_view_id: Option<(WidgetId, WidgetId)>,
    pub content: BufferContent,
    pub view: EditorView,
    pub compare: Option<String>,
    pub scroll_offset: Vec2,
    pub cursor: Cursor,
    pub last_cursor_instant: Rc<RefCell<Instant>>,
    pub size: Rc<RefCell<Size>>,
    pub sticky_header: Rc<RefCell<StickyHeaderInfo>>,
    pub window_origin: Rc<RefCell<Point>>,
    pub snippet: Option<Vec<(usize, (usize, usize))>>,
    pub last_movement_new: Movement,
    pub last_inline_find: Option<(InlineFindDirection, String)>,
    pub inline_find: Option<InlineFindDirection>,
    pub motion_mode: Option<MotionMode>,
}

impl LapceEditorData {
    pub fn new(
        view_id: Option<WidgetId>,
        editor_id: Option<WidgetId>,
        tab_id: Option<WidgetId>,
        content: BufferContent,
        config: &LapceConfig,
    ) -> Self {
        Self {
            tab_id,
            view_id: view_id.unwrap_or_else(WidgetId::next),
            editor_id: editor_id.unwrap_or_else(WidgetId::next),
            view: EditorView::Normal,
            parent_view_id: None,
            find_view_id: if content.is_special() {
                None
            } else {
                Some((WidgetId::next(), WidgetId::next()))
            },
            scroll_offset: Vec2::ZERO,
            cursor: if content.is_input() {
                Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None)
            } else if config.core.modal {
                Cursor::new(CursorMode::Normal(0), None, None)
            } else {
                Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None)
            },
            last_cursor_instant: Rc::new(RefCell::new(Instant::now())),
            content,
            size: Rc::new(RefCell::new(Size::ZERO)),
            sticky_header: Rc::new(RefCell::new(StickyHeaderInfo {
                height: 0.0,
                lines: Vec::new(),
                last_y_diff: 0.0,
            })),
            compare: None,
            window_origin: Rc::new(RefCell::new(Point::ZERO)),
            snippet: None,
            last_movement_new: Movement::Left,
            inline_find: None,
            last_inline_find: None,
            motion_mode: None,
        }
    }

    pub fn copy(&self) -> LapceEditorData {
        let mut new_editor = self.clone();
        new_editor.view_id = WidgetId::next();
        new_editor.editor_id = WidgetId::next();
        new_editor.find_view_id = new_editor
            .find_view_id
            .map(|_| (WidgetId::next(), WidgetId::next()));
        new_editor.size = Rc::new(RefCell::new(Size::ZERO));
        new_editor.window_origin = Rc::new(RefCell::new(Point::ZERO));
        new_editor
    }

    pub fn is_code_lens(&self) -> bool {
        matches!(self.view, EditorView::Lens)
    }

    pub fn add_snippet_placeholders(
        &mut self,
        new_placeholders: Vec<(usize, (usize, usize))>,
    ) {
        if self.snippet.is_none() {
            if new_placeholders.len() > 1 {
                self.snippet = Some(new_placeholders);
            }
            return;
        }

        let placeholders = self.snippet.as_mut().unwrap();

        let mut current = 0;
        let offset = self.cursor.offset();
        for (i, (_, (start, end))) in placeholders.iter().enumerate() {
            if *start <= offset && offset <= *end {
                current = i;
                break;
            }
        }

        let v = placeholders.split_off(current);
        placeholders.extend_from_slice(&new_placeholders);
        placeholders.extend_from_slice(&v[1..]);
    }

    pub fn editor_info(&self, data: &LapceTabData) -> EditorInfo {
        let unsaved = if let BufferContent::Scratch(id, _) = &self.content {
            let doc = data.main_split.scratch_docs.get(id).unwrap();
            Some(doc.buffer().to_string())
        } else {
            None
        };

        EditorInfo {
            content: self.content.clone(),
            unsaved,
            scroll_offset: (self.scroll_offset.x, self.scroll_offset.y),
            position: if let BufferContent::File(_) = &self.content {
                Some(self.cursor.offset())
            } else {
                None
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct SshHost {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<usize>,
}

impl SshHost {
    pub fn from_string(s: &str) -> Self {
        let mut whole_splits = s.split(':');
        let splits = whole_splits
            .next()
            .unwrap()
            .split('@')
            .collect::<Vec<&str>>();
        let mut splits = splits.iter().rev();
        let host = splits.next().unwrap().to_string();
        let user = splits.next().map(|s| s.to_string());
        let port = whole_splits.next().and_then(|s| s.parse::<usize>().ok());
        Self { user, host, port }
    }

    pub fn user_host(&self) -> String {
        if let Some(user) = self.user.as_ref() {
            format!("{user}@{}", self.host)
        } else {
            self.host.clone()
        }
    }
}

impl Display for SshHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(user) = self.user.as_ref() {
            write!(f, "{user}@")?;
        }
        write!(f, "{}", self.host)?;
        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LapceWorkspaceType {
    Local,
    RemoteSSH(SshHost),
    #[cfg(windows)]
    RemoteWSL,
}

impl LapceWorkspaceType {
    #[cfg(windows)]
    pub fn is_remote(&self) -> bool {
        matches!(
            self,
            LapceWorkspaceType::RemoteSSH(_) | LapceWorkspaceType::RemoteWSL
        )
    }

    #[cfg(not(windows))]
    pub fn is_remote(&self) -> bool {
        matches!(self, LapceWorkspaceType::RemoteSSH(_))
    }
}

impl std::fmt::Display for LapceWorkspaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LapceWorkspaceType::Local => f.write_str("Local"),
            LapceWorkspaceType::RemoteSSH(ssh) => {
                write!(f, "ssh://{ssh}")
            }
            #[cfg(windows)]
            LapceWorkspaceType::RemoteWSL => f.write_str("WSL"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LapceWorkspace {
    pub kind: LapceWorkspaceType,
    pub path: Option<PathBuf>,
    pub last_open: u64,
}

impl Default for LapceWorkspace {
    fn default() -> Self {
        Self {
            kind: LapceWorkspaceType::Local,
            path: None,
            last_open: 0,
        }
    }
}

impl std::fmt::Display for LapceWorkspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.kind,
            self.path.as_ref().and_then(|p| p.to_str()).unwrap_or("")
        )
    }
}
