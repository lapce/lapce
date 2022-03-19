use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use druid::{
    piet::{PietText, PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    theme, Color, Command, Data, Env, EventCtx, ExtEventSink, FontFamily, Lens,
    Point, Rect, Size, Target, Vec2, WidgetId, WindowId,
};

use lapce_proxy::{
    dispatch::{FileDiff, FileNodeItem},
    plugin::PluginDescription,
    terminal::TermId,
};
use lsp_types::{
    CodeActionOrCommand, CodeActionResponse, CompletionItem, CompletionTextEdit,
    Diagnostic, DiagnosticSeverity, Location, Position, ProgressToken, TextEdit,
};
use notify::Watcher;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tree_sitter::Parser;
use tree_sitter_highlight::{
    Highlight, HighlightConfiguration, HighlightEvent, Highlighter,
};
use xi_rope::{spans::SpansBuilder, Interval, RopeDelta, Transformer};

use crate::{
    buffer::{
        has_unmatched_pair, matching_char, matching_pair_direction, Buffer,
        BufferContent, BufferUpdate, EditType, LocalBufferKind, Style, UpdateEvent,
    },
    command::{
        CommandTarget, EnsureVisiblePosition, LapceCommandNew, LapceUICommand,
        LapceWorkbenchCommand, LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND,
    },
    completion::{CompletionData, Snippet},
    config::{Config, ConfigWatcher, GetConfig},
    db::{
        EditorInfo, EditorTabChildInfo, EditorTabInfo, LapceDb, SplitContentInfo,
        SplitInfo, TabsInfo, WindowInfo, WorkspaceInfo,
    },
    editor::{EditorLocationNew, LapceEditorBufferData, TabRect},
    explorer::FileExplorerData,
    find::Find,
    keypress::KeyPressData,
    language::{new_highlight_config, new_parser, LapceLanguage, SCOPES},
    menu::MenuData,
    movement::{Cursor, CursorMode, InsertDrift, Movement, SelRegion, Selection},
    palette::{PaletteData, PaletteType, PaletteViewData},
    panel::PanelPosition,
    picker::FilePickerData,
    plugin::PluginData,
    problem::ProblemData,
    proxy::{LapceProxy, ProxyStatus, TermEvent},
    search::SearchData,
    settings::LapceSettingsPanelData,
    source_control::SourceControlData,
    split::{SplitDirection, SplitMoveDirection},
    state::{LapceWorkspace, LapceWorkspaceType, Mode, VisualMode},
    svg::get_svg,
    terminal::TerminalSplitData,
};

#[derive(Clone, Data)]
pub struct LapceData {
    pub windows: im::HashMap<WindowId, LapceWindowData>,
    pub keypress: Arc<KeyPressData>,
    pub db: Arc<LapceDb>,
}

impl LapceData {
    pub fn load(event_sink: ExtEventSink) -> Self {
        let db = Arc::new(LapceDb::new().unwrap());
        let mut windows = im::HashMap::new();
        let config = Config::load(&LapceWorkspace::default()).unwrap_or_default();
        let keypress = Arc::new(KeyPressData::new(&config, event_sink.clone()));

        if let Ok(app) = db.get_app() {
            for info in app.windows.iter() {
                let window = LapceWindowData::new(
                    keypress.clone(),
                    event_sink.clone(),
                    info,
                    db.clone(),
                );
                windows.insert(window.window_id, window);
            }
        }

        if windows.is_empty() {
            let info = db.get_last_window_info().unwrap_or_else(|_| WindowInfo {
                size: Size::new(800.0, 600.0),
                pos: Point::new(0.0, 0.0),
                tabs: TabsInfo {
                    active_tab: 0,
                    workspaces: vec![],
                },
            });
            let window = LapceWindowData::new(
                keypress.clone(),
                event_sink.clone(),
                &info,
                db.clone(),
            );
            windows.insert(window.window_id, window);
        }

        thread::spawn(move || {
            if let Ok(plugins) = LapceData::load_plugin_descriptions() {
                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdatePluginDescriptions(plugins),
                    Target::Auto,
                );
            }
        });
        Self {
            windows,
            keypress,
            db,
        }
    }

    pub fn reload_env(&self, env: &mut Env) {
        env.set(theme::SCROLLBAR_WIDTH, 10.0);
        env.set(theme::SCROLLBAR_MAX_OPACITY, 0.7);
    }

    fn load_plugin_descriptions() -> Result<Vec<PluginDescription>> {
        let plugins: Vec<String> =
            reqwest::blocking::get("https://lapce.github.io/plugins.json")?
                .json()?;
        let plugins: Vec<PluginDescription> = plugins
            .iter()
            .filter_map(|plugin| LapceData::load_plgin_description(plugin).ok())
            .collect();
        Ok(plugins)
    }

    fn load_plgin_description(plugin: &str) -> Result<PluginDescription> {
        let url = format!(
            "https://raw.githubusercontent.com/{}/master/plugin.toml",
            plugin
        );
        let content = reqwest::blocking::get(url)?.text()?;
        let plugin: PluginDescription = toml::from_str(&content)?;
        Ok(plugin)
    }
}

#[derive(Clone)]
pub struct LapceWindowData {
    pub window_id: WindowId,
    pub tabs: im::HashMap<WidgetId, LapceTabData>,
    pub tabs_order: Arc<Vec<WidgetId>>,
    pub active: usize,
    pub active_id: WidgetId,
    pub keypress: Arc<KeyPressData>,
    pub config: Arc<Config>,
    pub plugins: Arc<Vec<PluginDescription>>,
    pub db: Arc<LapceDb>,
    pub watcher: Arc<notify::RecommendedWatcher>,
    pub menu: Arc<MenuData>,
    pub size: Size,
    pub pos: Point,
}

impl Data for LapceWindowData {
    fn same(&self, other: &Self) -> bool {
        self.active == other.active
            && self.tabs.same(&other.tabs)
            && self.menu.same(&other.menu)
            && self.size.same(&other.size)
            && self.pos.same(&other.pos)
            && self.keypress.same(&other.keypress)
    }
}

impl LapceWindowData {
    pub fn new(
        keypress: Arc<KeyPressData>,
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
                event_sink.clone(),
            );
            tabs.insert(tab_id, tab);
            tabs_order.push(tab_id);
            active_tab_id = tab_id;
        }

        let config = Arc::new(
            Config::load(&LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: None,
                last_open: 0,
            })
            .unwrap_or_default(),
        );
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(active_tab_id),
        );

        let mut watcher =
            notify::recommended_watcher(ConfigWatcher::new(event_sink)).unwrap();
        if let Some(path) = Config::settings_file() {
            let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
        }
        if let Some(path) = KeyPressData::file() {
            let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
        }
        let menu = MenuData::new();

        Self {
            window_id,
            tabs,
            tabs_order: Arc::new(tabs_order),
            active,
            plugins: Arc::new(Vec::new()),
            active_id: active_tab_id,
            keypress,
            config,
            db,
            watcher: Arc::new(watcher),
            menu: Arc::new(menu),
            size: info.size,
            pos: info.pos,
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
                if tab.id == self.active_id {
                    active_tab = i;
                }
                (*tab.workspace).clone()
            })
            .collect();
        WindowInfo {
            size: self.size,
            pos: self.pos,
            tabs: TabsInfo {
                active_tab,
                workspaces,
            },
        }
    }
}

#[derive(Clone)]
pub struct EditorDiagnostic {
    pub range: Option<(usize, usize)>,
    pub diagnositc: Diagnostic,
}

#[derive(Clone, Copy, PartialEq, Data, Serialize, Deserialize, Hash, Eq)]
pub enum PanelKind {
    FileExplorer,
    SourceControl,
    Plugin,
    Terminal,
    Search,
    Problem,
}

impl PanelKind {
    pub fn svg_name(&self) -> String {
        match &self {
            PanelKind::FileExplorer => "file-explorer.svg".to_string(),
            PanelKind::SourceControl => "git-icon.svg".to_string(),
            PanelKind::Plugin => "plugin-icon.svg".to_string(),
            PanelKind::Terminal => "terminal.svg".to_string(),
            PanelKind::Search => "search.svg".to_string(),
            PanelKind::Problem => "error.svg".to_string(),
        }
    }

    pub fn svg(&self) -> Svg {
        get_svg(&self.svg_name()).unwrap()
    }
}

#[derive(Clone)]
pub struct PanelData {
    pub active: PanelKind,
    pub widgets: Vec<PanelKind>,
    pub shown: bool,
    pub maximized: bool,
}

