use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
    process::{self, Stdio},
    rc::Rc,
    str::FromStr,
    sync::Arc,
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender, TryRecvError};
use crossbeam_utils::sync::WaitGroup;
use directories::{ProjectDirs, UserDirs};
use druid::{
    piet::{PietText, Text},
    theme,
    widget::{Label, LabelText},
    Application, Color, Command, Data, Env, EventCtx, ExtEventSink, FontDescriptor,
    FontFamily, Insets, KeyEvent, Lens, LocalizedString, Menu, MenuItem, Point,
    Rect, Size, Target, TextLayout, Vec2, WidgetId, WindowId,
};
use im::{self, hashmap};
use itertools::Itertools;
use lapce_proxy::terminal::TermId;
use lsp_types::{
    CodeActionOrCommand, CodeActionResponse, CompletionItem, CompletionResponse,
    CompletionTextEdit, Diagnostic, DiagnosticSeverity, GotoDefinitionResponse,
    Location, Position, ProgressToken, TextEdit, WorkspaceClientCapabilities,
};
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use tree_sitter::{Node, Parser};
use tree_sitter_highlight::{
    Highlight, HighlightConfiguration, HighlightEvent, Highlighter,
};
use xi_core_lib::{
    selection::InsertDrift,
    watcher::{FileWatcher, Notify, WatchToken},
};
use xi_rope::{
    spans::SpansBuilder, DeltaBuilder, Interval, Rope, RopeDelta, Transformer,
};
use xi_rpc::{RpcLoop, RpcPeer};

use crate::{
    buffer::{
        get_word_property, has_unmatched_pair, matching_char,
        matching_pair_direction, previous_has_unmatched_pair, BufferId, BufferNew,
        BufferState, BufferUpdate, EditType, Style, UpdateEvent, WordProperty,
    },
    command::{
        CommandTarget, EnsureVisiblePosition, LapceCommand, LapceCommandNew,
        LapceUICommand, LapceWorkbenchCommand, LAPCE_COMMAND, LAPCE_NEW_COMMAND,
        LAPCE_UI_COMMAND,
    },
    completion::{CompletionData, CompletionStatus, Snippet},
    config::{Config, LapceTheme},
    db::{LapceDb, WorkspaceInfo},
    editor::{EditorLocationNew, LapceEditorBufferData, LapceEditorViewContent},
    explorer::FileExplorerData,
    find::Find,
    keypress::{KeyPressData, KeyPressFocus},
    language::{new_highlight_config, new_parser, LapceLanguage},
    movement::{Cursor, CursorMode, LinePosition, Movement, SelRegion, Selection},
    palette::{PaletteData, PaletteType, PaletteViewData},
    panel::PanelPosition,
    proxy::{LapceProxy, ProxyHandlerNew, TermEvent},
    source_control::{SourceControlData, SOURCE_CONTROL_BUFFER},
    state::{LapceWorkspace, LapceWorkspaceType, Mode, VisualMode},
    terminal::TerminalSplitData,
};

#[derive(Clone, Data)]
pub struct LapceData {
    pub windows: im::HashMap<WindowId, LapceWindowData>,
    pub keypress: Arc<KeyPressData>,
}

impl LapceData {
    pub fn load(event_sink: ExtEventSink) -> Self {
        let mut windows = im::HashMap::new();
        let keypress = Arc::new(KeyPressData::new());
        let window = LapceWindowData::new(keypress.clone(), event_sink.clone());
        windows.insert(WindowId::next(), window);
        Self { windows, keypress }
    }

    pub fn reload_env(&self, env: &mut Env) {}
}

#[derive(Clone)]
pub struct LapceWindowData {
    pub tabs: im::HashMap<WidgetId, LapceTabData>,
    pub tabs_order: Arc<Vec<WidgetId>>,
    pub active: usize,
    pub active_id: WidgetId,
    pub keypress: Arc<KeyPressData>,
    pub config: Arc<Config>,
    pub db: Arc<LapceDb>,
}

impl Data for LapceWindowData {
    fn same(&self, other: &Self) -> bool {
        self.active == other.active && self.tabs.same(&other.tabs)
    }
}

impl LapceWindowData {
    pub fn new(keypress: Arc<KeyPressData>, event_sink: ExtEventSink) -> Self {
        let db = Arc::new(LapceDb::new().unwrap());
        let mut tabs = im::HashMap::new();
        let mut tabs_order = Vec::new();
        let mut active_tab_id = WidgetId::next();
        let mut active = 0;

        if let Ok(info) = db.get_tabs_info() {
            for (i, workspace) in info.workspaces.iter().enumerate() {
                let tab_id = WidgetId::next();
                let tab = LapceTabData::new(
                    tab_id,
                    workspace.clone(),
                    db.clone(),
                    keypress.clone(),
                    event_sink.clone(),
                );
                tabs.insert(tab_id, tab);
                tabs_order.push(tab_id);
                if i == info.active_tab {
                    active_tab_id = tab_id;
                    active = i;
                }
            }
        }

        if tabs.len() == 0 {
            let tab_id = WidgetId::next();
            let tab = LapceTabData::new(
                tab_id,
                None,
                db.clone(),
                keypress.clone(),
                event_sink,
            );
            tabs.insert(tab_id, tab);
            tabs_order.push(tab_id);
            active_tab_id = tab_id;
        }

        let config = Arc::new(Config::load(None).unwrap_or_default());
        Self {
            tabs,
            tabs_order: Arc::new(tabs_order),
            active,
            active_id: active_tab_id,
            keypress,
            config,
            db,
        }
    }
}

#[derive(Clone)]
pub struct EditorDiagnostic {
    pub range: Option<(usize, usize)>,
    pub diagnositc: Diagnostic,
}

#[derive(Clone)]
pub enum PanelKind {
    FileExplorer,
    SourceControl,
    Terminal,
}

#[derive(Clone)]
pub struct PanelData {
    pub active: WidgetId,
    pub widgets: Vec<(WidgetId, PanelKind)>,
    pub shown: bool,
    pub maximized: bool,
}

impl PanelData {
    pub fn is_shown(&self) -> bool {
        self.shown && self.widgets.len() > 0
    }

    pub fn is_maximized(&self) -> bool {
        self.maximized && self.widgets.len() > 0
    }
}

#[derive(Clone, Data)]
pub struct PanelSize {
    pub left: f64,
    pub left_split: f64,
    pub bottom: f64,
    pub bottom_split: f64,
    pub right: f64,
    pub right_split: f64,
}

struct ConfigNotify {
    event_sink: ExtEventSink,
}

impl Notify for ConfigNotify {
    fn notify(&self) {
        println!("receive config file watcher notify");
        self.event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::ReloadConfig,
            Target::Auto,
        );
    }
}