impl PanelData {
    pub fn is_shown(&self) -> bool {
        self.shown && !self.widgets.is_empty()
    }

    pub fn is_maximized(&self) -> bool {
        self.maximized && !self.widgets.is_empty()
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
    Editor,
    Panel(PanelKind),
    FilePicker,
}

#[derive(Clone)]
pub enum DragContent {
    EditorTab(WidgetId, usize, EditorTabChild, TabRect),
}

#[derive(Clone, Lens)]
pub struct LapceTabData {
    pub id: WidgetId,
    pub window_id: WindowId,
    pub workspace: Arc<LapceWorkspace>,
    pub main_split: LapceMainSplitData,
    pub completion: Arc<CompletionData>,
    pub terminal: Arc<TerminalSplitData>,
    pub palette: Arc<PaletteData>,
    pub find: Arc<Find>,
    pub source_control: Arc<SourceControlData>,
    pub problem: Arc<ProblemData>,
    pub search: Arc<SearchData>,
    pub plugin: Arc<PluginData>,
    pub picker: Arc<FilePickerData>,
    pub plugins: Arc<Vec<PluginDescription>>,
    pub installed_plugins: Arc<HashMap<String, PluginDescription>>,
    pub file_explorer: Arc<FileExplorerData>,
    pub proxy: Arc<LapceProxy>,
    pub proxy_status: Arc<ProxyStatus>,
    pub keypress: Arc<KeyPressData>,
    pub settings: Arc<LapceSettingsPanelData>,
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
    pub drag: Arc<Option<(Vec2, DragContent)>>,
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
            && self.proxy_status.same(&other.proxy_status)
            && self.find.same(&other.find)
            && self.progresses.ptr_eq(&other.progresses)
            && self.file_explorer.same(&other.file_explorer)
            && self.plugin.same(&other.plugin)
            && self.problem.same(&other.problem)
            && self.search.same(&other.search)
            && self.installed_plugins.same(&other.installed_plugins)
            && self.picker.same(&other.picker)
            && self.drag.same(&other.drag)
            && self.keypress.same(&other.keypress)
            && self.settings.same(&other.settings)
    }
}

impl GetConfig for LapceTabData {
    fn get_config(&self) -> &Config {
        &self.config
    }
}