pub fn watch_settings(event_sink: ExtEventSink) {
    thread::spawn(move || {
        if let Some(proj_dirs) = ProjectDirs::from("", "", "Lapce") {
            let mut watcher = FileWatcher::new(ConfigNotify { event_sink });
            let path = proj_dirs.config_dir().join("settings.toml");
            println!("start to watch path {:?}", path);
            watcher.watch(&path, false, WatchToken(0));
            loop {
                thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });
}

#[derive(Clone)]
pub struct WorkProgress {
    pub token: ProgressToken,
    pub title: String,
    pub message: Option<String>,
    pub percentage: Option<u32>,
}

#[derive(Clone, PartialEq, Data)]
pub enum FocusArea {
    Palette,
    SourceControl,
    Editor,
    Terminal,
}

#[derive(Clone, Lens)]
pub struct LapceTabData {
    pub id: WidgetId,
    pub workspace: Option<Arc<LapceWorkspace>>,
    pub main_split: LapceMainSplitData,
    pub completion: Arc<CompletionData>,
    pub terminal: Arc<TerminalSplitData>,
    pub palette: Arc<PaletteData>,
    pub find: Arc<Find>,
    pub source_control: Arc<SourceControlData>,
    pub file_explorer: Arc<FileExplorerData>,
    pub proxy: Arc<LapceProxy>,
    pub keypress: Arc<KeyPressData>,
    pub update_receiver: Option<Receiver<UpdateEvent>>,
    pub term_tx: Arc<Sender<(TermId, TermEvent)>>,
    pub term_rx: Option<Receiver<(TermId, TermEvent)>>,
    pub update_sender: Arc<Sender<UpdateEvent>>,
    pub window_origin: Point,
    pub panels: im::HashMap<PanelPosition, Arc<PanelData>>,
    pub panel_active: PanelPosition,
    pub panel_size: PanelSize,
    pub config: Arc<Config>,
    pub focus: WidgetId,
    pub focus_area: FocusArea,
    pub db: Arc<LapceDb>,
    pub progresses: im::Vector<WorkProgress>,
}

impl Data for LapceTabData {
    fn same(&self, other: &Self) -> bool {
        self.main_split.same(&other.main_split)
            && self.completion.same(&other.completion)
            && self.palette.same(&other.palette)
            && self.workspace.same(&other.workspace)
            && self.source_control.same(&other.source_control)
            && self.panels.same(&other.panels)
            && self.panel_size.same(&other.panel_size)
            && self.window_origin.same(&other.window_origin)
            && self.config.same(&other.config)
            && self.terminal.same(&other.terminal)
            && self.focus == other.focus
            && self.focus_area == other.focus_area
            && self.panel_active == other.panel_active
            && self.find.same(&other.find)
            && self.progresses.ptr_eq(&other.progresses)
            && self.file_explorer.same(&other.file_explorer)
    }
}

impl LapceTabData {
    pub fn new(
        tab_id: WidgetId,
        workspace: Option<LapceWorkspace>,
        db: Arc<LapceDb>,
        keypress: Arc<KeyPressData>,
        event_sink: ExtEventSink,
    ) -> Self {
        let config = Arc::new(Config::load(None).unwrap_or_default());

        let workspace_info = workspace
            .as_ref()
            .and_then(|w| db.get_workspace_info(w).ok());

        let (update_sender, update_receiver) = unbounded();
        let update_sender = Arc::new(update_sender);
        let (term_sender, term_receiver) = unbounded();
        let proxy = Arc::new(LapceProxy::new(tab_id, term_sender.clone()));
        let palette = Arc::new(PaletteData::new(proxy.clone()));
        let completion = Arc::new(CompletionData::new());
        let source_control = Arc::new(SourceControlData::new());
        let file_explorer = Arc::new(FileExplorerData::new(
            tab_id,
            workspace.clone(),
            proxy.clone(),
            event_sink.clone(),
        ));
        let mut main_split = LapceMainSplitData::new(
            tab_id,
            workspace_info.as_ref(),
            palette.preview_editor,
            update_sender.clone(),
            proxy.clone(),
            &config,
            event_sink.clone(),
        );
        main_split.add_source_control_editor(
            source_control.editor_view_id,
            source_control.split_id,
            &config,
        );

        let terminal = Arc::new(TerminalSplitData::new(proxy.clone()));

        let mut panels = im::HashMap::new();
        panels.insert(
            PanelPosition::LeftTop,
            Arc::new(PanelData {
                active: file_explorer.widget_id,
                widgets: vec![
                    (file_explorer.widget_id, PanelKind::FileExplorer),
                    (source_control.widget_id, PanelKind::SourceControl),
                ],
                shown: true,
                maximized: false,
            }),
        );
        panels.insert(
            PanelPosition::BottomLeft,
            Arc::new(PanelData {
                active: terminal.widget_id,
                widgets: vec![(terminal.widget_id, PanelKind::Terminal)],
                shown: true,
                maximized: false,
            }),
        );
        let mut tab = Self {
            id: tab_id,
            workspace: workspace.map(|w| Arc::new(w)),
            focus: *main_split.active,
            main_split,
            completion,
            terminal,
            find: Arc::new(Find::new(0)),
            source_control,
            file_explorer,
            term_rx: Some(term_receiver),
            term_tx: Arc::new(term_sender),
            palette,
            proxy,
            keypress,
            update_sender,
            update_receiver: Some(update_receiver),
            window_origin: Point::ZERO,
            panels,
            panel_size: PanelSize {
                left: 250.0,
                left_split: 0.5,
                bottom: 300.0,
                bottom_split: 0.5,
                right: 250.0,
                right_split: 0.5,
            },
            panel_active: PanelPosition::LeftTop,
            config,
            focus_area: FocusArea::Editor,
            db,
            progresses: im::Vector::new(),
        };
        tab.start_update_process(event_sink);
        tab
    }

    pub fn start_update_process(&mut self, event_sink: ExtEventSink) {
        if let Some(receiver) = self.update_receiver.take() {
            let tab_id = self.id;
            let local_event_sink = event_sink.clone();
            thread::spawn(move || {
                LapceTabData::buffer_update_process(
                    tab_id,
                    receiver,
                    local_event_sink,
                );
                println!("buffer update process stopped");
            });
        }

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
                println!("terminal update process stopped");
            });
        }

        if let Some(receiver) = Arc::make_mut(&mut self.palette).receiver.take() {
            let widget_id = self.palette.widget_id;
            thread::spawn(move || {
                PaletteViewData::update_process(receiver, widget_id, event_sink);
                println!("palette update process stopped");
            });
        }
    }

    pub fn editor_view_content(
        &self,
        editor_view_id: WidgetId,
    ) -> LapceEditorViewContent {
        let editor = self.main_split.editors.get(&editor_view_id).unwrap();
        match &editor.content {
            EditorContent::Buffer(path) => {
                let buffer = self.main_split.open_files.get(path).unwrap().clone();
                LapceEditorViewContent::Buffer(LapceEditorBufferData {
                    view_id: editor_view_id,
                    main_split: self.main_split.clone(),
                    completion: self.completion.clone(),
                    proxy: self.proxy.clone(),
                    find: self.find.clone(),
                    buffer,
                    editor: editor.clone(),
                    config: self.config.clone(),
                    workspace: self.workspace.clone(),
                })
            }
            EditorContent::None => LapceEditorViewContent::None,
        }
    }

    pub fn code_action_size(&self, text: &mut PietText, env: &Env) -> Size {
        let editor = self.main_split.active_editor();
        match &editor.content {
            EditorContent::None => Size::ZERO,
            EditorContent::Buffer(path) => {
                let buffer = self.main_split.open_files.get(path).unwrap();
                let offset = editor.cursor.offset();
                let prev_offset = buffer.prev_code_boundary(offset);
                let empty_vec = Vec::new();
                let code_actions =
                    buffer.code_actions.get(&prev_offset).unwrap_or(&empty_vec);

                let action_text_layouts: Vec<TextLayout<String>> = code_actions
                    .iter()
                    .map(|code_action| {
                        let title = match code_action {
                            CodeActionOrCommand::Command(cmd) => {
                                cmd.title.to_string()
                            }
                            CodeActionOrCommand::CodeAction(action) => {
                                action.title.to_string()
                            }
                        };
                        let mut text_layout =
                            TextLayout::<String>::from_text(title.clone());
                        text_layout.set_font(
                            FontDescriptor::new(FontFamily::SYSTEM_UI)
                                .with_size(14.0),
                        );
                        text_layout.rebuild_if_needed(text, env);
                        text_layout
                    })
                    .collect();

                let mut width = 0.0;
                for text_layout in &action_text_layouts {
                    let line_width = text_layout.size().width + 10.0;
                    if line_width > width {
                        width = line_width;
                    }
                }
                let line_height = self.config.editor.line_height as f64;
                Size::new(width, code_actions.len() as f64 * line_height)
            }
        }
    }

    pub fn update_from_editor_buffer_data(
        &mut self,
        editor_buffer_data: LapceEditorBufferData,
        editor: &Arc<LapceEditorData>,
        buffer: &Arc<BufferNew>,
    ) {
        self.completion = editor_buffer_data.completion.clone();
        self.main_split = editor_buffer_data.main_split.clone();
        self.find = editor_buffer_data.find.clone();
        if !editor_buffer_data.editor.same(editor) {
            self.main_split
                .editors
                .insert(editor.view_id, editor_buffer_data.editor);
        }
        if !editor_buffer_data.buffer.same(&buffer) {
            self.main_split
                .open_files
                .insert(buffer.path.clone(), editor_buffer_data.buffer);
        }
    }

    pub fn code_action_origin(
        &self,
        text: &mut PietText,
        tab_size: Size,
        config: &Config,
    ) -> Point {
        let line_height = self.config.editor.line_height as f64;
        let editor = self.main_split.active_editor();
        match &editor.content {
            EditorContent::None => {
                editor.window_origin - self.window_origin.to_vec2()
            }
            EditorContent::Buffer(path) => {
                let buffer = self.main_split.open_files.get(path).unwrap();
                let offset = editor.cursor.offset();
                let (line, col) = buffer.offset_to_line_col(offset);
                let width = config.editor_text_width(text, "W");
                let x = col as f64 * width;
                let y = (line + 1) as f64 * line_height;
                let origin = editor.window_origin - self.window_origin.to_vec2()
                    + Vec2::new(x, y);
                origin
            }
        }
    }

    pub fn completion_origin(
        &self,
        text: &mut PietText,
        tab_size: Size,
        config: &Config,
    ) -> Point {
        let line_height = self.config.editor.line_height as f64;

        let editor = self.main_split.active_editor();
        match &editor.content {
            EditorContent::None => {
                editor.window_origin - self.window_origin.to_vec2()
            }
            EditorContent::Buffer(path) => {
                let buffer = self.main_split.open_files.get(path).unwrap();
                let offset = self.completion.offset;
                let (line, col) = buffer.offset_to_line_col(offset);
                let width = config.editor_text_width(text, "W");
                let x = col as f64 * width - line_height - 5.0;
                let y = (line + 1) as f64 * line_height;
                let mut origin = editor.window_origin - self.window_origin.to_vec2()
                    + Vec2::new(x, y);
                if origin.y + self.completion.size.height + 1.0 > tab_size.height {
                    let height = self
                        .completion
                        .size
                        .height
                        .min(self.completion.len() as f64 * line_height);
                    origin.y = editor.window_origin.y - self.window_origin.y
                        + line as f64 * line_height
                        - height;
                }
                if origin.x + self.completion.size.width + 1.0 > tab_size.width {
                    origin.x = tab_size.width - self.completion.size.width - 1.0;
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
            focus_area: self.focus_area.clone(),
            terminal: self.terminal.clone(),
        }
    }

    pub fn run_workbench_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceWorkbenchCommand,
        count: Option<usize>,
        env: &Env,
    ) {
        match command {
            LapceWorkbenchCommand::OpenFolder => {
                let event_sink = ctx.get_external_handle();
                thread::spawn(move || {
                    let dir = UserDirs::new()
                        .and_then(|u| u.home_dir().to_str().map(|s| s.to_string()))
                        .unwrap_or(".".to_string());
                    if let Some(folder) =
                        tinyfiledialogs::select_folder_dialog("Open folder", &dir)
                    {
                        let path = PathBuf::from(folder);
                        let workspace = LapceWorkspace {
                            kind: LapceWorkspaceType::Local,
                            path,
                            last_open: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        };

                        event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::SetWorkspace(workspace),
                            Target::Auto,
                        );
                    }
                });
            }
            LapceWorkbenchCommand::EnableModal => {
                let config = Arc::make_mut(&mut self.config);
                config.lapce.modal = true;
                Config::update_file("lapce.modal", toml::Value::Boolean(true));
            }
            LapceWorkbenchCommand::DisableModal => {
                let config = Arc::make_mut(&mut self.config);
                config.lapce.modal = false;
                Config::update_file("lapce.modal", toml::Value::Boolean(false));
            }
            LapceWorkbenchCommand::ChangeTheme => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::Theme)),
                    Target::Widget(self.palette.widget_id),
                ));
            }
            LapceWorkbenchCommand::OpenSettings => {
                if let Some(proj_dirs) = ProjectDirs::from("", "", "Lapce") {
                    std::fs::create_dir_all(proj_dirs.config_dir());
                    let path = proj_dirs.config_dir().join("settings.toml");
                    {
                        std::fs::OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .open(&path);
                    }

                    let editor_view_id = self.main_split.active.clone();
                    self.main_split.jump_to_location(
                        ctx,
                        *editor_view_id,
                        EditorLocationNew {
                            path: path.clone(),
                            position: None,
                            scroll_offset: None,
                        },
                        &self.config,
                    );
                }
            }
            LapceWorkbenchCommand::OpenKeyboardShortcuts => {
                if let Some(proj_dirs) = ProjectDirs::from("", "", "Lapce") {
                    std::fs::create_dir_all(proj_dirs.config_dir());
                    let path = proj_dirs.config_dir().join("keymaps.toml");
                    {
                        std::fs::OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .open(&path);
                    }

                    let editor_view_id = self.main_split.active.clone();
                    self.main_split.jump_to_location(
                        ctx,
                        *editor_view_id,
                        EditorLocationNew {
                            path: path.clone(),
                            position: None,
                            scroll_offset: None,
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
            LapceWorkbenchCommand::NewTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NewTab,
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::CloseTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CloseTab,
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::NextTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NextTab,
                    Target::Auto,
                ));
            }
            LapceWorkbenchCommand::PreviousTab => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PreviousTab,
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
            LapceWorkbenchCommand::ToggleTerminal => {
                if self.focus_area == FocusArea::Terminal {
                    for (_, panel) in self.panels.iter_mut() {
                        if panel
                            .widgets
                            .iter()
                            .map(|(id, kind)| *id)
                            .contains(&self.terminal.widget_id)
                        {
                            let panel = Arc::make_mut(panel);
                            panel.shown = false;
                            break;
                        }
                    }
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(*self.main_split.active),
                    ));
                } else {
                    for (_, panel) in self.panels.iter_mut() {
                        if panel
                            .widgets
                            .iter()
                            .map(|(id, kind)| *id)
                            .contains(&self.terminal.widget_id)
                        {
                            let panel = Arc::make_mut(panel);
                            panel.shown = true;
                            panel.active = self.terminal.widget_id;
                            break;
                        }
                    }
                    if self.terminal.terminals.len() == 0 {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::InitTerminalPanel(true),
                            Target::Widget(self.terminal.split_id),
                        ));
                    } else {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(self.terminal.active),
                        ));
                    }
                }
            }
            LapceWorkbenchCommand::ToggleMaximizedPanel => {
                let panel = self.panels.get_mut(&self.panel_active).unwrap();
                let panel = Arc::make_mut(panel);
                panel.maximized = !panel.maximized;
            }
            LapceWorkbenchCommand::FocusEditor => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(*self.main_split.active),
                ));
            }
            LapceWorkbenchCommand::FocusTerminal => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::Focus,
                    Target::Widget(self.terminal.active),
                ));
            }
        }
    }

    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommandNew,
        count: Option<usize>,
        env: &Env,
    ) {
        match command.target {
            CommandTarget::Workbench => {
                if let Ok(cmd) = LapceWorkbenchCommand::from_str(&command.cmd) {
                    self.run_workbench_command(ctx, &cmd, count, env);
                }
            }
            CommandTarget::Plugin(_) => {}
            CommandTarget::Focus => ctx.submit_command(Command::new(
                LAPCE_NEW_COMMAND,
                command.clone(),
                Target::Widget(self.focus),
            )),
        }
    }

    pub fn terminal_update_process(
        tab_id: WidgetId,
        palette_widget_id: WidgetId,
        receiver: Receiver<(TermId, TermEvent)>,
        event_sink: ExtEventSink,
        workspace: Option<Arc<LapceWorkspace>>,
        proxy: Arc<LapceProxy>,
    ) {
        let mut terminals = HashMap::new();
        let mut last_redraw = std::time::Instant::now();
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
                        raw.lock().update_content(&content);
                        last_event = receiver.try_recv().ok();
                        if last_event.is_some() {
                            if last_redraw.elapsed().as_millis() > 10 {
                                last_redraw = std::time::Instant::now();
                                event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::RequestPaint,
                                    Target::Widget(tab_id),
                                );
                            }
                        } else {
                            last_redraw = std::time::Instant::now();
                            event_sink.submit_command(
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

    pub fn buffer_update_process(
        tab_id: WidgetId,
        receiver: Receiver<UpdateEvent>,
        event_sink: ExtEventSink,
    ) {
        use std::collections::{HashMap, HashSet};
        fn insert_update(
            updates: &mut HashMap<BufferId, UpdateEvent>,
            event: UpdateEvent,
        ) {
            let update = match &event {
                UpdateEvent::Buffer(update) => update,
                UpdateEvent::SemanticTokens(update, tokens) => update,
            };
            if let Some(current) = updates.get(&update.id) {
                let current = match &event {
                    UpdateEvent::Buffer(update) => update,
                    UpdateEvent::SemanticTokens(update, tokens) => update,
                };
                if update.rev > current.rev {
                    updates.insert(update.id, event);
                }
            } else {
                updates.insert(update.id, event);
            }
        }

        fn receive_batch(
            receiver: &Receiver<UpdateEvent>,
        ) -> HashMap<BufferId, UpdateEvent> {
            let mut updates = HashMap::new();
            loop {
                if let Ok(update) = receiver.recv() {
                    insert_update(&mut updates, update);
                }
                match receiver.try_recv() {
                    Ok(update) => {
                        insert_update(&mut updates, update);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            updates
        }

        let mut parsers = HashMap::new();
        let mut highlighter = Highlighter::new();
        let mut highlight_configs = HashMap::new();
        loop {
            let events = receive_batch(&receiver);
            if events.len() == 0 {
                return;
            }
            for (_, event) in events {
                match event {
                    UpdateEvent::Buffer(update) => {
                        buffer_receive_update(
                            update,
                            &mut parsers,
                            &mut highlighter,
                            &mut highlight_configs,
                            &event_sink,
                            tab_id,
                        );
                    }
                    UpdateEvent::SemanticTokens(update, tokens) => {
                        let mut highlights = SpansBuilder::new(update.rope.len());
                        for (start, end, hl) in tokens {
                            highlights.add_span(
                                Interval::new(start, end),
                                Style {
                                    fg_color: Some(hl.to_string()),
                                },
                            );
                        }
                        let highlights = highlights.build();
                        event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateStyle {
                                id: update.id,
                                path: update.path,
                                rev: update.rev,
                                highlights,
                                semantic_tokens: true,
                            },
                            Target::Widget(tab_id),
                        );
                    }
                };
            }
        }
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
        f(&tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceTabData) -> V>(
        &self,
        data: &mut LapceWindowData,
        f: F,
    ) -> V {
        let mut tab = data.tabs.get(&self.0).unwrap().clone();
        tab.keypress = data.keypress.clone();
        let result = f(&mut tab);
        data.keypress = tab.keypress.clone();
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
        f(&tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceWindowData) -> V>(
        &self,
        data: &mut LapceData,
        f: F,
    ) -> V {
        let mut win = data.windows.get(&self.0).unwrap().clone();
        win.keypress = data.keypress.clone();
        let result = f(&mut win);
        data.keypress = win.keypress.clone();
        if !win.same(data.windows.get(&self.0).unwrap()) {
            data.windows.insert(self.0, win);
        }
        result
    }
}

#[derive(Clone, Default)]
pub struct RegisterData {
    pub content: String,
    pub mode: VisualMode,
}

#[derive(Clone, Default)]
pub struct Register {
    pub unamed: RegisterData,
    last_yank: RegisterData,
    last_deletes: [RegisterData; 10],
    newest_delete: usize,
}

impl Register {
    pub fn add_delete(&mut self, data: RegisterData) {
        self.unamed = data.clone();
    }

    pub fn add_yank(&mut self, data: RegisterData) {
        self.unamed = data.clone();
        self.last_yank = data;
    }
}

#[derive(Clone, Debug)]
pub enum EditorKind {
    PalettePreview,
    SplitActive,
}

#[derive(Clone, Data, Lens)]
pub struct LapceMainSplitData {
    pub tab_id: Arc<WidgetId>,
    pub split_id: Arc<WidgetId>,
    pub active: Arc<WidgetId>,
    pub editors: im::HashMap<WidgetId, Arc<LapceEditorData>>,
    pub editors_order: Arc<Vec<WidgetId>>,
    pub open_files: im::HashMap<PathBuf, Arc<BufferNew>>,
    pub update_sender: Arc<Sender<UpdateEvent>>,
    pub register: Arc<Register>,
    pub proxy: Arc<LapceProxy>,
    pub palette_preview_editor: Arc<WidgetId>,
    pub show_code_actions: bool,
    pub current_code_actions: usize,
    pub diagnostics: im::HashMap<PathBuf, Arc<Vec<EditorDiagnostic>>>,
    pub error_count: usize,
    pub warning_count: usize,
}

impl LapceMainSplitData {
    pub fn editor_kind(&self, kind: &EditorKind) -> &LapceEditorData {
        match kind {
            EditorKind::PalettePreview => {
                self.editors.get(&self.palette_preview_editor).unwrap()
            }
            EditorKind::SplitActive => self.editors.get(&self.active).unwrap(),
        }
    }

    pub fn editor_kind_mut(&mut self, kind: &EditorKind) -> &mut LapceEditorData {
        match kind {
            EditorKind::PalettePreview => Arc::make_mut(
                self.editors.get_mut(&self.palette_preview_editor).unwrap(),
            ),
            EditorKind::SplitActive => {
                Arc::make_mut(self.editors.get_mut(&self.active).unwrap())
            }
        }
    }

    pub fn active_editor(&self) -> &LapceEditorData {
        self.editors.get(&self.active).unwrap()
    }

    pub fn active_editor_mut(&mut self) -> &mut LapceEditorData {
        Arc::make_mut(self.editors.get_mut(&self.active).unwrap())
    }

    pub fn document_format_and_save(
        &mut self,
        ctx: &mut EventCtx,
        path: &PathBuf,
        rev: u64,
        result: &Result<Value>,
    ) {
        let buffer = self.open_files.get(path).unwrap();
        if buffer.rev != rev {
            return;
        }

        if let Ok(res) = result {
            let edits: Result<Vec<TextEdit>, serde_json::Error> =
                serde_json::from_value(res.clone());
            if let Ok(edits) = edits {
                if edits.len() > 0 {
                    let buffer = self.open_files.get_mut(path).unwrap();

                    let edits: Vec<(Selection, String)> = edits
                        .iter()
                        .map(|edit| {
                            let selection = Selection::region(
                                buffer.offset_of_position(&edit.range.start),
                                buffer.offset_of_position(&edit.range.end),
                            );
                            (selection, edit.new_text.clone())
                        })
                        .collect();

                    self.edit(
                        ctx,
                        &path,
                        edits.iter().map(|(s, c)| (s, c.as_ref())).collect(),
                        EditType::Other,
                    );
                }
            }
        }

        let buffer = self.open_files.get(path).unwrap();
        let rev = buffer.rev;
        let buffer_id = buffer.id;
        let event_sink = ctx.get_external_handle();
        let path = path.clone();
        self.proxy.save(
            rev,
            buffer_id,
            Box::new(move |result| {
                if let Ok(r) = result {
                    event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::BufferSave(path, rev),
                        Target::Auto,
                    );
                }
            }),
        );
    }

    fn initiate_diagnositcs_offset(&mut self, path: &PathBuf) {
        if let Some(diagnostics) = self.diagnostics.get_mut(path) {
            if let Some(buffer) = self.open_files.get(path) {
                for diagnostic in Arc::make_mut(diagnostics).iter_mut() {
                    if diagnostic.range.is_none() {
                        diagnostic.range = Some((
                            buffer.offset_of_position(
                                &diagnostic.diagnositc.range.start,
                            ),
                            buffer.offset_of_position(
                                &diagnostic.diagnositc.range.end,
                            ),
                        ));
                    }
                }
            }
        }
    }

    fn update_diagnositcs_offset(&mut self, path: &PathBuf, delta: &RopeDelta) {
        if let Some(diagnostics) = self.diagnostics.get_mut(path) {
            if let Some(buffer) = self.open_files.get(path) {
                let mut transformer = Transformer::new(delta);
                for diagnostic in Arc::make_mut(diagnostics).iter_mut() {
                    let (start, end) = diagnostic.range.clone().unwrap();
                    let (new_start, new_end) = (
                        transformer.transform(start, false),
                        transformer.transform(end, true),
                    );
                    diagnostic.range = Some((new_start, new_end));
                    if start != new_start {
                        diagnostic.diagnositc.range.start =
                            buffer.offset_to_position(new_start);
                    }
                    if end != new_end {
                        diagnostic.diagnositc.range.end =
                            buffer.offset_to_position(new_end);
                        buffer.offset_to_position(new_end);
                    }
                }
            }
        }
    }

    fn cursor_apply_delta(&mut self, path: &PathBuf, delta: &RopeDelta) {
        for (view_id, editor) in self.editors.iter_mut() {
            match &editor.content {
                EditorContent::Buffer(current_path) => {
                    if current_path == path {
                        Arc::make_mut(editor).cursor.apply_delta(delta);
                    }
                }
                EditorContent::None => {}
            }
        }
    }

    pub fn edit(
        &mut self,
        ctx: &mut EventCtx,
        path: &PathBuf,
        edits: Vec<(&Selection, &str)>,
        edit_type: EditType,
    ) -> Option<RopeDelta> {
        self.initiate_diagnositcs_offset(path);
        let proxy = self.proxy.clone();
        let buffer = self.open_files.get_mut(path)?;
        let delta =
            Arc::make_mut(buffer).edit_multiple(ctx, edits, proxy, edit_type);
        self.cursor_apply_delta(path, &delta);
        self.update_diagnositcs_offset(path, &delta);
        Some(delta)
    }

    pub fn jump_to_position(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: WidgetId,
        position: Position,
        config: &Config,
    ) {
        let editor = self.editors.get(&editor_view_id).unwrap();
        match &editor.content {
            EditorContent::Buffer(path) => {
                let location = EditorLocationNew {
                    path: path.clone(),
                    position: Some(position),
                    scroll_offset: None,
                };
                self.jump_to_location(ctx, editor_view_id, location, config);
            }
            EditorContent::None => {}
        }
    }

    pub fn jump_to_location(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: WidgetId,
        location: EditorLocationNew,
        config: &Config,
    ) {
        let editor = Arc::make_mut(self.editors.get_mut(&editor_view_id).unwrap());
        match &editor.content {
            EditorContent::Buffer(path) => {
                let buffer = self.open_files.get(path).unwrap().clone();
                editor.save_jump_location(&buffer);
            }
            EditorContent::None => {}
        }
        self.go_to_location(ctx, editor_view_id, location, config);
    }

    pub fn go_to_location(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: WidgetId,
        location: EditorLocationNew,
        config: &Config,
    ) {
        let editor = Arc::make_mut(self.editors.get_mut(&editor_view_id).unwrap());
        let new_buffer = match &editor.content {
            EditorContent::Buffer(path) => path != &location.path,
            EditorContent::None => true,
        };
        let path = location.path.clone();
        let buffer_exists = self.open_files.contains_key(&path);
        if !buffer_exists {
            let buffer =
                Arc::new(BufferNew::new(path.clone(), self.update_sender.clone()));
            self.open_files.insert(path.clone(), buffer.clone());
            buffer.retrieve_file(
                *self.tab_id,
                self.proxy.clone(),
                ctx.get_external_handle(),
                vec![(editor.view_id, location)],
            );
        } else {
            let buffer = self.open_files.get(&path).unwrap();

            let (offset, scroll_offset) = match &location.position {
                Some(position) => (
                    buffer.offset_of_position(position),
                    location.scroll_offset.as_ref(),
                ),
                None => (buffer.cursor_offset, Some(&buffer.scroll_offset)),
            };

            editor.content = EditorContent::Buffer(path.clone());
            editor.cursor = if config.lapce.modal {
                Cursor::new(CursorMode::Normal(offset), None)
            } else {
                Cursor::new(CursorMode::Insert(Selection::caret(offset)), None)
            };

            if let Some(scroll_offset) = scroll_offset {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ForceScrollTo(scroll_offset.x, scroll_offset.y),
                    Target::Widget(editor.view_id),
                ));
            } else {
                if new_buffer || editor_view_id == *self.palette_preview_editor {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EnsureCursorCenter,
                        Target::Widget(editor.view_id),
                    ));
                } else {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EnsureCursorVisible(Some(
                            EnsureVisiblePosition::CenterOfWindow,
                        )),
                        Target::Widget(editor.view_id),
                    ));
                }
            }
        }
    }

    pub fn jump_to_line(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: WidgetId,
        line: usize,
        config: &Config,
    ) {
        let editor = self.editors.get(&editor_view_id).unwrap();
        match &editor.content {
            EditorContent::Buffer(path) => {
                let buffer = self.open_files.get(path).unwrap();
                let offset = buffer.first_non_blank_character_on_line(if line > 0 {
                    line - 1
                } else {
                    0
                });
                let position = buffer.offset_to_position(offset);
                self.jump_to_position(ctx, editor_view_id, position, config);
            }
            EditorContent::None => {}
        }
    }
}

impl LapceMainSplitData {
    pub fn new(
        tab_id: WidgetId,
        workspace_info: Option<&WorkspaceInfo>,
        palette_preview_editor: WidgetId,
        update_sender: Arc<Sender<UpdateEvent>>,
        proxy: Arc<LapceProxy>,
        config: &Config,
        event_sink: ExtEventSink,
    ) -> Self {
        let split_id = Arc::new(WidgetId::next());

        let mut open_files = im::HashMap::new();
        let mut editors = im::HashMap::new();
        let mut editors_order = Vec::new();

        let mut active = WidgetId::next();
        if let Some(info) = workspace_info {
            let mut positions = HashMap::new();
            for (i, e) in info.editors.iter().enumerate() {
                let editor = LapceEditorData::new(
                    None,
                    Some(*split_id),
                    e.content.clone(),
                    EditorType::Normal,
                    config,
                );
                if info.active_editor == i {
                    active = editor.view_id;
                }
                match &e.content {
                    EditorContent::Buffer(path) => {
                        if !positions.contains_key(path) {
                            positions.insert(path.clone(), vec![]);
                        }

                        positions.get_mut(path).unwrap().push((
                            editor.view_id,
                            EditorLocationNew {
                                path: path.clone(),
                                position: e.position.clone(),
                                scroll_offset: Some(Vec2::new(
                                    e.scroll_offset.0,
                                    e.scroll_offset.1,
                                )),
                            },
                        ));

                        if !open_files.contains_key(path) {
                            let buffer = Arc::new(BufferNew::new(
                                path.clone(),
                                update_sender.clone(),
                            ));
                            open_files.insert(path.clone(), buffer.clone());
                        }
                    }
                    EditorContent::None => {}
                }
                editors_order.push(editor.view_id);
                editors.insert(editor.view_id, Arc::new(editor));
            }
            for (path, locations) in positions.into_iter() {
                open_files.get(&path).unwrap().retrieve_file(
                    tab_id,
                    proxy.clone(),
                    event_sink.clone(),
                    locations.clone(),
                );
            }
        }

        if editors.len() == 0 {
            let editor = LapceEditorData::new(
                None,
                Some(*split_id),
                EditorContent::None,
                EditorType::Normal,
                config,
            );
            active = editor.view_id;
            editors_order.push(editor.view_id);
            editors.insert(editor.view_id, Arc::new(editor));
        }

        let path = PathBuf::from("[Palette Preview Editor]");
        let editor = LapceEditorData::new(
            Some(palette_preview_editor),
            None,
            EditorContent::Buffer(path.clone()),
            EditorType::Palette,
            config,
        );
        editors.insert(editor.view_id, Arc::new(editor));
        let mut buffer = BufferNew::new(path.clone(), update_sender.clone());
        buffer.loaded = true;
        open_files.insert(path.clone(), Arc::new(buffer));

        Self {
            tab_id: Arc::new(tab_id),
            split_id,
            editors,
            editors_order: Arc::new(editors_order),
            open_files,
            active: Arc::new(active),
            update_sender,
            register: Arc::new(Register::default()),
            proxy,
            palette_preview_editor: Arc::new(palette_preview_editor),
            show_code_actions: false,
            current_code_actions: 0,
            diagnostics: im::HashMap::new(),
            error_count: 0,
            warning_count: 0,
        }
    }