impl LapceTabData {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        workspace: LapceWorkspace,
        db: Arc<LapceDb>,
        keypress: Arc<KeyPressData>,
        event_sink: ExtEventSink,
    ) -> Self {
        let config = Arc::new(Config::load(&workspace).unwrap_or_default());

        let workspace_info = if workspace.path.is_some() {
            db.get_workspace_info(&workspace).ok()
        } else {
            None
        };

        let (update_sender, update_receiver) = unbounded();
        let update_sender = Arc::new(update_sender);
        let (term_sender, term_receiver) = unbounded();
        let proxy = Arc::new(LapceProxy::new(
            tab_id,
            workspace.clone(),
            term_sender.clone(),
            event_sink.clone(),
        ));
        let palette = Arc::new(PaletteData::new(proxy.clone()));
        let completion = Arc::new(CompletionData::new());
        let source_control = Arc::new(SourceControlData::new());
        let settings = Arc::new(LapceSettingsPanelData::new());
        let plugin = Arc::new(PluginData::new());
        let file_explorer = Arc::new(FileExplorerData::new(
            tab_id,
            workspace.clone(),
            proxy.clone(),
            event_sink.clone(),
        ));
        let search = Arc::new(SearchData::new());
        let file_picker = Arc::new(FilePickerData::new());

        let mut main_split = LapceMainSplitData::new(
            tab_id,
            workspace_info.as_ref(),
            palette.preview_editor,
            update_sender.clone(),
            proxy.clone(),
            &config,
            event_sink.clone(),
            Arc::new(workspace.clone()),
            db.clone(),
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
            file_picker.editor_view_id,
            None,
            LocalBufferKind::FilePicker,
            &config,
            event_sink.clone(),
        );

        let terminal = Arc::new(TerminalSplitData::new(proxy.clone()));
        let problem = Arc::new(ProblemData::new());

        let mut panels = im::HashMap::new();
        panels.insert(
            PanelPosition::LeftTop,
            Arc::new(PanelData {
                active: PanelKind::FileExplorer,
                widgets: vec![
                    PanelKind::FileExplorer,
                    PanelKind::SourceControl,
                    PanelKind::Plugin,
                ],
                shown: true,
                maximized: false,
            }),
        );
        panels.insert(
            PanelPosition::BottomLeft,
            Arc::new(PanelData {
                active: PanelKind::Terminal,
                widgets: vec![
                    PanelKind::Terminal,
                    PanelKind::Search,
                    PanelKind::Problem,
                ],
                shown: true,
                maximized: false,
            }),
        );
        let focus = (*main_split.active).unwrap_or(*main_split.split_id);
        let mut tab = Self {
            id: tab_id,
            window_id,
            workspace: Arc::new(workspace),
            focus,
            main_split,
            completion,
            terminal,
            plugin,
            problem,
            search,
            plugins: Arc::new(Vec::new()),
            installed_plugins: Arc::new(HashMap::new()),
            find: Arc::new(Find::new(0)),
            picker: file_picker,
            source_control,
            file_explorer,
            term_rx: Some(term_receiver),
            term_tx: Arc::new(term_sender),
            palette,
            proxy,
            settings,
            proxy_status: Arc::new(ProxyStatus::Connecting),
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
            drag: Arc::new(None),
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
            split: main_split_data.split_info(self, self.config.editor.tab_width),
        }
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
            });
        }

        if let Some(receiver) = Arc::make_mut(&mut self.palette).receiver.take() {
            let widget_id = self.palette.widget_id;
            thread::spawn(move || {
                PaletteViewData::update_process(receiver, widget_id, event_sink);
            });
        }
    }

    pub fn editor_view_content(
        &self,
        editor_view_id: WidgetId,
    ) -> LapceEditorBufferData {
        let editor = self.main_split.editors.get(&editor_view_id).unwrap();
        let buffer = match &editor.content {
            BufferContent::File(path) => {
                self.main_split.open_files.get(path).unwrap().clone()
            }
            BufferContent::Local(kind) => {
                self.main_split.local_buffers.get(kind).unwrap().clone()
            }
        };
        LapceEditorBufferData {
            view_id: editor_view_id,
            main_split: self.main_split.clone(),
            completion: self.completion.clone(),
            source_control: self.source_control.clone(),
            proxy: self.proxy.clone(),
            find: self.find.clone(),
            buffer,
            editor: editor.clone(),
            config: self.config.clone(),
            workspace: self.workspace.clone(),
        }
    }

    #[allow(unused_variables)]
    pub fn code_action_size(&self, text: &mut PietText, env: &Env) -> Size {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return Size::ZERO,
        };

        match &editor.content {
            BufferContent::Local(_) => Size::ZERO,
            BufferContent::File(path) => {
                let buffer = self.main_split.open_files.get(path).unwrap();
                let offset = editor.cursor.offset();
                let prev_offset = buffer.prev_code_boundary(offset);
                let empty_vec = Vec::new();
                let code_actions =
                    buffer.code_actions.get(&prev_offset).unwrap_or(&empty_vec);

                let action_text_layouts: Vec<PietTextLayout> = code_actions
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

                        text.new_text_layout(title)
                            .font(FontFamily::SYSTEM_UI, 14.0)
                            .build()
                            .unwrap()
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

    pub fn panel_position(&self, kind: PanelKind) -> Option<PanelPosition> {
        for (pos, panels) in self.panels.iter() {
            if panels.widgets.contains(&kind) {
                return Some(pos.clone());
            }
        }
        None
    }

    pub fn update_from_editor_buffer_data(
        &mut self,
        editor_buffer_data: LapceEditorBufferData,
        editor: &Arc<LapceEditorData>,
        buffer: &Arc<Buffer>,
    ) {
        self.completion = editor_buffer_data.completion.clone();
        self.main_split = editor_buffer_data.main_split.clone();
        self.find = editor_buffer_data.find.clone();
        if !editor_buffer_data.editor.same(editor) {
            self.main_split
                .editors
                .insert(editor.view_id, editor_buffer_data.editor);
        }
        if !editor_buffer_data.buffer.same(buffer) {
            match &buffer.content {
                BufferContent::File(path) => {
                    self.main_split
                        .open_files
                        .insert(path.clone(), editor_buffer_data.buffer);
                }
                BufferContent::Local(kind) => {
                    self.main_split
                        .local_buffers
                        .insert(kind.clone(), editor_buffer_data.buffer);
                }
            }
        }
    }

    #[allow(unused_variables)]
    pub fn code_action_origin(
        &self,
        text: &mut PietText,
        tab_size: Size,
        config: &Config,
    ) -> Point {
        let line_height = self.config.editor.line_height as f64;
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return Point::ZERO,
        };

        match &editor.content {
            BufferContent::Local(_) => {
                editor.window_origin - self.window_origin.to_vec2()
            }
            BufferContent::File(path) => {
                let buffer = self.main_split.open_files.get(path).unwrap();
                let offset = editor.cursor.offset();
                let (line, col) =
                    buffer.offset_to_line_col(offset, self.config.editor.tab_width);
                let width = config.editor_text_width(text, "W");
                let x = col as f64 * width;
                let y = (line + 1) as f64 * line_height;

                editor.window_origin - self.window_origin.to_vec2() + Vec2::new(x, y)
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
        let editor = match editor {
            Some(editor) => editor,
            None => return Point::ZERO,
        };

        match &editor.content {
            BufferContent::Local(_) => {
                editor.window_origin - self.window_origin.to_vec2()
            }
            BufferContent::File(path) => {
                let buffer = self.main_split.open_files.get(path).unwrap();
                let offset = self.completion.offset;
                let (line, col) =
                    buffer.offset_to_line_col(offset, self.config.editor.tab_width);
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

    #[allow(unused_variables)]
    pub fn run_workbench_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceWorkbenchCommand,
        data: Option<serde_json::Value>,
        count: Option<usize>,
        env: &Env,
    ) {
        match command {
            LapceWorkbenchCommand::OpenFolder => {
                if !self.workspace.kind.is_remote() {
                    let event_sink = ctx.get_external_handle();
                    let tab_id = self.id;
                    thread::spawn(move || {
                        let dir = directories::UserDirs::new()
                            .and_then(|u| {
                                u.home_dir().to_str().map(|s| s.to_string())
                            })
                            .unwrap_or_else(|| ".".to_string());
                        if let Some(folder) = tinyfiledialogs::select_folder_dialog(
                            "Open folder",
                            &dir,
                        ) {
                            let path = PathBuf::from(folder);
                            let workspace = LapceWorkspace {
                                kind: LapceWorkspaceType::Local,
                                path: Some(path),
                                last_open: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            };

                            if let Err(err) = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::SetWorkspace(workspace),
                                Target::Auto,
                            ) {
                                // TODO! Handle or log `err`
                                todo!()
                            }
                        }
                    });
                } else {
                    let picker = Arc::make_mut(&mut self.picker);
                    picker.active = true;
                    if let Some(node) = picker.get_file_node(&picker.pwd) {
                        if !node.read {
                            let tab_id = self.id;
                            let path = node.path_buf.clone();
                            let event_sink = ctx.get_external_handle();
                            self.proxy.read_dir(
                                &node.path_buf,
                                Box::new(move |result| {
                                    if let Ok(res) = result {
                                        let resp: Result<
                                            Vec<FileNodeItem>,
                                            serde_json::Error,
                                        > = serde_json::from_value(res);
                                        if let Ok(items) = resp {
                                            let _ = event_sink.submit_command(
                                                LAPCE_UI_COMMAND,
                                                LapceUICommand::UpdatePickerItems(
                                                    path,
                                                    items
                                                        .iter()
                                                        .map(|item| {
                                                            (
                                                                item.path_buf
                                                                    .clone(),
                                                                item.clone(),
                                                            )
                                                        })
                                                        .collect(),
                                                ),
                                                Target::Widget(tab_id),
                                            );
                                        }
                                    }
                                }),
                            );
                        }
                    }
                }
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
            LapceWorkbenchCommand::OpenLogFile => {
                if let Some(path) = Config::log_file() {
                    let editor_view_id = self.main_split.active.clone();
                    self.main_split.jump_to_location(
                        ctx,
                        *editor_view_id,
                        EditorLocationNew {
                            path,
                            position: None,
                            scroll_offset: None,
                            hisotry: None,
                        },
                        &self.config,
                    );
                }
            }
            LapceWorkbenchCommand::OpenSettings => {
                let settings = Arc::make_mut(&mut self.settings);
                settings.shown = true;
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ShowSettings,
                    Target::Widget(self.settings.panel_widget_id),
                ));
            }
            LapceWorkbenchCommand::OpenSettingsFile => {
                if let Some(path) = Config::settings_file() {
                    let editor_view_id = self.main_split.active.clone();
                    self.main_split.jump_to_location(
                        ctx,
                        *editor_view_id,
                        EditorLocationNew {
                            path,
                            position: None,
                            scroll_offset: None,
                            hisotry: None,
                        },
                        &self.config,
                    );
                }
            }
            LapceWorkbenchCommand::OpenKeyboardShortcuts => {
                let settings = Arc::make_mut(&mut self.settings);
                settings.shown = true;
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ShowKeybindings,
                    Target::Widget(self.settings.panel_widget_id),
                ));
            }
            LapceWorkbenchCommand::OpenKeyboardShortcutsFile => {
                if let Some(path) = KeyPressData::file() {
                    let editor_view_id = self.main_split.active.clone();
                    self.main_split.jump_to_location(
                        ctx,
                        *editor_view_id,
                        EditorLocationNew {
                            path,
                            position: None,
                            scroll_offset: None,
                            hisotry: None,
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
            LapceWorkbenchCommand::NewWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NewWindow(self.window_id),
                    Target::Global,
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
                self.toggle_panel(ctx, PanelKind::Terminal);
            }
            LapceWorkbenchCommand::ToggleMaximizedPanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        for (_, panel) in self.panels.iter_mut() {
                            if panel.widgets.contains(&kind) {
                                if panel.active == kind {
                                    let panel = Arc::make_mut(panel);
                                    panel.maximized = !panel.maximized;
                                }
                                break;
                            }
                        }
                    }
                } else {
                    let panel = self.panels.get_mut(&self.panel_active).unwrap();
                    let panel = Arc::make_mut(panel);
                    panel.maximized = !panel.maximized;
                }
            }
            LapceWorkbenchCommand::FocusEditor => {
                if let Some(active) = *self.main_split.active {
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
                    Target::Widget(self.terminal.active),
                ));
            }
            LapceWorkbenchCommand::ToggleSourceControl => {
                self.toggle_panel(ctx, PanelKind::SourceControl);
            }
            LapceWorkbenchCommand::TogglePlugin => {
                self.toggle_panel(ctx, PanelKind::Plugin);
            }
            LapceWorkbenchCommand::ToggleFileExplorer => {
                self.toggle_panel(ctx, PanelKind::FileExplorer);
            }
            LapceWorkbenchCommand::ToggleSearch => {
                self.toggle_panel(ctx, PanelKind::Search);
            }
            LapceWorkbenchCommand::ToggleProblem => {
                self.toggle_panel(ctx, PanelKind::Problem);
            }
            LapceWorkbenchCommand::TogglePanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.toggle_panel(ctx, kind);
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
            LapceWorkbenchCommand::SourceControlCommit => {
                let diffs: Vec<FileDiff> = self
                    .source_control
                    .file_diffs
                    .iter()
                    .filter_map(
                        |(diff, checked)| {
                            if *checked {
                                Some(diff.clone())
                            } else {
                                None
                            }
                        },
                    )
                    .collect();
                if diffs.is_empty() {
                    return;
                }
                let buffer = self
                    .main_split
                    .local_buffers
                    .get_mut(&LocalBufferKind::SourceControl)
                    .unwrap();
                let message = buffer.rope.to_string();
                let message = message.trim();
                if message.is_empty() {
                    return;
                }
                self.proxy.git_commit(message, diffs);
                Arc::make_mut(buffer).load_content("");
                let editor = self
                    .main_split
                    .editors
                    .get_mut(&self.source_control.editor_view_id)
                    .unwrap();
                Arc::make_mut(editor).cursor = if self.config.lapce.modal {
                    Cursor::new(CursorMode::Normal(0), None)
                } else {
                    Cursor::new(CursorMode::Insert(Selection::caret(0)), None)
                };
            }
            LapceWorkbenchCommand::CheckoutBranch => {}
            LapceWorkbenchCommand::ConnectSshHost => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::SshHost)),
                    Target::Widget(self.palette.widget_id),
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
                    self.run_workbench_command(
                        ctx,
                        &cmd,
                        command.data.clone(),
                        count,
                        env,
                    );
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

    #[allow(unused_variables)]
    pub fn terminal_update_process(
        tab_id: WidgetId,
        palette_widget_id: WidgetId,
        receiver: Receiver<(TermId, TermEvent)>,
        event_sink: ExtEventSink,
        workspace: Arc<LapceWorkspace>,
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
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::RequestPaint,
                                    Target::Widget(tab_id),
                                );
                            }
                        } else {
                            last_redraw = std::time::Instant::now();
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

    fn hide_panel(&mut self, ctx: &mut EventCtx, kind: PanelKind) {
        for (_, panel) in self.panels.iter_mut() {
            if panel.widgets.contains(&kind) {
                let panel = Arc::make_mut(panel);
                panel.shown = false;
                break;
            }
        }
        if let Some(active) = *self.main_split.active {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(active),
            ));
        }
    }

    fn show_panel(&mut self, ctx: &mut EventCtx, kind: PanelKind) {
        for (_, panel) in self.panels.iter_mut() {
            for k in panel.widgets.clone() {
                if k == kind {
                    let panel = Arc::make_mut(panel);
                    panel.shown = true;
                    panel.active = k;
                    let focus_id = match kind {
                        PanelKind::FileExplorer => self.file_explorer.widget_id,
                        PanelKind::SourceControl => self.source_control.active,
                        PanelKind::Plugin => self.plugin.widget_id,
                        PanelKind::Terminal => self.terminal.widget_id,
                        PanelKind::Search => self.search.active,
                        PanelKind::Problem => self.problem.widget_id,
                    };
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(focus_id),
                    ));
                    break;
                }
            }
        }
    }

    fn toggle_panel(&mut self, ctx: &mut EventCtx, kind: PanelKind) {
        if self.focus_area == FocusArea::Panel(kind) {
            self.hide_panel(ctx, kind);
        } else {
            self.show_panel(ctx, kind);
        }
    }

    pub fn buffer_update_process(
        tab_id: WidgetId,
        receiver: Receiver<UpdateEvent>,
        event_sink: ExtEventSink,
    ) {
        let mut parsers = HashMap::new();
        let mut highlighter = Highlighter::new();
        let mut highlight_configs = HashMap::new();
        loop {
            match receiver.recv() {
                Err(_) => return,
                Ok(event) => {
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
                            let mut highlights =
                                SpansBuilder::new(update.rope.len());
                            for (start, end, hl) in tokens {
                                highlights.add_span(
                                    Interval::new(start, end),
                                    Style {
                                        fg_color: Some(hl.to_string()),
                                    },
                                );
                            }
                            let highlights = highlights.build();
                            let _ = event_sink.submit_command(
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

    pub fn read_picker_pwd(&mut self, ctx: &mut EventCtx) {
        let path = self.picker.pwd.clone();
        let event_sink = ctx.get_external_handle();
        let tab_id = self.id;
        self.proxy.read_dir(
            &path.clone(),
            Box::new(move |result| {
                if let Ok(res) = result {
                    let resp: Result<Vec<FileNodeItem>, serde_json::Error> =
                        serde_json::from_value(res);
                    if let Ok(items) = resp {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdatePickerItems(
                                path,
                                items
                                    .iter()
                                    .map(|item| {
                                        (item.path_buf.clone(), item.clone())
                                    })
                                    .collect(),
                            ),
                            Target::Widget(tab_id),
                        );
                    }
                }
            }),
        );
    }

    pub fn set_picker_pwd(&mut self, pwd: PathBuf) {
        let picker = Arc::make_mut(&mut self.picker);
        picker.pwd = pwd.clone();
        if let Some(s) = pwd.to_str() {
            let buffer = self
                .main_split
                .local_buffers
                .get_mut(&LocalBufferKind::FilePicker)
                .unwrap();
            let buffer = Arc::make_mut(buffer);
            buffer.load_content(s);
            let editor = self
                .main_split
                .editors
                .get_mut(&self.picker.editor_view_id)
                .unwrap();
            let editor = Arc::make_mut(editor);
            editor.cursor = if self.config.lapce.modal {
                Cursor::new(
                    CursorMode::Normal(buffer.line_end_offset(0, false)),
                    None,
                )
            } else {
                Cursor::new(
                    CursorMode::Insert(Selection::caret(
                        buffer.line_end_offset(0, true),
                    )),
                    None,
                )
            };
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
        f(tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceTabData) -> V>(
        &self,
        data: &mut LapceWindowData,
        f: F,
    ) -> V {
        let mut tab = data.tabs.get(&self.0).unwrap().clone();
        tab.keypress = data.keypress.clone();
        tab.plugins = data.plugins.clone();
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
        f(tab)
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

#[derive(Clone, Debug, PartialEq)]
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

    pub fn content_info(
        &self,
        data: &LapceTabData,
        tab_width: usize,
    ) -> SplitContentInfo {
        match &self {
            SplitContent::EditorTab(widget_id) => {
                let editor_tab_data =
                    data.main_split.editor_tabs.get(widget_id).unwrap();
                SplitContentInfo::EditorTab(
                    editor_tab_data.tab_info(data, tab_width),
                )
            }
            SplitContent::Split(split_id) => {
                let split_data = data.main_split.splits.get(split_id).unwrap();
                SplitContentInfo::Split(split_data.split_info(data, tab_width))
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

#[derive(Clone, Debug, PartialEq)]
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
    pub fn split_info(&self, data: &LapceTabData, tab_width: usize) -> SplitInfo {
        let info = SplitInfo {
            direction: self.direction,
            children: self
                .children
                .iter()
                .map(|child| child.content_info(data, tab_width))
                .collect(),
        };
        info
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

    #[allow(dead_code)]
    last_deletes: [RegisterData; 10],

    #[allow(dead_code)]
    newest_delete: usize,
}

impl Register {
    pub fn add_delete(&mut self, data: RegisterData) {
        self.unamed = data;
    }

    pub fn add_yank(&mut self, data: RegisterData) {
        self.unamed = data.clone();
        self.last_yank = data;
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
    pub open_files: im::HashMap<PathBuf, Arc<Buffer>>,
    pub splits: im::HashMap<WidgetId, Arc<SplitData>>,
    pub local_buffers: im::HashMap<LocalBufferKind, Arc<Buffer>>,
    pub update_sender: Arc<Sender<UpdateEvent>>,
    pub register: Arc<Register>,
    pub proxy: Arc<LapceProxy>,
    pub palette_preview_editor: Arc<WidgetId>,
    pub show_code_actions: bool,
    pub current_code_actions: usize,
    pub diagnostics: im::HashMap<PathBuf, Arc<Vec<EditorDiagnostic>>>,
    pub error_count: usize,
    pub warning_count: usize,
    pub workspace: Arc<LapceWorkspace>,
    pub db: Arc<LapceDb>,
}

impl LapceMainSplitData {
    pub fn active_editor(&self) -> Option<&LapceEditorData> {
        match *self.active {
            Some(active) => match self.editors.get(&active) {
                Some(editor) => Some(editor),
                None => None,
            },
            None => None,
        }
    }

    // pub fn active_editor_mut(&mut self) -> &mut LapceEditorData {
    //     Arc::make_mut(self.editors.get_mut(&self.active).unwrap())
    // }

    pub fn editor_buffer(&self, editor_view_id: WidgetId) -> Arc<Buffer> {
        let editor = self.editors.get(&editor_view_id).unwrap();
        let buffer = match &editor.content {
            BufferContent::File(path) => self.open_files.get(path).unwrap().clone(),
            BufferContent::Local(kind) => {
                self.local_buffers.get(kind).unwrap().clone()
            }
        };
        buffer
    }

    pub fn document_format(
        &mut self,
        ctx: &mut EventCtx,
        path: &Path,
        rev: u64,
        result: &Result<Value>,
        config: &Config,
    ) {
        let buffer = self.open_files.get(path).unwrap();
        if buffer.rev != rev {
            return;
        }

        if let Ok(res) = result {
            let edits: Result<Vec<TextEdit>, serde_json::Error> =
                serde_json::from_value(res.clone());
            if let Ok(edits) = edits {
                if !edits.is_empty() {
                    let buffer = self.open_files.get_mut(path).unwrap();

                    let edits: Vec<(Selection, String)> = edits
                        .iter()
                        .map(|edit| {
                            let selection = Selection::region(
                                buffer.offset_of_position(
                                    &edit.range.start,
                                    config.editor.tab_width,
                                ),
                                buffer.offset_of_position(
                                    &edit.range.end,
                                    config.editor.tab_width,
                                ),
                            );
                            (selection, edit.new_text.clone())
                        })
                        .collect();

                    self.edit(
                        ctx,
                        path,
                        edits.iter().map(|(s, c)| (s, c.as_ref())).collect(),
                        EditType::Other,
                        config,
                    );
                }
            }
        }
    }

    pub fn document_format_and_save(
        &mut self,
        ctx: &mut EventCtx,
        path: &Path,
        rev: u64,
        result: &Result<Value>,
        config: &Config,
    ) {
        self.document_format(ctx, path, rev, result, config);

        let buffer = self.open_files.get(path).unwrap();
        let rev = buffer.rev;
        let buffer_id = buffer.id;
        let event_sink = ctx.get_external_handle();
        let path = PathBuf::from(path);
        self.proxy.save(
            rev,
            buffer_id,
            Box::new(move |result| {
                if let Ok(_r) = result {
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::BufferSave(path, rev),
                        Target::Auto,
                    );
                }
            }),
        );
    }

    fn initiate_diagnositcs_offset(&mut self, path: &Path, config: &Config) {
        if let Some(diagnostics) = self.diagnostics.get_mut(path) {
            if let Some(buffer) = self.open_files.get(path) {
                for diagnostic in Arc::make_mut(diagnostics).iter_mut() {
                    if diagnostic.range.is_none() {
                        diagnostic.range = Some((
                            buffer.offset_of_position(
                                &diagnostic.diagnositc.range.start,
                                config.editor.tab_width,
                            ),
                            buffer.offset_of_position(
                                &diagnostic.diagnositc.range.end,
                                config.editor.tab_width,
                            ),
                        ));
                    }
                }
            }
        }
    }

    fn update_diagnositcs_offset(
        &mut self,
        path: &Path,
        delta: &RopeDelta,
        config: &Config,
    ) {
        if let Some(diagnostics) = self.diagnostics.get_mut(path) {
            if let Some(buffer) = self.open_files.get(path) {
                let mut transformer = Transformer::new(delta);
                for diagnostic in Arc::make_mut(diagnostics).iter_mut() {
                    let (start, end) = diagnostic.range.unwrap();
                    let (new_start, new_end) = (
                        transformer.transform(start, false),
                        transformer.transform(end, true),
                    );
                    diagnostic.range = Some((new_start, new_end));
                    if start != new_start {
                        diagnostic.diagnositc.range.start = buffer
                            .offset_to_position(new_start, config.editor.tab_width);
                    }
                    if end != new_end {
                        diagnostic.diagnositc.range.end = buffer
                            .offset_to_position(new_end, config.editor.tab_width);
                        buffer.offset_to_position(new_end, config.editor.tab_width);
                    }
                }
            }
        }
    }

    fn cursor_apply_delta(&mut self, path: &Path, delta: &RopeDelta) {
        for (_view_id, editor) in self.editors.iter_mut() {
            match &editor.content {
                BufferContent::File(current_path) => {
                    if current_path == path {
                        Arc::make_mut(editor).cursor.apply_delta(delta);
                    }
                }
                BufferContent::Local(_) => {}
            }
        }
    }

    pub fn edit(
        &mut self,
        ctx: &mut EventCtx,
        path: &Path,
        edits: Vec<(&Selection, &str)>,
        edit_type: EditType,
        config: &Config,
    ) -> Option<RopeDelta> {
        self.initiate_diagnositcs_offset(path, config);
        let proxy = self.proxy.clone();
        let buffer = self.open_files.get_mut(path)?;

        let buffer_len = buffer.len();
        let mut move_cursor = true;
        for (selection, _) in edits.iter() {
            if selection.min_offset() == 0
                && selection.max_offset() >= buffer_len - 1
            {
                move_cursor = false;
                break;
            }
        }

        let delta =
            Arc::make_mut(buffer).edit_multiple(ctx, edits, proxy, edit_type);
        if move_cursor {
            self.cursor_apply_delta(path, &delta);
        }
        self.update_diagnositcs_offset(path, &delta, config);
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
                children: vec![],
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

    fn get_editor_or_new(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        path: Option<PathBuf>,
        config: &Config,
    ) -> &mut LapceEditorData {
        match editor_view_id {
            Some(view_id) => Arc::make_mut(self.editors.get_mut(&view_id).unwrap()),
            None => match *self.active_tab {
                Some(active) => {
                    let editor_tab =
                        Arc::make_mut(self.editor_tabs.get_mut(&active).unwrap());
                    match &editor_tab.children[editor_tab.active] {
                        EditorTabChild::Editor(id) => {
                            if config.editor.show_tab {
                                if let Some(path) = path {
                                    let mut editor_size = Size::ZERO;
                                    for (i, child) in
                                        editor_tab.children.iter().enumerate()
                                    {
                                        match child {
                                            EditorTabChild::Editor(id) => {
                                                let editor =
                                                    self.editors.get(id).unwrap();
                                                let current_size =
                                                    *editor.size.borrow();
                                                if current_size.height > 0.0 {
                                                    editor_size = current_size;
                                                }
                                                if editor.content
                                                    == BufferContent::File(
                                                        path.clone(),
                                                    )
                                                {
                                                    editor_tab.active = i;
                                                    ctx.submit_command(
                                                        Command::new(
                                                            LAPCE_UI_COMMAND,
                                                            LapceUICommand::Focus,
                                                            Target::Widget(*id),
                                                        ),
                                                    );
                                                    return Arc::make_mut(
                                                        self.editors
                                                            .get_mut(id)
                                                            .unwrap(),
                                                    );
                                                }
                                            }
                                        }
                                    }

                                    let new_editor = Arc::new(LapceEditorData::new(
                                        None,
                                        Some(editor_tab.widget_id),
                                        BufferContent::Local(LocalBufferKind::Empty),
                                        config,
                                    ));
                                    *new_editor.size.borrow_mut() = editor_size;
                                    self.editors.insert(
                                        new_editor.view_id,
                                        new_editor.clone(),
                                    );
                                    editor_tab.children.insert(
                                        editor_tab.active + 1,
                                        EditorTabChild::Editor(new_editor.view_id),
                                    );
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::EditorTabAdd(
                                            editor_tab.active + 1,
                                            EditorTabChild::Editor(
                                                new_editor.view_id,
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

                                    return Arc::make_mut(
                                        self.editors
                                            .get_mut(&new_editor.view_id)
                                            .unwrap(),
                                    );
                                }
                                Arc::make_mut(self.editors.get_mut(id).unwrap())
                            } else {
                                Arc::make_mut(self.editors.get_mut(id).unwrap())
                            }
                        }
                    }
                }
                None => {
                    let split = self.splits.get_mut(&self.split_id).unwrap();
                    let split = Arc::make_mut(split);

                    let mut editor_tab = LapceEditorTabData {
                        widget_id: WidgetId::next(),
                        split: *self.split_id,
                        active: 0,
                        children: vec![],
                        layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
                        content_is_hot: Rc::new(RefCell::new(false)),
                    };

                    let editor = Arc::new(LapceEditorData::new(
                        None,
                        Some(editor_tab.widget_id),
                        BufferContent::Local(LocalBufferKind::Empty),
                        config,
                    ));

                    editor_tab
                        .children
                        .push(EditorTabChild::Editor(editor.view_id));

                    self.active = Arc::new(Some(editor.view_id));
                    self.active_tab = Arc::new(Some(editor_tab.widget_id));

                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EditorTabAdd(
                            0,
                            EditorTabChild::Editor(editor.view_id),
                        ),
                        Target::Widget(editor_tab.widget_id),
                    ));
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

                    self.editors.insert(editor.view_id, editor.clone());
                    self.editor_tabs
                        .insert(editor_tab.widget_id, Arc::new(editor_tab));

                    Arc::make_mut(self.editors.get_mut(&editor.view_id).unwrap())
                }
            },
        }
    }

    pub fn jump_to_position(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        position: Position,
        config: &Config,
    ) {
        let editor = self.get_editor_or_new(ctx, editor_view_id, None, config);
        match &editor.content {
            BufferContent::File(path) => {
                let location = EditorLocationNew {
                    path: path.clone(),
                    position: Some(position),
                    scroll_offset: None,
                    hisotry: None,
                };
                self.jump_to_location(ctx, editor_view_id, location, config);
            }
            BufferContent::Local(_) => {}
        }
    }

    pub fn jump_to_location(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        location: EditorLocationNew,
        config: &Config,
    ) -> WidgetId {
        let editor_view_id = self
            .get_editor_or_new(
                ctx,
                editor_view_id,
                Some(location.path.clone()),
                config,
            )
            .view_id;
        let buffer = self.editor_buffer(editor_view_id);
        let editor = self.get_editor_or_new(
            ctx,
            Some(editor_view_id),
            Some(location.path.clone()),
            config,
        );
        editor.save_jump_location(&buffer, config.editor.tab_width);
        self.go_to_location(ctx, Some(editor_view_id), location, config);
        editor_view_id
    }

    pub fn go_to_location(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        location: EditorLocationNew,
        config: &Config,
    ) {
        let editor_view_id = self
            .get_editor_or_new(
                ctx,
                editor_view_id,
                Some(location.path.clone()),
                config,
            )
            .view_id;
        let buffer = self.editor_buffer(editor_view_id);
        let new_buffer = match &buffer.content {
            BufferContent::File(path) => path != &location.path,
            BufferContent::Local(_) => true,
        };
        if new_buffer {
            self.db.save_buffer_position(&self.workspace, &buffer);
        } else if location.position.is_none()
            && location.scroll_offset.is_none()
            && location.hisotry.is_none()
        {
            return;
        }
        let path = location.path.clone();
        let buffer_exists = self.open_files.contains_key(&path);
        if !buffer_exists {
            let mut buffer = Buffer::new(
                BufferContent::File(path.clone()),
                self.update_sender.clone(),
                *self.tab_id,
                ctx.get_external_handle(),
            );
            if let Ok(info) = self.db.get_buffer_info(&self.workspace, &path) {
                buffer.scroll_offset =
                    Vec2::new(info.scroll_offset.0, info.scroll_offset.1);
                buffer.cursor_offset = info.cursor_offset;
            }
            let buffer = Arc::new(buffer);
            self.open_files.insert(path.clone(), buffer.clone());
            buffer.retrieve_file(
                *self.tab_id,
                self.proxy.clone(),
                ctx.get_external_handle(),
                vec![(editor_view_id, location)],
            );
        } else {
            let buffer = self.open_files.get_mut(&path).unwrap().clone();

            let (offset, scroll_offset) = match &location.position {
                Some(position) => {
                    let offset =
                        buffer.offset_of_position(position, config.editor.tab_width);
                    let buffer = self.open_files.get_mut(&path).unwrap();
                    let buffer = Arc::make_mut(buffer);
                    buffer.cursor_offset = offset;
                    if let Some(scroll_offset) = location.scroll_offset.as_ref() {
                        buffer.scroll_offset = *scroll_offset;
                    }

                    (offset, location.scroll_offset.as_ref())
                }
                None => (buffer.cursor_offset, Some(&buffer.scroll_offset)),
            };

            if let Some(compare) = location.hisotry.as_ref() {
                if !buffer.histories.contains_key(compare) {
                    buffer.retrieve_file_head(
                        *self.tab_id,
                        self.proxy.clone(),
                        ctx.get_external_handle(),
                    );
                }
            }

            let editor = self.get_editor_or_new(
                ctx,
                Some(editor_view_id),
                Some(location.path.clone()),
                config,
            );
            editor.content = BufferContent::File(path.clone());
            editor.compare = location.hisotry.clone();
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
            } else if new_buffer || editor_view_id == *self.palette_preview_editor {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorCenter,
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
        }
    }

    pub fn jump_to_line(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: Option<WidgetId>,
        line: usize,
        config: &Config,
    ) {
        let editor_view_id = self
            .get_editor_or_new(ctx, editor_view_id, None, config)
            .view_id;
        let buffer = self.editor_buffer(editor_view_id);
        let offset = buffer.first_non_blank_character_on_line(if line > 0 {
            line - 1
        } else {
            0
        });
        let position = buffer.offset_to_position(offset, config.editor.tab_width);
        self.jump_to_position(ctx, Some(editor_view_id), position, config);
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
        workspace: Arc<LapceWorkspace>,
        db: Arc<LapceDb>,
    ) -> Self {
        let split_id = Arc::new(WidgetId::next());

        let mut open_files = im::HashMap::new();
        let mut editors = im::HashMap::new();
        let editor_tabs = im::HashMap::new();
        let splits = im::HashMap::new();

        let path = PathBuf::from("[Palette Preview Editor]");
        let editor = LapceEditorData::new(
            Some(palette_preview_editor),
            None,
            BufferContent::File(path.clone()),
            config,
        );
        editors.insert(editor.view_id, Arc::new(editor));
        let mut buffer = Buffer::new(
            BufferContent::File(path.clone()),
            update_sender.clone(),
            tab_id,
            event_sink.clone(),
        );
        buffer.loaded = true;
        open_files.insert(path, Arc::new(buffer));

        let mut local_buffers = im::HashMap::new();
        local_buffers.insert(
            LocalBufferKind::Empty,
            Arc::new(Buffer::new(
                BufferContent::Local(LocalBufferKind::Empty),
                update_sender.clone(),
                tab_id,
                event_sink.clone(),
            )),
        );

        let mut main_split_data = Self {
            tab_id: Arc::new(tab_id),
            split_id,
            editors,
            editor_tabs,
            splits,
            open_files,
            local_buffers,
            active: Arc::new(None),
            active_tab: Arc::new(None),
            update_sender: update_sender.clone(),
            register: Arc::new(Register::default()),
            proxy: proxy.clone(),
            palette_preview_editor: Arc::new(palette_preview_editor),
            show_code_actions: false,
            current_code_actions: 0,
            diagnostics: im::HashMap::new(),
            error_count: 0,
            warning_count: 0,
            workspace,
            db,
        };

        if let Some(info) = workspace_info {
            let mut positions = HashMap::new();
            let split_data = info.split.to_data(
                &mut main_split_data,
                None,
                &mut positions,
                tab_id,
                update_sender,
                config,
                event_sink.clone(),
            );
            main_split_data.split_id = Arc::new(split_data.widget_id);
            for (path, locations) in positions.into_iter() {
                main_split_data
                    .open_files
                    .get(&path)
                    .unwrap()
                    .retrieve_file(
                        tab_id,
                        proxy.clone(),
                        event_sink.clone(),
                        locations.clone(),
                    );
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

    pub fn add_editor(
        &mut self,
        view_id: WidgetId,
        split_id: Option<WidgetId>,
        buffer_kind: LocalBufferKind,
        config: &Config,
        event_sink: ExtEventSink,
    ) {
        let mut buffer = Buffer::new(
            BufferContent::Local(buffer_kind.clone()),
            self.update_sender.clone(),
            *self.tab_id,
            event_sink,
        )
        .set_local();
        buffer.load_content("");
        self.local_buffers
            .insert(buffer_kind.clone(), Arc::new(buffer));
        let editor = LapceEditorData::new(
            Some(view_id),
            split_id,
            BufferContent::Local(buffer_kind),
            config,
        );
        self.editors.insert(editor.view_id, Arc::new(editor));
    }

    #[allow(unused_variables)]
    pub fn split_close(
        &mut self,
        ctx: &mut EventCtx,
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

    pub fn editor_close(&mut self, ctx: &mut EventCtx, view_id: WidgetId) {
        let editor = self.editors.get(&view_id).unwrap();
        if let BufferContent::File(path) = &editor.content {
            let buffer = self.open_files.get(path).unwrap();
            self.db.save_buffer_position(&self.workspace, buffer);
        }
        if let Some(tab_id) = editor.tab_id {
            let editor_tab = self.editor_tabs.get(&tab_id).unwrap();
            let mut index = 0;
            for (i, child) in editor_tab.children.iter().enumerate() {
                if child == &EditorTabChild::Editor(view_id) {
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
                LapceUICommand::SplitChangeDirectoin(direction),
                Target::Widget(split_id),
            ));
        }

        if direction == split.direction {
            let new_index = if shift_current { index } else { index + 1 };
            split.children.insert(new_index, new_content.clone());
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::SplitAdd(new_index, new_content, false),
                Target::Widget(split_id),
            ));
            split_id
        } else {
            let children = if shift_current {
                vec![new_content.clone(), from_content.clone()]
            } else {
                vec![from_content.clone(), new_content.clone()]
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

    pub fn split_editor(
        &mut self,
        ctx: &mut EventCtx,
        editor: &mut LapceEditorData,
        direction: SplitDirection,
    ) {
        if let Some(editor_tab_id) = editor.tab_id {
            let editor_tab = self.editor_tabs.get(&editor_tab_id).unwrap();
            let split_id = editor_tab.split;
            let mut new_editor = editor.copy(WidgetId::next());
            let mut new_editor_tab = LapceEditorTabData {
                widget_id: WidgetId::next(),
                split: split_id,
                active: 0,
                children: vec![EditorTabChild::Editor(new_editor.view_id)],
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

            self.editors
                .insert(new_editor.view_id, Arc::new(new_editor));
            self.editor_tabs
                .insert(new_editor_tab.widget_id, Arc::new(new_editor_tab));
        }

        //   if let Some(split_id) = editor.split_id.clone() {
        //       let mut new_editor = editor.clone();
        //       new_editor.view_id = WidgetId::next();

        //       let new_split_id = self.split(
        //           ctx,
        //           split_id,
        //           SplitContent::Editor(editor.view_id),
        //           SplitContent::Editor(new_editor.view_id),
        //           direction,
        //       );

        //       new_editor.split_id = Some(new_split_id);
        //       if new_split_id != split_id {
        //           editor.split_id = Some(new_split_id);
        //       }

        //       self.editors
        //           .insert(new_editor.view_id, Arc::new(new_editor));
        //   }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub enum EditorType {
//     DiffSplit(WidgetId, WidgetId),
//     Normal,
//     SourceControl,
//     Search,
//     Palette,
// }

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

#[derive(Clone, Debug, PartialEq)]
pub enum EditorTabChild {
    Editor(WidgetId),
}

impl EditorTabChild {
    pub fn widget_id(&self) -> WidgetId {
        match &self {
            EditorTabChild::Editor(widget_id) => *widget_id,
        }
    }

    pub fn child_info(
        &self,
        data: &LapceTabData,
        tab_width: usize,
    ) -> EditorTabChildInfo {
        match &self {
            EditorTabChild::Editor(view_id) => {
                let editor_data = data.main_split.editors.get(view_id).unwrap();
                EditorTabChildInfo::Editor(editor_data.editor_info(data, tab_width))
            }
        }
    }

    pub fn set_editor_tab(&self, data: &mut LapceTabData, editor_tab_id: WidgetId) {
        match &self {
            EditorTabChild::Editor(view_id) => {
                let editor_data = data.main_split.editors.get_mut(view_id).unwrap();
                let editor_data = Arc::make_mut(editor_data);
                editor_data.tab_id = Some(editor_tab_id);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct LapceEditorTabData {
    pub widget_id: WidgetId,
    pub split: WidgetId,
    pub active: usize,
    pub children: Vec<EditorTabChild>,
    pub layout_rect: Rc<RefCell<Rect>>,
    pub content_is_hot: Rc<RefCell<bool>>,
}

impl LapceEditorTabData {
    pub fn tab_info(&self, data: &LapceTabData, tab_width: usize) -> EditorTabInfo {
        let info = EditorTabInfo {
            active: self.active,
            is_focus: *data.main_split.active_tab == Some(self.widget_id),
            children: self
                .children
                .iter()
                .map(|child| child.child_info(data, tab_width))
                .collect(),
        };
        info
    }
}

#[derive(Clone, Debug)]
pub struct SelectionHistory {
    pub rev: u64,
    pub content: BufferContent,
    pub selections: im::Vector<Selection>,
}

#[derive(Clone, Debug)]
pub struct LapceEditorData {
    pub tab_id: Option<WidgetId>,
    pub view_id: WidgetId,
    pub content: BufferContent,
    pub compare: Option<String>,
    pub code_lens: bool,
    pub scroll_offset: Vec2,
    pub cursor: Cursor,
    pub selection_history: SelectionHistory,
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
        tab_id: Option<WidgetId>,
        content: BufferContent,
        config: &Config,
    ) -> Self {
        Self {
            tab_id,
            view_id: view_id.unwrap_or_else(WidgetId::next),
            selection_history: SelectionHistory {
                rev: 0,
                content: content.clone(),
                selections: im::Vector::new(),
            },
            content,
            scroll_offset: Vec2::ZERO,
            cursor: if config.lapce.modal {
                Cursor::new(CursorMode::Normal(0), None)
            } else {
                Cursor::new(CursorMode::Insert(Selection::caret(0)), None)
            },
            size: Rc::new(RefCell::new(Size::ZERO)),
            compare: None,
            code_lens: false,
            window_origin: Point::ZERO,
            snippet: None,
            locations: vec![],
            current_location: 0,
            last_movement: Movement::Left,
            inline_find: None,
            last_inline_find: None,
        }
    }

    pub fn copy(&self, new_view_id: WidgetId) -> LapceEditorData {
        let mut new_editor = self.clone();
        new_editor.view_id = new_view_id;
        new_editor.size = Rc::new(RefCell::new(Size::ZERO));
        new_editor
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

    pub fn save_jump_location(&mut self, buffer: &Buffer, tab_width: usize) {
        if let BufferContent::File(path) = &buffer.content {
            let location = EditorLocationNew {
                path: path.clone(),
                position: Some(
                    buffer.offset_to_position(self.cursor.offset(), tab_width),
                ),
                scroll_offset: Some(self.scroll_offset),
                hisotry: None,
            };
            self.locations.push(location);
            self.current_location = self.locations.len();
        }
    }

    pub fn editor_info(&self, data: &LapceTabData, tab_width: usize) -> EditorInfo {
        let info = EditorInfo {
            content: self.content.clone(),
            scroll_offset: (self.scroll_offset.x, self.scroll_offset.y),
            position: if let BufferContent::File(path) = &self.content {
                let buffer = data.main_split.open_files.get(path).unwrap().clone();
                Some(buffer.offset_to_position(self.cursor.offset(), tab_width))
            } else {
                None
            },
        };
        info
    }
}

#[derive(Clone, Data, Lens)]
pub struct LapceEditorViewData {
    pub main_split: LapceMainSplitData,
    pub workspace: Option<Arc<LapceWorkspace>>,
    pub proxy: Arc<LapceProxy>,
    pub editor: Arc<LapceEditorData>,
    pub buffer: Arc<Buffer>,
    pub diagnostics: Arc<Vec<EditorDiagnostic>>,
    pub all_diagnostics: im::HashMap<PathBuf, Arc<Vec<EditorDiagnostic>>>,
    pub keypress: Arc<KeyPressData>,
    pub completion: Arc<CompletionData>,
    pub palette: Arc<WidgetId>,
    pub theme: Arc<std::collections::HashMap<String, Color>>,
    pub config: Arc<Config>,
}

impl LapceEditorViewData {
    pub fn buffer_mut(&mut self) -> &mut Buffer {
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

    #[allow(unused_variables)]
    pub fn fill_text_layouts(
        &mut self,
        ctx: &mut EventCtx,
        theme: &Arc<HashMap<String, Color>>,
        env: &Env,
    ) {
    }

    pub fn on_diagnostic(&self, tab_width: usize) -> Option<usize> {
        let offset = self.editor.cursor.offset();
        let position = self.buffer.offset_to_position(offset, tab_width);
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

    pub fn get_code_actions(&mut self, ctx: &mut EventCtx, tab_width: usize) {
        if !self.buffer.loaded {
            return;
        }
        if self.buffer.local {
            return;
        }
        if let BufferContent::File(path) = &self.buffer.content {
            let path = path.clone();
            let offset = self.editor.cursor.offset();
            let prev_offset = self.buffer.prev_code_boundary(offset);
            if self.buffer.code_actions.get(&prev_offset).is_none() {
                let buffer_id = self.buffer.id;
                let position =
                    self.buffer.offset_to_position(prev_offset, tab_width);
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
                                let _ = event_sink.submit_command(
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
    }

    #[allow(dead_code)]
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
        tab_width: usize,
    ) -> Result<()> {
        let additioal_edit = item.additional_text_edits.as_ref().map(|edits| {
            edits
                .iter()
                .map(|edit| {
                    let selection = Selection::region(
                        self.buffer.offset_of_position(&edit.range.start, tab_width),
                        self.buffer.offset_of_position(&edit.range.end, tab_width),
                    );
                    (selection, edit.new_text.clone())
                })
                .collect::<Vec<(Selection, String)>>()
        });
        let additioal_edit = additioal_edit.as_ref().map(|edits| {
            edits
                .iter()
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
                        self.buffer.offset_of_position(&edit.range.start, tab_width);
                    let edit_end =
                        self.buffer.offset_of_position(&edit.range.end, tab_width);
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

                            if snippet_tabs.is_empty() {
                                self.set_cursor_after_change(selection);
                                return Ok(());
                            }

                            let mut selection = Selection::new();
                            let (_tab, (start, end)) = &snippet_tabs[0];
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

    #[allow(dead_code)]
    fn scroll(
        &mut self,
        ctx: &mut EventCtx,
        down: bool,
        count: usize,
        config: &Config,
    ) {
        let line_height = self.config.editor.line_height as f64;
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.editor.cursor.offset();
        let (line, col) = self
            .buffer
            .offset_to_line_col(offset, config.editor.tab_width);
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
            + col.min(self.buffer.line_end_col(
                line,
                false,
                config.editor.tab_width,
            ));
        self.set_cursor(Cursor::new(
            CursorMode::Normal(offset),
            self.editor.cursor.horiz,
        ));
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((self.editor.scroll_offset.x, top)),
            Target::Widget(self.editor.view_id),
        ));
    }

    #[allow(dead_code)]
    fn page_move(&mut self, ctx: &mut EventCtx, down: bool, config: &Config) {
        let line_height = self.config.editor.line_height as f64;
        let lines =
            (self.editor.size.borrow().height / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        self.do_move(
            if down { &Movement::Down } else { &Movement::Up },
            lines,
            config,
        );
        let rect = Rect::ZERO
            .with_origin(
                self.editor.scroll_offset.to_point()
                    + Vec2::new(0.0, if down { distance } else { -distance }),
            )
            .with_size(*self.editor.size.borrow());
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureRectVisible(rect),
            Target::Widget(self.editor.view_id),
        ));
    }

    pub fn next_error(&mut self, ctx: &mut EventCtx, config: &Config) {
        if let BufferContent::File(buffer_path) = &self.buffer.content {
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
                    if errors.is_empty() {
                        None
                    } else {
                        errors.sort();
                        Some((path, errors))
                    }
                })
                .collect::<Vec<(&PathBuf, Vec<Position>)>>();
            if file_diagnostics.is_empty() {
                return;
            }
            file_diagnostics.sort_by(|a, b| a.0.cmp(b.0));

            let offset = self.editor.cursor.offset();
            let position = self
                .buffer
                .offset_to_position(offset, config.editor.tab_width);
            let (path, position) =
                next_in_file_errors_offset(position, buffer_path, &file_diagnostics);
            let location = EditorLocationNew {
                path,
                position: Some(position),
                scroll_offset: None,
                hisotry: None,
            };
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::JumpToLocation(None, location),
                Target::Auto,
            ));
        }
    }

    #[allow(unused_variables)]
    pub fn jump_location_forward(
        &mut self,
        ctx: &mut EventCtx,
        env: &Env,
    ) -> Option<()> {
        if self.editor.locations.is_empty() {
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
        config: &Config,
    ) -> Option<()> {
        if self.editor.current_location < 1 {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer, config.editor.tab_width);
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

    pub fn do_move(&mut self, movement: &Movement, count: usize, config: &Config) {
        if movement.is_jump() && movement != &self.editor.last_movement {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer, config.editor.tab_width);
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
                    self.editor.code_lens,
                    self.editor.compare.clone(),
                    config,
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
                    self.editor.code_lens,
                    self.editor.compare.clone(),
                    config,
                );
                let start = *start;
                let mode = *mode;
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
                    self.editor.code_lens,
                    self.editor.compare.clone(),
                    &self.config,
                );
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

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
            let line_end = self.buffer.line_end_col(
                line,
                !self.editor.cursor.is_normal(),
                config.editor.tab_width,
            );
            let width = config.editor_text_width(text, "W");

            let col = (if self.editor.cursor.is_insert() {
                (pos.x / width).round() as usize
            } else {
                (pos.x / width).floor() as usize
            })
            .min(line_end);
            (line, col)
        };
        self.buffer
            .offset_of_line_col(line, col, config.editor.tab_width)
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor = cursor;
    }

    #[allow(dead_code)]
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
                    CursorMode::Insert { .. } | CursorMode::Visual { .. } => self
                        .editor
                        .cursor
                        .edit_selection(&self.buffer, self.config.editor.tab_width),
                };
                let after = !data.content.contains('\n');
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
                        self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        ),
                        "\n".to_string() + &data.content,
                    ),
                    CursorMode::Visual { mode, .. } => {
                        let selection = self.editor.cursor.edit_selection(
                            &self.buffer,
                            self.config.editor.tab_width,
                        );
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
                        buffer.offset_of_position(
                            &diagnostic.diagnositc.range.start,
                            self.config.editor.tab_width,
                        ),
                        buffer.offset_of_position(
                            &diagnostic.diagnositc.range.end,
                            self.config.editor.tab_width,
                        ),
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
                let (start, end) = diagnostic.range.unwrap();
                let (new_start, new_end) = (
                    transformer.transform(start, false),
                    transformer.transform(end, true),
                );
                diagnostic.range = Some((new_start, new_end));
                if start != new_start {
                    diagnostic.diagnositc.range.start = buffer
                        .offset_to_position(new_start, self.config.editor.tab_width);
                }
                if end != new_end {
                    diagnostic.diagnositc.range.end = buffer
                        .offset_to_position(new_end, self.config.editor.tab_width);
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
                    let data = self
                        .editor
                        .cursor
                        .yank(&self.buffer, self.config.editor.tab_width);
                    let register = Arc::make_mut(&mut self.main_split.register);
                    register.add_delete(data);
                }
            }
            #[allow(unused_variables)]
            CursorMode::Visual { start, end, mode } => {
                let data = self
                    .editor
                    .cursor
                    .yank(&self.buffer, self.config.editor.tab_width);
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
            buffer.edit(ctx, selection, c, proxy, edit_type)
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
            if view_id != &self.editor.view_id
                && self.editor.content == editor.content
            {
                Arc::make_mut(editor).cursor.apply_delta(delta);
            }
        }
    }

    pub fn cancel_completion(&mut self) {
        let completion = Arc::make_mut(&mut self.completion);
        completion.cancel();
    }
}

fn next_in_file_errors_offset(
    position: Position,
    path: &Path,
    file_diagnostics: &[(&PathBuf, Vec<Position>)],
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

#[allow(dead_code)]
fn process_get_references(
    editor_view_id: WidgetId,
    offset: usize,
    result: Result<Value, Value>,
    event_sink: ExtEventSink,
) -> Result<()> {
    let res = result.map_err(|e| anyhow!("{:?}", e))?;
    let locations: Vec<Location> = serde_json::from_value(res)?;
    if locations.is_empty() {
        return Ok(());
    }
    if locations.len() == 1 {
        let location = &locations[0];
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::GotoReference(
                editor_view_id,
                offset,
                EditorLocationNew {
                    path: PathBuf::from(location.uri.path()),
                    position: Some(location.range.start),
                    scroll_offset: None,
                    hisotry: None,
                },
            ),
            Target::Auto,
        );
    }
    let _ = event_sink.submit_command(
        LAPCE_UI_COMMAND,
        LapceUICommand::PaletteReferences(offset, locations),
        Target::Auto,
    );
    Ok(())
}

pub fn hex_to_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches('#');
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
    highlight_configs: &mut HashMap<LapceLanguage, HighlightConfiguration>,
    event_sink: &ExtEventSink,
    tab_id: WidgetId,
) {
    if let std::collections::hash_map::Entry::Vacant(e) =
        parsers.entry(update.language)
    {
        let parser = new_parser(update.language);
        e.insert(parser);
    }
    let parser = parsers.get_mut(&update.language).unwrap();
    if let Some(tree) = parser.parse(
        update.rope.slice_to_cow(0..update.rope.len()).as_bytes(),
        None,
    ) {
        let _ = event_sink.submit_command(
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
        if let std::collections::hash_map::Entry::Vacant(e) =
            highlight_configs.entry(update.language)
        {
            let highlight_config = new_highlight_config(update.language);
            e.insert(highlight_config);
        }
        let highlight_config = highlight_configs.get(&update.language).unwrap();
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
                            if let Some(hl) = SCOPES.get(hl.0) {
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
        let _ = event_sink.submit_command(
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

#[allow(dead_code)]
fn str_is_pair_left(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        if matching_pair_direction(c).unwrap_or(false) {
            return true;
        }
    }
    false
}

#[allow(dead_code)]
fn str_is_pair_right(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return !matching_pair_direction(c).unwrap_or(true);
    }
    false
}

#[allow(dead_code)]
fn str_matching_pair(c: &str) -> Option<char> {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return matching_char(c);
    }
    None
}

#[allow(dead_code)]
fn progress_term_event() {}