    pub fn add_source_control_editor(
        &mut self,
        view_id: WidgetId,
        split_id: WidgetId,
        config: &Config,
    ) {
        let path = PathBuf::from(SOURCE_CONTROL_BUFFER);
        let mut buffer =
            BufferNew::new(path.clone(), self.update_sender.clone()).set_local();
        buffer.load_content("");
        self.open_files.insert(path.clone(), Arc::new(buffer));
        let editor = LapceEditorData::new(
            Some(view_id),
            Some(split_id),
            EditorContent::Buffer(path.clone()),
            EditorType::SourceControl,
            config,
        );
        self.editors.insert(editor.view_id, Arc::new(editor));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditorType {
    DiffSplit(WidgetId, WidgetId),
    Normal,
    SourceControl,
    Palette,
}

pub enum LapceEditorContainerKind {
    Container(WidgetId),
    DiffSplit(WidgetId, WidgetId),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum EditorContent {
    Buffer(PathBuf),
    None,
}

#[derive(Clone, Debug)]
pub enum InlineFindDirection {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub struct LapceEditorData {
    pub split_id: Option<WidgetId>,
    pub view_id: WidgetId,
    pub editor_type: EditorType,
    pub content: EditorContent,
    pub scroll_offset: Vec2,
    pub cursor: Cursor,
    pub size: Rc<RefCell<Size>>,
    pub window_origin: Point,
    pub snippet: Option<Vec<(usize, (usize, usize))>>,
    pub locations: Vec<EditorLocationNew>,
    pub current_location: usize,
    pub last_movement: Movement,
    pub last_inline_find: Option<(InlineFindDirection, String)>,
    pub inline_find: Option<InlineFindDirection>,
}

impl LapceEditorData {
    pub fn new(
        view_id: Option<WidgetId>,
        split_id: Option<WidgetId>,
        content: EditorContent,
        editor_type: EditorType,
        config: &Config,
    ) -> Self {
        Self {
            split_id,
            view_id: view_id.unwrap_or(WidgetId::next()),
            editor_type,
            content,
            scroll_offset: Vec2::ZERO,
            cursor: if config.lapce.modal {
                Cursor::new(CursorMode::Normal(0), None)
            } else {
                Cursor::new(CursorMode::Insert(Selection::caret(0)), None)
            },
            size: Rc::new(RefCell::new(Size::ZERO)),
            window_origin: Point::ZERO,
            snippet: None,
            locations: vec![],
            current_location: 0,
            last_movement: Movement::Left,
            inline_find: None,
            last_inline_find: None,
        }
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

    pub fn save_jump_location(&mut self, buffer: &BufferNew) {
        let location = EditorLocationNew {
            path: buffer.path.clone(),
            position: Some(buffer.offset_to_position(self.cursor.offset())),
            scroll_offset: Some(self.scroll_offset.clone()),
        };
        self.locations.push(location);
        self.current_location = self.locations.len();
    }
}

#[derive(Clone, Data, Lens)]
pub struct LapceEditorViewData {
    pub main_split: LapceMainSplitData,
    pub workspace: Option<Arc<LapceWorkspace>>,
    pub proxy: Arc<LapceProxy>,
    pub editor: Arc<LapceEditorData>,
    pub buffer: Arc<BufferNew>,
    pub diagnostics: Arc<Vec<EditorDiagnostic>>,
    pub all_diagnostics: im::HashMap<PathBuf, Arc<Vec<EditorDiagnostic>>>,
    pub keypress: Arc<KeyPressData>,
    pub completion: Arc<CompletionData>,
    pub palette: Arc<WidgetId>,
    pub theme: Arc<std::collections::HashMap<String, Color>>,
    pub config: Arc<Config>,
}

impl LapceEditorViewData {
    // pub fn key_down(
    //     &mut self,
    //     ctx: &mut EventCtx,
    //     key_event: &KeyEvent,
    //     env: &Env,
    // ) -> bool {
    //     let mut keypress = self.keypress.clone();
    //     let k = Arc::make_mut(&mut keypress);
    //     let executed = k.key_down(ctx, key_event, self, env);
    //     self.keypress = keypress;
    //     executed
    // }

    pub fn buffer_mut(&mut self) -> &mut BufferNew {
        Arc::make_mut(&mut self.buffer)
    }

    pub fn sync_buffer_position(&mut self, scroll_offset: Vec2) {
        let cursor_offset = self.editor.cursor.offset();
        if self.buffer.cursor_offset != cursor_offset
            || self.buffer.scroll_offset != scroll_offset
        {
            let buffer = self.buffer_mut();
            buffer.cursor_offset = cursor_offset;
            buffer.scroll_offset = scroll_offset;
        }
    }

    pub fn fill_text_layouts(
        &mut self,
        ctx: &mut EventCtx,
        theme: &Arc<HashMap<String, Color>>,
        env: &Env,
    ) {
        // let start = std::time::SystemTime::now();
        // let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        // let start_line = (self.editor.scroll_offset.y / line_height) as usize;
        // let size = self.editor.size;
        // let num_lines = ((size.height / line_height).ceil()) as usize;
        // let text = ctx.text();
        // let buffer = self.buffer_mut();
        // for line in start_line..start_line + num_lines + 1 {
        //     buffer.update_line_layouts(text, line, theme, env);
        // }
        // let end = std::time::SystemTime::now();
        // let duration = end.duration_since(start).unwrap().as_micros();
        // println!("fill text layout took {}", duration);
    }

    pub fn on_diagnostic(&self) -> Option<usize> {
        let offset = self.editor.cursor.offset();
        let position = self.buffer.offset_to_position(offset);
        for diagnostic in self.diagnostics.iter() {
            if diagnostic.diagnositc.range.start == position {
                return Some(offset);
            }
        }
        None
    }

    pub fn current_code_actions(&self) -> Option<&CodeActionResponse> {
        let offset = self.editor.cursor.offset();
        let prev_offset = self.buffer.prev_code_boundary(offset);
        self.buffer.code_actions.get(&prev_offset)
    }

    pub fn get_code_actions(&mut self, ctx: &mut EventCtx) {
        if !self.buffer.loaded {
            return;
        }
        if self.buffer.local {
            return;
        }
        let offset = self.editor.cursor.offset();
        let prev_offset = self.buffer.prev_code_boundary(offset);
        if self.buffer.code_actions.get(&prev_offset).is_none() {
            let buffer_id = self.buffer.id;
            let position = self.buffer.offset_to_position(prev_offset);
            let path = self.buffer.path.clone();
            let rev = self.buffer.rev;
            let event_sink = ctx.get_external_handle();
            self.proxy.get_code_actions(
                buffer_id,
                position,
                Box::new(move |result| {
                    if let Ok(res) = result {
                        if let Ok(resp) =
                            serde_json::from_value::<CodeActionResponse>(res)
                        {
                            event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::UpdateCodeActions(
                                    path,
                                    rev,
                                    prev_offset,
                                    resp,
                                ),
                                Target::Auto,
                            );
                        }
                    }
                }),
            );
        }
    }

    // fn move_command(
    //     &self,
    //     count: Option<usize>,
    //     cmd: &LapceCommand,
    // ) -> Option<Movement> {
    //     match cmd {
    //         LapceCommand::Left => Some(Movement::Left),
    //         LapceCommand::Right => Some(Movement::Right),
    //         LapceCommand::Up => Some(Movement::Up),
    //         LapceCommand::Down => Some(Movement::Down),
    //         LapceCommand::LineStart => Some(Movement::StartOfLine),
    //         LapceCommand::LineEnd => Some(Movement::EndOfLine),
    //         LapceCommand::GotoLineDefaultFirst => Some(match count {
    //             Some(n) => Movement::Line(LinePosition::Line(n)),
    //             None => Movement::Line(LinePosition::First),
    //         }),
    //         LapceCommand::GotoLineDefaultLast => Some(match count {
    //             Some(n) => Movement::Line(LinePosition::Line(n)),
    //             None => Movement::Line(LinePosition::Last),
    //         }),
    //         LapceCommand::WordBackward => Some(Movement::WordBackward),
    //         LapceCommand::WordFoward => Some(Movement::WordForward),
    //         LapceCommand::WordEndForward => Some(Movement::WordEndForward),
    //         LapceCommand::MatchPairs => Some(Movement::MatchPairs),
    //         LapceCommand::NextUnmatchedRightBracket => {
    //             Some(Movement::NextUnmatched(')'))
    //         }
    //         LapceCommand::PreviousUnmatchedLeftBracket => {
    //             Some(Movement::PreviousUnmatched('('))
    //         }
    //         LapceCommand::NextUnmatchedRightCurlyBracket => {
    //             Some(Movement::NextUnmatched('}'))
    //         }
    //         LapceCommand::PreviousUnmatchedLeftCurlyBracket => {
    //             Some(Movement::PreviousUnmatched('{'))
    //         }
    //         _ => None,
    //     }
    // }

    fn toggle_visual(&mut self, visual_mode: VisualMode) {
        if !self.config.lapce.modal {
            return;
        }

        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;

        match &cursor.mode {
            CursorMode::Visual { start, end, mode } => {
                if mode != &visual_mode {
                    cursor.mode = CursorMode::Visual {
                        start: *start,
                        end: *end,
                        mode: visual_mode,
                    };
                } else {
                    cursor.mode = CursorMode::Normal(*end);
                };
            }
            _ => {
                let offset = cursor.offset();
                cursor.mode = CursorMode::Visual {
                    start: offset,
                    end: offset,
                    mode: visual_mode,
                };
            }
        }
    }

    pub fn apply_completion_item(
        &mut self,
        ctx: &mut EventCtx,
        item: &CompletionItem,
    ) -> Result<()> {
        let additioal_edit = item.additional_text_edits.as_ref().map(|edits| {
            edits
                .iter()
                .map(|edit| {
                    let selection = Selection::region(
                        self.buffer.offset_of_position(&edit.range.start),
                        self.buffer.offset_of_position(&edit.range.end),
                    );
                    (selection, edit.new_text.clone())
                })
                .collect::<Vec<(Selection, String)>>()
        });
        let additioal_edit = additioal_edit.as_ref().map(|edits| {
            edits
                .into_iter()
                .map(|(selection, c)| (selection, c.as_str()))
                .collect()
        });

        let text_format = item
            .insert_text_format
            .unwrap_or(lsp_types::InsertTextFormat::PlainText);
        if let Some(edit) = &item.text_edit {
            match edit {
                CompletionTextEdit::Edit(edit) => {
                    let offset = self.editor.cursor.offset();
                    let start_offset = self.buffer.prev_code_boundary(offset);
                    let end_offset = self.buffer.next_code_boundary(offset);
                    let edit_start =
                        self.buffer.offset_of_position(&edit.range.start);
                    let edit_end = self.buffer.offset_of_position(&edit.range.end);
                    let selection = Selection::region(
                        start_offset.min(edit_start),
                        end_offset.max(edit_end),
                    );
                    match text_format {
                        lsp_types::InsertTextFormat::PlainText => {
                            let (selection, _) = self.edit(
                                ctx,
                                &selection,
                                &edit.new_text,
                                additioal_edit,
                                true,
                                EditType::InsertChars,
                            );
                            self.set_cursor_after_change(selection);
                            return Ok(());
                        }
                        lsp_types::InsertTextFormat::Snippet => {
                            let snippet = Snippet::from_str(&edit.new_text)?;
                            let text = snippet.text();
                            let (selection, delta) = self.edit(
                                ctx,
                                &selection,
                                &text,
                                additioal_edit,
                                true,
                                EditType::InsertChars,
                            );

                            let mut transformer = Transformer::new(&delta);
                            let offset = transformer
                                .transform(start_offset.min(edit_start), false);
                            let snippet_tabs = snippet.tabs(offset);

                            if snippet_tabs.len() == 0 {
                                self.set_cursor_after_change(selection);
                                return Ok(());
                            }

                            let mut selection = Selection::new();
                            let (tab, (start, end)) = &snippet_tabs[0];
                            let region = SelRegion::new(*start, *end, None);
                            selection.add_region(region);
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
                            Arc::make_mut(&mut self.editor)
                                .add_snippet_placeholders(snippet_tabs);
                            return Ok(());
                        }
                    }
                }
                CompletionTextEdit::InsertAndReplace(_) => (),
            }
        }

        let offset = self.editor.cursor.offset();
        let start_offset = self.buffer.prev_code_boundary(offset);
        let end_offset = self.buffer.next_code_boundary(offset);
        let selection = Selection::region(start_offset, end_offset);

        let (selection, _) = self.edit(
            ctx,
            &selection,
            item.insert_text.as_ref().unwrap_or(&item.label),
            additioal_edit,
            true,
            EditType::InsertChars,
        );
        self.set_cursor_after_change(selection);
        Ok(())
    }

    fn scroll(&mut self, ctx: &mut EventCtx, down: bool, count: usize, env: &Env) {
        let line_height = self.config.editor.line_height as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.editor.cursor.offset();
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let top = self.editor.scroll_offset.y + diff;
        let bottom = top + self.editor.size.borrow().height;

        let line = if (line + 1) as f64 * line_height + line_height > bottom {
            let line = (bottom / line_height).floor() as usize;
            if line > 2 {
                line - 2
            } else {
                0
            }
        } else if line as f64 * line_height - line_height < top {
            let line = (top / line_height).ceil() as usize;
            line + 1
        } else {
            line
        };

        let offset = self.buffer.offset_of_line(line)
            + col.min(self.buffer.line_end_col(line, false));
        self.set_cursor(Cursor::new(
            CursorMode::Normal(offset),
            self.editor.cursor.horiz.clone(),
        ));
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((self.editor.scroll_offset.x, top)),
            Target::Widget(self.editor.view_id),
        ));
    }

    fn page_move(&mut self, ctx: &mut EventCtx, down: bool, env: &Env) {
        let line_height = self.config.editor.line_height as f64;
        let lines =
            (self.editor.size.borrow().height / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.do_move(if down { &Movement::Down } else { &Movement::Up }, lines);
        let rect = Rect::ZERO
            .with_origin(
                self.editor.scroll_offset.to_point()
                    + Vec2::new(0.0, if down { distance } else { -distance }),
            )
            .with_size(self.editor.size.borrow().clone());
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureRectVisible(rect),
            Target::Widget(self.editor.view_id),
        ));
    }

    pub fn next_error(&mut self, ctx: &mut EventCtx, env: &Env) {
        let mut file_diagnostics = self
            .all_diagnostics
            .iter()
            .filter_map(|(path, diagnositics)| {
                //let buffer = self.get_buffer_from_path(ctx, ui_state, path);
                let mut errors: Vec<Position> = diagnositics
                    .iter()
                    .filter_map(|d| {
                        let severity = d
                            .diagnositc
                            .severity
                            .unwrap_or(DiagnosticSeverity::Hint);
                        if severity != DiagnosticSeverity::Error {
                            return None;
                        }
                        Some(d.diagnositc.range.start)
                    })
                    .collect();
                if errors.len() == 0 {
                    None
                } else {
                    errors.sort();
                    Some((path, errors))
                }
            })
            .collect::<Vec<(&PathBuf, Vec<Position>)>>();
        if file_diagnostics.len() == 0 {
            return;
        }
        file_diagnostics.sort_by(|a, b| a.0.cmp(b.0));

        let offset = self.editor.cursor.offset();
        let position = self.buffer.offset_to_position(offset);
        let (path, position) = next_in_file_errors_offset(
            position,
            &self.buffer.path,
            &file_diagnostics,
        );
        let location = EditorLocationNew {
            path,
            position: Some(position),
            scroll_offset: None,
        };
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::JumpToLocation(EditorKind::SplitActive, location),
            Target::Auto,
        ));
    }

    pub fn jump_location_forward(
        &mut self,
        ctx: &mut EventCtx,
        env: &Env,
    ) -> Option<()> {
        if self.editor.locations.len() == 0 {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() - 1 {
            return None;
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.current_location += 1;
        let location = editor.locations[editor.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocationNew(editor.view_id, location),
            Target::Auto,
        ));
        None
    }

    pub fn jump_location_backward(
        &mut self,
        ctx: &mut EventCtx,
        env: &Env,
    ) -> Option<()> {
        if self.editor.current_location < 1 {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer);
            editor.current_location -= 1;
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.current_location -= 1;
        let location = editor.locations[editor.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocationNew(editor.view_id, location),
            Target::Auto,
        ));
        None
    }

    pub fn do_move(&mut self, movement: &Movement, count: usize) {
        if movement.is_jump() && movement != &self.editor.last_movement {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer);
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.last_movement = movement.clone();
        match &self.editor.cursor.mode {
            &CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    offset,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                );
                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Normal(new_offset);
                editor.cursor.horiz = Some(horiz);
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    *end,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                );
                let start = *start;
                let mode = mode.clone();
                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Visual {
                    start,
                    end: new_offset,
                    mode,
                };
                editor.cursor.horiz = Some(horiz);
            }
            CursorMode::Insert(selection) => {
                let selection = self.buffer.update_selection(
                    selection,
                    count,
                    movement,
                    Mode::Insert,
                    false,
                );
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

    // pub fn cusor_region(&self, config: &Config) -> Rect {
    //     self.editor.cursor.region(&self.buffer, config)
    // }

    pub fn insert_new_line(&mut self, ctx: &mut EventCtx, offset: usize) {
        let line = self.buffer.line_of_offset(offset);
        let line_start = self.buffer.offset_of_line(line);
        let line_end = self.buffer.line_end_offset(line, true);
        let line_indent = self.buffer.indent_on_line(line);
        let first_half = self.buffer.slice_to_cow(line_start..offset).to_string();
        let second_half = self.buffer.slice_to_cow(offset..line_end).to_string();

        let indent = if has_unmatched_pair(&first_half) {
            format!("{}    ", line_indent)
        } else {
            let next_line_indent = self.buffer.indent_on_line(line + 1);
            if next_line_indent.len() > line_indent.len() {
                next_line_indent
            } else {
                line_indent.clone()
            }
        };

        let selection = Selection::caret(offset);
        let content = format!("{}{}", "\n", indent);

        let (selection, _) = self.edit(
            ctx,
            &selection,
            &content,
            None,
            true,
            EditType::InsertNewline,
        );
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor.mode = CursorMode::Insert(selection.clone());
        editor.cursor.horiz = None;

        for c in first_half.chars().rev() {
            if c != ' ' {
                if let Some(pair_start) = matching_pair_direction(c) {
                    if pair_start {
                        if let Some(c) = matching_char(c) {
                            if second_half.trim().starts_with(&c.to_string()) {
                                let content = format!("{}{}", "\n", line_indent);
                                self.edit(
                                    ctx,
                                    &selection,
                                    &content,
                                    None,
                                    true,
                                    EditType::InsertNewline,
                                );
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    pub fn set_cursor_after_change(&mut self, selection: Selection) {
        match self.editor.cursor.mode {
            CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                let offset = selection.min_offset();
                let offset = self.buffer.offset_line_end(offset, false).min(offset);
                self.set_cursor(Cursor::new(CursorMode::Normal(offset), None));
            }
            CursorMode::Insert(_) => {
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

    pub fn offset_of_mouse(
        &self,
        text: &mut PietText,
        pos: Point,
        config: &Config,
    ) -> usize {
        let line_height = self.config.editor.line_height as f64;
        let line = (pos.y / line_height).floor() as usize;
        let last_line = self.buffer.last_line();
        let (line, col) = if line > last_line {
            (last_line, 0)
        } else {
            let line_end = self
                .buffer
                .line_end_col(line, !self.editor.cursor.is_normal());
            let width = config.editor_text_width(text, "W");

            let col = (if self.editor.cursor.is_insert() {
                (pos.x / width).round() as usize
            } else {
                (pos.x / width).floor() as usize
            })
            .min(line_end);
            (line, col)
        };
        self.buffer.offset_of_line_col(line, col)
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor = cursor;
    }

    fn paste(&mut self, ctx: &mut EventCtx, data: &RegisterData) {
        match data.mode {
            VisualMode::Normal => {
                Arc::make_mut(&mut self.editor).snippet = None;
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line_end = self.buffer.offset_line_end(offset, true);
                        let offset = (offset + 1).min(line_end);
                        Selection::caret(offset)
                    }
                    CursorMode::Insert { .. } | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                };
                let after = !data.content.contains("\n");
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    &data.content,
                    None,
                    after,
                    EditType::InsertChars,
                );
                if !after {
                    self.set_cursor_after_change(selection);
                } else {
                    match self.editor.cursor.mode {
                        CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                            let offset = self.buffer.prev_grapheme_offset(
                                selection.min_offset(),
                                1,
                                0,
                            );
                            self.set_cursor(Cursor::new(
                                CursorMode::Normal(offset),
                                None,
                            ));
                        }
                        CursorMode::Insert { .. } => {
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
                        }
                    }
                }
            }
            VisualMode::Linewise | VisualMode::Blockwise => {
                let (selection, content) = match &self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line = self.buffer.line_of_offset(*offset);
                        let offset = self.buffer.offset_of_line(line + 1);
                        (Selection::caret(offset), data.content.clone())
                    }
                    CursorMode::Insert { .. } => (
                        self.editor.cursor.edit_selection(&self.buffer),
                        "\n".to_string() + &data.content,
                    ),
                    CursorMode::Visual { mode, .. } => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let data = match mode {
                            VisualMode::Linewise => data.content.clone(),
                            _ => "\n".to_string() + &data.content,
                        };
                        (selection, data)
                    }
                };
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    &content,
                    None,
                    false,
                    EditType::InsertChars,
                );
                match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        let offset = selection.min_offset();
                        let offset = if self.editor.cursor.is_visual() {
                            offset + 1
                        } else {
                            offset
                        };
                        let line = self.buffer.line_of_offset(offset);
                        let offset =
                            self.buffer.first_non_blank_character_on_line(line);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Insert(_) => {
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }
                }
            }
        }
    }

    pub fn initiate_diagnositcs_offset(&mut self) {
        if self.diagnostics.len() > 0 {
            let buffer = self.buffer.clone();
            for diagnostic in Arc::make_mut(&mut self.diagnostics).iter_mut() {
                if diagnostic.range.is_none() {
                    diagnostic.range = Some((
                        buffer
                            .offset_of_position(&diagnostic.diagnositc.range.start),
                        buffer.offset_of_position(&diagnostic.diagnositc.range.end),
                    ));
                }
            }
        }
    }

    pub fn update_diagnositcs_offset(&mut self, delta: &RopeDelta) {
        if self.diagnostics.len() > 0 {
            let buffer = self.buffer.clone();
            let mut transformer = Transformer::new(delta);
            for diagnostic in Arc::make_mut(&mut self.diagnostics).iter_mut() {
                let (start, end) = diagnostic.range.clone().unwrap();
                let (new_start, new_end) = (
                    transformer.transform(start, false),
                    transformer.transform(end, true),
                );
                diagnostic.range = Some((new_start, new_end));
                if start != new_start {
                    diagnostic.diagnositc.range.start =
                        buffer.offset_to_position(new_start);
                }
                if end != new_end {
                    diagnostic.diagnositc.range.end =
                        buffer.offset_to_position(new_end);
                }
            }
        }
    }

    fn edit(
        &mut self,
        ctx: &mut EventCtx,
        selection: &Selection,
        c: &str,
        additional_edit: Option<Vec<(&Selection, &str)>>,
        after: bool,
        edit_type: EditType,
    ) -> (Selection, RopeDelta) {
        match &self.editor.cursor.mode {
            CursorMode::Normal(_) => {
                if !selection.is_caret() {
                    let data = self.editor.cursor.yank(&self.buffer);
                    let register = Arc::make_mut(&mut self.main_split.register);
                    register.add_delete(data);
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let data = self.editor.cursor.yank(&self.buffer);
                let register = Arc::make_mut(&mut self.main_split.register);
                register.add_delete(data);
            }
            CursorMode::Insert(_) => {}
        }

        self.initiate_diagnositcs_offset();

        let proxy = self.proxy.clone();
        let buffer = self.buffer_mut();
        let delta = if let Some(additional_edit) = additional_edit {
            let mut edits = vec![(selection, c)];
            edits.extend_from_slice(&additional_edit);
            buffer.edit_multiple(ctx, edits, proxy, edit_type)
        } else {
            buffer.edit(ctx, &selection, c, proxy, edit_type)
        };
        self.inactive_apply_delta(&delta);
        let selection = selection.apply_delta(&delta, after, InsertDrift::Default);
        if let Some(snippet) = self.editor.snippet.clone() {
            let mut transformer = Transformer::new(&delta);
            Arc::make_mut(&mut self.editor).snippet = Some(
                snippet
                    .iter()
                    .map(|(tab, (start, end))| {
                        (
                            *tab,
                            (
                                transformer.transform(*start, false),
                                transformer.transform(*end, true),
                            ),
                        )
                    })
                    .collect(),
            );
        }

        self.update_diagnositcs_offset(&delta);

        (selection, delta)
    }

    fn inactive_apply_delta(&mut self, delta: &RopeDelta) {
        for (view_id, editor) in self.main_split.editors.iter_mut() {
            if view_id != &self.editor.view_id {
                match (&self.editor.content, &editor.content) {
                    (
                        EditorContent::Buffer(current_path),
                        EditorContent::Buffer(path),
                    ) => {
                        if current_path == path {
                            Arc::make_mut(editor).cursor.apply_delta(delta);
                        }
                    }
                    _ => (),
                }
            }
        }
    }

    pub fn cancel_completion(&mut self) {
        let completion = Arc::make_mut(&mut self.completion);
        completion.cancel();
    }

    //  fn update_completion(&mut self, ctx: &mut EventCtx) {
    //      if self.get_mode() != Mode::Insert {
    //          return;
    //      }
    //      if !self.buffer.loaded {
    //          return;
    //      }
    //      if self.buffer.local {
    //          return;
    //      }
    //      let offset = self.editor.cursor.offset();
    //      let start_offset = self.buffer.prev_code_boundary(offset);
    //      let end_offset = self.buffer.next_code_boundary(offset);
    //      let input = self
    //          .buffer
    //          .slice_to_cow(start_offset..end_offset)
    //          .to_string();
    //      let char = if start_offset == 0 {
    //          "".to_string()
    //      } else {
    //          self.buffer
    //              .slice_to_cow(start_offset - 1..start_offset)
    //              .to_string()
    //      };
    //      let completion = Arc::make_mut(&mut self.completion);
    //      if input == "" && char != "." && char != ":" {
    //          completion.cancel();
    //          return;
    //      }

    //      if completion.status != CompletionStatus::Inactive
    //          && completion.offset == start_offset
    //          && completion.buffer_id == self.buffer.id
    //      {
    //          completion.update_input(input.clone());

    //          if !completion.input_items.contains_key("") {
    //              let event_sink = ctx.get_external_handle();
    //              completion.request(
    //                  self.proxy.clone(),
    //                  completion.request_id,
    //                  self.buffer.id,
    //                  "".to_string(),
    //                  self.buffer.offset_to_position(start_offset),
    //                  completion.id,
    //                  event_sink,
    //              );
    //          }

    //          if !completion.input_items.contains_key(&input) {
    //              let event_sink = ctx.get_external_handle();
    //              completion.request(
    //                  self.proxy.clone(),
    //                  completion.request_id,
    //                  self.buffer.id,
    //                  input,
    //                  self.buffer.offset_to_position(offset),
    //                  completion.id,
    //                  event_sink,
    //              );
    //          }

    //          return;
    //      }

    //      completion.buffer_id = self.buffer.id;
    //      completion.offset = start_offset;
    //      completion.input = input.clone();
    //      completion.status = CompletionStatus::Started;
    //      completion.input_items.clear();
    //      completion.request_id += 1;
    //      let event_sink = ctx.get_external_handle();
    //      completion.request(
    //          self.proxy.clone(),
    //          completion.request_id,
    //          self.buffer.id,
    //          "".to_string(),
    //          self.buffer.offset_to_position(start_offset),
    //          completion.id,
    //          event_sink.clone(),
    //      );
    //      if input != "" {
    //          completion.request(
    //              self.proxy.clone(),
    //              completion.request_id,
    //              self.buffer.id,
    //              input,
    //              self.buffer.offset_to_position(offset),
    //              completion.id,
    //              event_sink,
    //          );
    //      }
    //  }
}

// pub struct LapceEditorLens(pub WidgetId);
//
// impl Lens<LapceTabData, LapceEditorViewData> for LapceEditorLens {
//     fn with<V, F: FnOnce(&LapceEditorViewData) -> V>(
//         &self,
//         data: &LapceTabData,
//         f: F,
//     ) -> V {
//         let main_split = &data.main_split;
//         let editor = main_split.editors.get(&self.0).unwrap();
//         let diagnostics = data
//             .main_split
//             .diagnostics
//             .get(&editor.buffer)
//             .unwrap_or(&Arc::new(vec![]))
//             .clone();
//         let editor_view = LapceEditorViewData {
//             buffer: main_split.open_files.get(&editor.buffer).unwrap().clone(),
//             workspace: data.workspace.clone(),
//             editor: editor.clone(),
//             main_split: main_split.clone(),
//             diagnostics,
//             all_diagnostics: data.main_split.diagnostics.clone(),
//             keypress: data.keypress.clone(),
//             completion: data.completion.clone(),
//             palette: Arc::new(data.palette.widget_id),
//             theme: data.theme.clone(),
//             proxy: data.proxy.clone(),
//             config: data.config.clone(),
//         };
//         f(&editor_view)
//     }
//
//     fn with_mut<V, F: FnOnce(&mut LapceEditorViewData) -> V>(
//         &self,
//         data: &mut LapceTabData,
//         f: F,
//     ) -> V {
//         let main_split = &data.main_split;
//         let editor = main_split.editors.get(&self.0).unwrap().clone();
//         let buffer = main_split.open_files.get(&editor.buffer).unwrap().clone();
//         let diagnostics = data
//             .main_split
//             .diagnostics
//             .get(&editor.buffer)
//             .unwrap_or(&Arc::new(vec![]))
//             .clone();
//         let mut editor_view = LapceEditorViewData {
//             buffer: buffer.clone(),
//             workspace: data.workspace.clone(),
//             editor: editor.clone(),
//             diagnostics: diagnostics.clone(),
//             all_diagnostics: data.main_split.diagnostics.clone(),
//             main_split: data.main_split.clone(),
//             keypress: data.keypress.clone(),
//             completion: data.completion.clone(),
//             palette: Arc::new(data.palette.widget_id),
//             theme: data.theme.clone(),
//             proxy: data.proxy.clone(),
//             config: data.config.clone(),
//         };
//         let result = f(&mut editor_view);
//
//         data.keypress = editor_view.keypress.clone();
//         data.completion = editor_view.completion.clone();
//         data.main_split = editor_view.main_split.clone();
//         if !diagnostics.same(&editor_view.diagnostics) {
//             data.main_split.diagnostics.insert(
//                 editor_view.buffer.path.clone(),
//                 editor_view.diagnostics.clone(),
//             );
//         }
//         data.theme = editor_view.theme.clone();
//         if !editor.same(&editor_view.editor) {
//             data.main_split
//                 .editors
//                 .insert(self.0, editor_view.editor.clone());
//         }
//         if !buffer.same(&editor_view.buffer) {
//             data.main_split
//                 .open_files
//                 .insert(editor_view.buffer.path.clone(), editor_view.buffer.clone());
//         }
//
//         result
//     }
// }

// impl KeyPressFocus for LapceEditorViewData {
//     fn get_mode(&self) -> Mode {
//         self.editor.cursor.get_mode()
//     }
//
//     fn check_condition(&self, condition: &str) -> bool {
//         match condition {
//             "editor_focus" => true,
//             "source_control_focus" => {
//                 self.editor.editor_type == EditorType::SourceControl
//             }
//             "in_snippet" => self.editor.snippet.is_some(),
//             "list_focus" => {
//                 self.completion.status != CompletionStatus::Inactive
//                     && self.completion.len() > 0
//             }
//             _ => false,
//         }
//     }

// fn run_command(
//     &mut self,
//     ctx: &mut EventCtx,
//     cmd: &LapceCommand,
//     count: Option<usize>,
//     env: &Env,
// ) {
//     if let Some(movement) = self.move_command(count, cmd) {
//         self.do_move(&movement, count.unwrap_or(1));
//         if let Some(snippet) = self.editor.snippet.as_ref() {
//             let offset = self.editor.cursor.offset();
//             let mut within_region = false;
//             for (_, (start, end)) in snippet {
//                 if offset >= *start && offset <= *end {
//                     within_region = true;
//                     break;
//                 }
//             }
//             if !within_region {
//                 Arc::make_mut(&mut self.editor).snippet = None;
//             }
//         }
//         self.cancel_completion();
//         return;
//     }
//     match cmd {
//         LapceCommand::SplitLeft => {
//             if let Some(split_id) = self.editor.split_id.clone() {
//                 ctx.submit_command(Command::new(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::SplitEditorMove(
//                         SplitMoveDirection::Left,
//                         self.editor.view_id,
//                     ),
//                     Target::Widget(split_id),
//                 ));
//             }
//         }
//         LapceCommand::SplitRight => {
//             if let Some(split_id) = self.editor.split_id.clone() {
//                 ctx.submit_command(Command::new(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::SplitEditorMove(
//                         SplitMoveDirection::Right,
//                         self.editor.view_id,
//                     ),
//                     Target::Widget(split_id),
//                 ));
//             }
//         }
//         LapceCommand::SplitUp => {
//             if let Some(split_id) = self.editor.split_id.clone() {
//                 ctx.submit_command(Command::new(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::SplitEditorMove(
//                         SplitMoveDirection::Up,
//                         self.editor.view_id,
//                     ),
//                     Target::Widget(split_id),
//                 ));
//             }
//         }
//         LapceCommand::SplitDown => {
//             if let Some(split_id) = self.editor.split_id.clone() {
//                 ctx.submit_command(Command::new(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::SplitEditorMove(
//                         SplitMoveDirection::Down,
//                         self.editor.view_id,
//                     ),
//                     Target::Widget(split_id),
//                 ));
//             }
//         }
//         LapceCommand::SplitExchange => {
//             if let Some(split_id) = self.editor.split_id.clone() {
//                 if self.editor.editor_type == EditorType::Normal {
//                     ctx.submit_command(Command::new(
//                         LAPCE_UI_COMMAND,
//                         LapceUICommand::SplitEditorExchange(self.editor.view_id),
//                         Target::Widget(split_id),
//                     ));
//                 }
//             }
//         }
//         LapceCommand::SplitVertical => {
//             if let Some(split_id) = self.editor.split_id.clone() {
//                 if self.editor.editor_type == EditorType::Normal {
//                     ctx.submit_command(Command::new(
//                         LAPCE_UI_COMMAND,
//                         LapceUICommand::SplitEditor(true, self.editor.view_id),
//                         Target::Widget(split_id),
//                     ));
//                 }
//             }
//         }
//         LapceCommand::SplitClose => {
//             if let Some(split_id) = self.editor.split_id.clone() {
//                 if self.editor.editor_type == EditorType::Normal {
//                     ctx.submit_command(Command::new(
//                         LAPCE_UI_COMMAND,
//                         LapceUICommand::SplitEditorClose(self.editor.view_id),
//                         Target::Widget(split_id),
//                     ));
//                 }
//             }
//         }
//         LapceCommand::Undo => {
//             self.initiate_diagnositcs_offset();
//             let proxy = self.proxy.clone();
//             let buffer = self.buffer_mut();
//             if let Some(delta) = buffer.do_undo(proxy) {
//                 let (iv, _) = delta.summary();
//                 let line = self.buffer.line_of_offset(iv.start);
//                 let offset = self.buffer.first_non_blank_character_on_line(line);
//                 let selection = Selection::caret(offset);
//                 self.set_cursor_after_change(selection);
//                 self.update_diagnositcs_offset(&delta);
//             }
//         }
//         LapceCommand::Redo => {
//             self.initiate_diagnositcs_offset();
//             let proxy = self.proxy.clone();
//             let buffer = self.buffer_mut();
//             if let Some(delta) = buffer.do_redo(proxy) {
//                 let (iv, _) = delta.summary();
//                 let line = self.buffer.line_of_offset(iv.start);
//                 let offset = self.buffer.first_non_blank_character_on_line(line);
//                 let selection = Selection::caret(offset);
//                 self.set_cursor_after_change(selection);
//                 self.update_diagnositcs_offset(&delta);
//             }
//         }
//         LapceCommand::Append => {
//             let offset = self
//                 .buffer
//                 .move_offset(
//                     self.editor.cursor.offset(),
//                     None,
//                     1,
//                     &Movement::Right,
//                     Mode::Insert,
//                 )
//                 .0;
//             self.buffer_mut().update_edit_type();
//             self.set_cursor(Cursor::new(
//                 CursorMode::Insert(Selection::caret(offset)),
//                 None,
//             ));
//         }
//         LapceCommand::AppendEndOfLine => {
//             let (offset, horiz) = self.buffer.move_offset(
//                 self.editor.cursor.offset(),
//                 None,
//                 1,
//                 &Movement::EndOfLine,
//                 Mode::Insert,
//             );
//             self.buffer_mut().update_edit_type();
//             self.set_cursor(Cursor::new(
//                 CursorMode::Insert(Selection::caret(offset)),
//                 Some(horiz),
//             ));
//         }
//         LapceCommand::InsertMode => {
//             Arc::make_mut(&mut self.editor).cursor.mode = CursorMode::Insert(
//                 Selection::caret(self.editor.cursor.offset()),
//             );
//             self.buffer_mut().update_edit_type();
//         }
//         LapceCommand::InsertFirstNonBlank => {
//             match &self.editor.cursor.mode {
//                 CursorMode::Normal(offset) => {
//                     let (offset, horiz) = self.buffer.move_offset(
//                         *offset,
//                         None,
//                         1,
//                         &Movement::FirstNonBlank,
//                         Mode::Normal,
//                     );
//                     self.buffer_mut().update_edit_type();
//                     self.set_cursor(Cursor::new(
//                         CursorMode::Insert(Selection::caret(offset)),
//                         Some(horiz),
//                     ));
//                 }
//                 CursorMode::Visual { start, end, mode } => {
//                     let mut selection = Selection::new();
//                     for region in
//                         self.editor.cursor.edit_selection(&self.buffer).regions()
//                     {
//                         selection.add_region(SelRegion::caret(region.min()));
//                     }
//                     self.buffer_mut().update_edit_type();
//                     self.set_cursor(Cursor::new(
//                         CursorMode::Insert(selection),
//                         None,
//                     ));
//                 }
//                 CursorMode::Insert(_) => {}
//             };
//         }
//         LapceCommand::NewLineAbove => {
//             let line = self.editor.cursor.current_line(&self.buffer);
//             let offset = if line > 0 {
//                 self.buffer.line_end_offset(line - 1, true)
//             } else {
//                 self.buffer.first_non_blank_character_on_line(line)
//             };
//             self.insert_new_line(ctx, offset);
//         }
//         LapceCommand::NewLineBelow => {
//             let offset = self.editor.cursor.offset();
//             let offset = self.buffer.offset_line_end(offset, true);
//             self.insert_new_line(ctx, offset);
//         }
//         LapceCommand::DeleteToBeginningOfLine => {
//             let selection = match self.editor.cursor.mode {
//                 CursorMode::Normal(_) | CursorMode::Visual { .. } => {
//                     self.editor.cursor.edit_selection(&self.buffer)
//                 }
//                 CursorMode::Insert(_) => {
//                     let selection =
//                         self.editor.cursor.edit_selection(&self.buffer);
//                     let selection = self.buffer.update_selection(
//                         &selection,
//                         1,
//                         &Movement::StartOfLine,
//                         Mode::Insert,
//                         true,
//                     );
//                     selection
//                 }
//             };
//             let (selection, _) =
//                 self.edit(ctx, &selection, "", None, true, EditType::Delete);
//             match self.editor.cursor.mode {
//                 CursorMode::Normal(_) | CursorMode::Visual { .. } => {
//                     let offset = selection.min_offset();
//                     let offset =
//                         self.buffer.offset_line_end(offset, false).min(offset);
//                     self.set_cursor(Cursor::new(
//                         CursorMode::Normal(offset),
//                         None,
//                     ));
//                 }
//                 CursorMode::Insert(_) => {
//                     self.set_cursor(Cursor::new(
//                         CursorMode::Insert(selection),
//                         None,
//                     ));
//                 }
//             }
//         }
//         LapceCommand::Yank => {
//             let data = self.editor.cursor.yank(&self.buffer);
//             let register = Arc::make_mut(&mut self.main_split.register);
//             register.add_yank(data);
//             match &self.editor.cursor.mode {
//                 CursorMode::Visual { start, end, mode } => {
//                     let offset = *start.min(end);
//                     let offset =
//                         self.buffer.offset_line_end(offset, false).min(offset);
//                     self.set_cursor(Cursor::new(
//                         CursorMode::Normal(offset),
//                         None,
//                     ));
//                 }
//                 CursorMode::Normal(_) => {}
//                 CursorMode::Insert(_) => {}
//             }
//         }
//         LapceCommand::ClipboardCopy => {
//             let data = self.editor.cursor.yank(&self.buffer);
//             Application::global().clipboard().put_string(data.content);
//             match &self.editor.cursor.mode {
//                 CursorMode::Visual { start, end, mode } => {
//                     let offset = *start.min(end);
//                     let offset =
//                         self.buffer.offset_line_end(offset, false).min(offset);
//                     self.set_cursor(Cursor::new(
//                         CursorMode::Normal(offset),
//                         None,
//                     ));
//                 }
//                 CursorMode::Normal(_) => {}
//                 CursorMode::Insert(_) => {}
//             }
//         }
//         LapceCommand::ClipboardPaste => {
//             if let Some(s) = Application::global().clipboard().get_string() {
//                 let data = RegisterData {
//                     content: s.to_string(),
//                     mode: VisualMode::Normal,
//                 };
//                 self.paste(ctx, &data);
//             }
//         }
//         LapceCommand::Paste => {
//             let data = self.main_split.register.unamed.clone();
//             self.paste(ctx, &data);
//         }
//         LapceCommand::DeleteWordBackward => {
//             let selection = match self.editor.cursor.mode {
//                 CursorMode::Normal(_) | CursorMode::Visual { .. } => {
//                     self.editor.cursor.edit_selection(&self.buffer)
//                 }
//                 CursorMode::Insert(_) => {
//                     let selection =
//                         self.editor.cursor.edit_selection(&self.buffer);
//                     let selection = self.buffer.update_selection(
//                         &selection,
//                         1,
//                         &Movement::WordBackward,
//                         Mode::Insert,
//                         true,
//                     );
//                     selection
//                 }
//             };
//             let (selection, _) =
//                 self.edit(ctx, &selection, "", None, true, EditType::Delete);
//             self.set_cursor_after_change(selection);
//             self.update_completion(ctx);
//         }
//         LapceCommand::DeleteBackward => {
//             let selection = match self.editor.cursor.mode {
//                 CursorMode::Normal(_) | CursorMode::Visual { .. } => {
//                     self.editor.cursor.edit_selection(&self.buffer)
//                 }
//                 CursorMode::Insert(_) => {
//                     let selection =
//                         self.editor.cursor.edit_selection(&self.buffer);
//                     let mut selection = self.buffer.update_selection(
//                         &selection,
//                         1,
//                         &Movement::Left,
//                         Mode::Insert,
//                         true,
//                     );
//                     if selection.regions().len() == 1 {
//                         let delete_str = self
//                             .buffer
//                             .slice_to_cow(
//                                 selection.min_offset()..selection.max_offset(),
//                             )
//                             .to_string();
//                         if str_is_pair_left(&delete_str) {
//                             if let Some(c) = str_matching_pair(&delete_str) {
//                                 let offset = selection.max_offset();
//                                 let line = self.buffer.line_of_offset(offset);
//                                 let line_end =
//                                     self.buffer.line_end_offset(line, true);
//                                 let content = self
//                                     .buffer
//                                     .slice_to_cow(offset..line_end)
//                                     .to_string();
//                                 if content.trim().starts_with(&c.to_string()) {
//                                     let index = content
//                                         .match_indices(c)
//                                         .next()
//                                         .unwrap()
//                                         .0;
//                                     selection = Selection::region(
//                                         selection.min_offset(),
//                                         offset + index + 1,
//                                     );
//                                 }
//                             }
//                         }
//                     }
//                     selection
//                 }
//             };
//             let (selection, _) =
//                 self.edit(ctx, &selection, "", None, true, EditType::Delete);
//             self.set_cursor_after_change(selection);
//             self.update_completion(ctx);
//         }
//         LapceCommand::DeleteForeward => {
//             let selection = self.editor.cursor.edit_selection(&self.buffer);
//             let (selection, _) =
//                 self.edit(ctx, &selection, "", None, true, EditType::Delete);
//             self.set_cursor_after_change(selection);
//             self.update_completion(ctx);
//         }
//         LapceCommand::DeleteForewardAndInsert => {
//             let selection = self.editor.cursor.edit_selection(&self.buffer);
//             let (selection, _) =
//                 self.edit(ctx, &selection, "", None, true, EditType::Delete);
//             self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
//             self.update_completion(ctx);
//         }
//         LapceCommand::InsertNewLine => {
//             let selection = self.editor.cursor.edit_selection(&self.buffer);
//             if selection.regions().len() > 1 {
//                 let (selection, _) = self.edit(
//                     ctx,
//                     &selection,
//                     "\n",
//                     None,
//                     true,
//                     EditType::InsertNewline,
//                 );
//                 self.set_cursor(Cursor::new(
//                     CursorMode::Insert(selection),
//                     None,
//                 ));
//                 return;
//             };
//             self.insert_new_line(ctx, self.editor.cursor.offset());
//             self.update_completion(ctx);
//         }
//         LapceCommand::ToggleVisualMode => {
//             self.toggle_visual(VisualMode::Normal);
//         }
//         LapceCommand::ToggleLinewiseVisualMode => {
//             self.toggle_visual(VisualMode::Linewise);
//         }
//         LapceCommand::ToggleBlockwiseVisualMode => {
//             self.toggle_visual(VisualMode::Blockwise);
//         }
//         LapceCommand::CenterOfWindow => {
//             ctx.submit_command(Command::new(
//                 LAPCE_UI_COMMAND,
//                 LapceUICommand::EnsureCursorCenter,
//                 Target::Widget(self.editor.view_id),
//             ));
//         }
//         LapceCommand::ScrollDown => {
//             self.scroll(ctx, true, count.unwrap_or(1), env);
//         }
//         LapceCommand::ScrollUp => {
//             self.scroll(ctx, false, count.unwrap_or(1), env);
//         }
//         LapceCommand::PageDown => {
//             self.page_move(ctx, true, env);
//         }
//         LapceCommand::PageUp => {
//             self.page_move(ctx, false, env);
//         }
//         LapceCommand::JumpLocationBackward => {
//             self.jump_location_backward(ctx, env);
//         }
//         LapceCommand::JumpLocationForward => {
//             self.jump_location_forward(ctx, env);
//         }
//         LapceCommand::NextError => {
//             self.next_error(ctx, env);
//         }
//         LapceCommand::PreviousError => {}
//         LapceCommand::ListNext => {
//             let completion = Arc::make_mut(&mut self.completion);
//             completion.next();
//         }
//         LapceCommand::ListPrevious => {
//             let completion = Arc::make_mut(&mut self.completion);
//             completion.previous();
//         }
//         LapceCommand::JumpToNextSnippetPlaceholder => {
//             if let Some(snippet) = self.editor.snippet.as_ref() {
//                 let mut current = 0;
//                 let offset = self.editor.cursor.offset();
//                 for (i, (_, (start, end))) in snippet.iter().enumerate() {
//                     if *start <= offset && offset <= *end {
//                         current = i;
//                         break;
//                     }
//                 }

//                 let last_placeholder = current + 1 >= snippet.len() - 1;

//                 if let Some((_, (start, end))) = snippet.get(current + 1) {
//                     let mut selection = Selection::new();
//                     let region = SelRegion::new(*start, *end, None);
//                     selection.add_region(region);
//                     self.set_cursor(Cursor::new(
//                         CursorMode::Insert(selection),
//                         None,
//                     ));
//                 }

//                 if last_placeholder {
//                     Arc::make_mut(&mut self.editor).snippet = None;
//                 }
//                 self.cancel_completion();
//             }
//         }
//         LapceCommand::JumpToPrevSnippetPlaceholder => {
//             if let Some(snippet) = self.editor.snippet.as_ref() {
//                 let mut current = 0;
//                 let offset = self.editor.cursor.offset();
//                 for (i, (_, (start, end))) in snippet.iter().enumerate() {
//                     if *start <= offset && offset <= *end {
//                         current = i;
//                         break;
//                     }
//                 }

//                 if current > 0 {
//                     if let Some((_, (start, end))) = snippet.get(current - 1) {
//                         let mut selection = Selection::new();
//                         let region = SelRegion::new(*start, *end, None);
//                         selection.add_region(region);
//                         self.set_cursor(Cursor::new(
//                             CursorMode::Insert(selection),
//                             None,
//                         ));
//                     }
//                     self.cancel_completion();
//                 }
//             }
//         }
//         LapceCommand::ListSelect => {
//             let selection = self.editor.cursor.edit_selection(&self.buffer);

//             let count = self.completion.input.len();
//             let selection = if count > 0 {
//                 self.buffer.update_selection(
//                     &selection,
//                     count,
//                     &Movement::Left,
//                     Mode::Insert,
//                     true,
//                 )
//             } else {
//                 selection
//             };

//             let item = self.completion.current_item().to_owned();
//             self.cancel_completion();
//             if item.data.is_some() {
//                 let view_id = self.editor.view_id;
//                 let buffer_id = self.buffer.id;
//                 let rev = self.buffer.rev;
//                 let offset = self.editor.cursor.offset();
//                 let event_sink = ctx.get_external_handle();
//                 self.proxy.completion_resolve(
//                     buffer_id,
//                     item.clone(),
//                     Box::new(move |result| {
//                         println!("completion resolve result {:?}", result);
//                         let mut item = item.clone();
//                         if let Ok(res) = result {
//                             if let Ok(i) =
//                                 serde_json::from_value::<CompletionItem>(res)
//                             {
//                                 item = i;
//                             }
//                         };
//                         event_sink.submit_command(
//                             LAPCE_UI_COMMAND,
//                             LapceUICommand::ResolveCompletion(
//                                 buffer_id, rev, offset, item,
//                             ),
//                             Target::Widget(view_id),
//                         );
//                     }),
//                 );
//             } else {
//                 self.apply_completion_item(ctx, &item);
//             }
//         }
//         LapceCommand::NormalMode => {
//             if !self.config.lapce.modal {
//                 return;
//             }

//             let offset = match &self.editor.cursor.mode {
//                 CursorMode::Insert(selection) => {
//                     self.buffer
//                         .move_offset(
//                             selection.get_cursor_offset(),
//                             None,
//                             1,
//                             &Movement::Left,
//                             Mode::Normal,
//                         )
//                         .0
//                 }
//                 CursorMode::Visual { start, end, mode } => {
//                     self.buffer.offset_line_end(*end, false).min(*end)
//                 }
//                 CursorMode::Normal(offset) => *offset,
//             };
//             self.buffer_mut().update_edit_type();
//             let mut cursor = &mut Arc::make_mut(&mut self.editor).cursor;
//             cursor.mode = CursorMode::Normal(offset);
//             cursor.horiz = None;
//             Arc::make_mut(&mut self.editor).snippet = None;
//             self.cancel_completion();
//         }
//         LapceCommand::GotoDefinition => {
//             let offset = self.editor.cursor.offset();
//             let start_offset = self.buffer.prev_code_boundary(offset);
//             let start_position = self.buffer.offset_to_position(start_offset);
//             let event_sink = ctx.get_external_handle();
//             let buffer_id = self.buffer.id;
//             let position = self.buffer.offset_to_position(offset);
//             let proxy = self.proxy.clone();
//             let editor_view_id = self.editor.view_id;
//             self.proxy.get_definition(
//                 offset,
//                 buffer_id,
//                 position,
//                 Box::new(move |result| {
//                     if let Ok(res) = result {
//                         if let Ok(resp) =
//                             serde_json::from_value::<GotoDefinitionResponse>(res)
//                         {
//                             if let Some(location) = match resp {
//                                 GotoDefinitionResponse::Scalar(location) => {
//                                     Some(location)
//                                 }
//                                 GotoDefinitionResponse::Array(locations) => {
//                                     if locations.len() > 0 {
//                                         Some(locations[0].clone())
//                                     } else {
//                                         None
//                                     }
//                                 }
//                                 GotoDefinitionResponse::Link(location_links) => {
//                                     None
//                                 }
//                             } {
//                                 if location.range.start == start_position {
//                                     proxy.get_references(
//                                         buffer_id,
//                                         position,
//                                         Box::new(move |result| {
//                                             process_get_references(
//                                                 editor_view_id,
//                                                 offset,
//                                                 result,
//                                                 event_sink,
//                                             );
//                                         }),
//                                     );
//                                 } else {
//                                     event_sink.submit_command(
//                                         LAPCE_UI_COMMAND,
//                                         LapceUICommand::GotoDefinition(
//                                             editor_view_id,
//                                             offset,
//                                             EditorLocationNew {
//                                                 path: PathBuf::from(
//                                                     location.uri.path(),
//                                                 ),
//                                                 position: Some(
//                                                     location.range.start,
//                                                 ),
//                                                 scroll_offset: None,
//                                             },
//                                         ),
//                                         Target::Auto,
//                                     );
//                                 }
//                             }
//                         }
//                     }
//                 }),
//             );
//         }
//         LapceCommand::SourceControl => {
//             ctx.submit_command(Command::new(
//                 LAPCE_UI_COMMAND,
//                 LapceUICommand::FocusSourceControl,
//                 Target::Auto,
//             ));
//         }
//         LapceCommand::SourceControlCancel => {
//             if self.editor.editor_type == EditorType::SourceControl {
//                 ctx.submit_command(Command::new(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::FocusEditor,
//                     Target::Auto,
//                 ));
//                 println!("source control cancel");
//             }
//         }
//         LapceCommand::ShowCodeActions => {
//             if let Some(actions) = self.current_code_actions() {
//                 if actions.len() > 0 {
//                     ctx.submit_command(Command::new(
//                         LAPCE_UI_COMMAND,
//                         LapceUICommand::ShowCodeActions,
//                         Target::Auto,
//                     ));
//                 }
//             }
//         }
//         LapceCommand::SearchWholeWordForward => {
//             let offset = self.editor.cursor.offset();
//             let (start, end) = self.buffer.select_word(offset);
//             let word = self.buffer.slice_to_cow(start..end).to_string();
//             Arc::make_mut(&mut self.main_split.find)
//                 .set_find(&word, false, false, true);
//             let next = self.main_split.find.next(
//                 &self.buffer.rope,
//                 offset,
//                 false,
//                 true,
//             );
//             if let Some((start, end)) = next {
//                 self.do_move(&Movement::Offset(start), 1);
//             }
//         }
//         LapceCommand::SearchForward => {
//             let offset = self.editor.cursor.offset();
//             let next = self.main_split.find.next(
//                 &self.buffer.rope,
//                 offset,
//                 false,
//                 true,
//             );
//             if let Some((start, end)) = next {
//                 self.do_move(&Movement::Offset(start), 1);
//             }
//         }
//         LapceCommand::SearchBackward => {
//             let offset = self.editor.cursor.offset();
//             let next =
//                 self.main_split
//                     .find
//                     .next(&self.buffer.rope, offset, true, true);
//             if let Some((start, end)) = next {
//                 self.do_move(&Movement::Offset(start), 1);
//             }
//         }
//         LapceCommand::JoinLines => {
//             let offset = self.editor.cursor.offset();
//             let (line, col) = self.buffer.offset_to_line_col(offset);
//             if line < self.buffer.last_line() {
//                 let start = self.buffer.line_end_offset(line, true);
//                 let end =
//                     self.buffer.first_non_blank_character_on_line(line + 1);
//                 self.edit(
//                     ctx,
//                     &Selection::region(start, end),
//                     " ",
//                     None,
//                     false,
//                     EditType::Other,
//                 );
//             }
//         }
//         LapceCommand::Save => {
//             if !self.buffer.dirty {
//                 return;
//             }

//             let proxy = self.proxy.clone();
//             let buffer_id = self.buffer.id;
//             let rev = self.buffer.rev;
//             let path = self.buffer.path.clone();
//             let event_sink = ctx.get_external_handle();
//             let (sender, receiver) = bounded(1);
//             thread::spawn(move || {
//                 proxy.get_document_formatting(
//                     buffer_id,
//                     Box::new(move |result| {
//                         sender.send(result);
//                     }),
//                 );

//                 let result =
//                     receiver.recv_timeout(Duration::from_secs(1)).map_or_else(
//                         |e| Err(anyhow!("{}", e)),
//                         |v| v.map_err(|e| anyhow!("{:?}", e)),
//                     );
//                 event_sink.submit_command(
//                     LAPCE_UI_COMMAND,
//                     LapceUICommand::DocumentFormatAndSave(path, rev, result),
//                     Target::Auto,
//                 );
//             });
//         }
//         _ => {
//             ctx.submit_command(Command::new(
//                 LAPCE_COMMAND,
//                 cmd.clone(),
//                 Target::Auto,
//             ));
//         }
//     }
// }

//     fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
//         if self.get_mode() == Mode::Insert {
//             let mut selection = self.editor.cursor.edit_selection(&self.buffer);
//             let cursor_char =
//                 self.buffer.char_at_offset(selection.get_cursor_offset());
//
//             let mut content = c.to_string();
//             if c.chars().count() == 1 {
//                 let c = c.chars().next().unwrap();
//                 if !matching_pair_direction(c).unwrap_or(true) {
//                     if cursor_char == Some(c) {
//                         self.do_move(&Movement::Right, 1);
//                         return;
//                     } else {
//                         let offset = selection.get_cursor_offset();
//                         let line = self.buffer.line_of_offset(offset);
//                         let line_start = self.buffer.offset_of_line(line);
//                         if self.buffer.slice_to_cow(line_start..offset).trim() == ""
//                         {
//                             if let Some(c) = matching_char(c) {
//                                 if let Some(previous_offset) =
//                                     self.buffer.previous_unmatched(c, offset)
//                                 {
//                                     let previous_line =
//                                         self.buffer.line_of_offset(previous_offset);
//                                     let line_indent =
//                                         self.buffer.indent_on_line(previous_line);
//                                     content = line_indent + &content;
//                                     selection =
//                                         Selection::region(line_start, offset);
//                                 }
//                             }
//                         };
//                     }
//                 }
//             }
//
//             let (selection, _) = self.edit(
//                 ctx,
//                 &selection,
//                 &content,
//                 None,
//                 true,
//                 EditType::InsertChars,
//             );
//             let editor = Arc::make_mut(&mut self.editor);
//             editor.cursor.mode = CursorMode::Insert(selection.clone());
//             editor.cursor.horiz = None;
//             if c.chars().count() == 1 {
//                 let c = c.chars().next().unwrap();
//                 if matching_pair_direction(c).unwrap_or(false) {
//                     if cursor_char
//                         .map(|c| {
//                             let prop = get_word_property(c);
//                             prop == WordProperty::Lf
//                                 || prop == WordProperty::Space
//                                 || prop == WordProperty::Punctuation
//                         })
//                         .unwrap_or(true)
//                     {
//                         if let Some(c) = matching_char(c) {
//                             self.edit(
//                                 ctx,
//                                 &selection,
//                                 &c.to_string(),
//                                 None,
//                                 false,
//                                 EditType::InsertChars,
//                             );
//                         }
//                     }
//                 }
//             }
//             self.update_completion(ctx);
//         }
//     }
// }

fn next_in_file_errors_offset(
    position: Position,
    path: &PathBuf,
    file_diagnostics: &Vec<(&PathBuf, Vec<Position>)>,
) -> (PathBuf, Position) {
    for (current_path, positions) in file_diagnostics {
        if &path == current_path {
            for error_position in positions {
                if error_position.line > position.line
                    || (error_position.line == position.line
                        && error_position.character > position.character)
                {
                    return ((*current_path).clone(), *error_position);
                }
            }
        }
        if current_path > &path {
            return ((*current_path).clone(), positions[0]);
        }
    }
    ((*file_diagnostics[0].0).clone(), file_diagnostics[0].1[0])
}

fn process_get_references(
    editor_view_id: WidgetId,
    offset: usize,
    result: Result<Value, xi_rpc::Error>,
    event_sink: ExtEventSink,
) -> Result<()> {
    let res = result.map_err(|e| anyhow!("{:?}", e))?;
    let locations: Vec<Location> = serde_json::from_value(res)?;
    if locations.len() == 0 {
        return Ok(());
    }
    if locations.len() == 1 {
        let location = &locations[0];
        event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::GotoReference(
                editor_view_id,
                offset,
                EditorLocationNew {
                    path: PathBuf::from(location.uri.path()),
                    position: Some(location.range.start.clone()),
                    scroll_offset: None,
                },
            ),
            Target::Auto,
        );
    }
    event_sink.submit_command(
        LAPCE_UI_COMMAND,
        LapceUICommand::PaletteReferences(offset, locations),
        Target::Auto,
    );
    Ok(())
}

pub fn hex_to_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches("#");
    let (r, g, b, a) = match hex.len() {
        3 => (
            format!("{}{}", &hex[0..0], &hex[0..0]),
            format!("{}{}", &hex[1..1], &hex[1..1]),
            format!("{}{}", &hex[2..2], &hex[2..2]),
            "ff".to_string(),
        ),
        6 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            "ff".to_string(),
        ),
        8 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            hex[6..8].to_string(),
        ),
        _ => return Err(anyhow!("invalid hex color")),
    };

    Ok(Color::rgba8(
        u8::from_str_radix(&r, 16)?,
        u8::from_str_radix(&g, 16)?,
        u8::from_str_radix(&b, 16)?,
        u8::from_str_radix(&a, 16)?,
    ))
}

fn buffer_receive_update(
    update: BufferUpdate,
    parsers: &mut HashMap<LapceLanguage, Parser>,
    highlighter: &mut Highlighter,
    highlight_configs: &mut HashMap<
        LapceLanguage,
        (HighlightConfiguration, Vec<String>),
    >,
    event_sink: &ExtEventSink,
    tab_id: WidgetId,
) {
    if !parsers.contains_key(&update.language) {
        let parser = new_parser(update.language);
        parsers.insert(update.language, parser);
    }
    let parser = parsers.get_mut(&update.language).unwrap();
    if let Some(tree) = parser.parse(
        update.rope.slice_to_cow(0..update.rope.len()).as_bytes(),
        None,
    ) {
        event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateSyntaxTree {
                id: update.id,
                path: update.path.clone(),
                rev: update.rev,
                tree,
            },
            Target::Widget(tab_id),
        );
    }

    if !update.semantic_tokens {
        if !highlight_configs.contains_key(&update.language) {
            let (highlight_config, highlight_names) =
                new_highlight_config(update.language);
            highlight_configs
                .insert(update.language, (highlight_config, highlight_names));
        }
        let (highlight_config, highlight_names) =
            highlight_configs.get(&update.language).unwrap();
        let mut current_hl: Option<Highlight> = None;
        let mut highlights = SpansBuilder::new(update.rope.len());
        for hightlight in highlighter
            .highlight(
                highlight_config,
                update.rope.slice_to_cow(0..update.rope.len()).as_bytes(),
                None,
                |_| None,
            )
            .unwrap()
        {
            if let Ok(highlight) = hightlight {
                match highlight {
                    HighlightEvent::Source { start, end } => {
                        if let Some(hl) = current_hl {
                            if let Some(hl) = highlight_names.get(hl.0) {
                                highlights.add_span(
                                    Interval::new(start, end),
                                    Style {
                                        fg_color: Some(hl.to_string()),
                                    },
                                );
                            }
                        }
                    }
                    HighlightEvent::HighlightStart(hl) => {
                        current_hl = Some(hl);
                    }
                    HighlightEvent::HighlightEnd => current_hl = None,
                }
            }
        }
        let highlights = highlights.build();
        event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::UpdateStyle {
                id: update.id,
                path: update.path,
                rev: update.rev,
                highlights,
                semantic_tokens: false,
            },
            Target::Widget(tab_id),
        );
    }
}

fn str_is_pair_left(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        if matching_pair_direction(c).unwrap_or(false) {
            return true;
        }
    }
    false
}

fn str_is_pair_right(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return !matching_pair_direction(c).unwrap_or(true);
    }
    false
}

fn str_matching_pair(c: &str) -> Option<char> {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return matching_char(c);
    }
    None
}

fn progress_term_event() {}
