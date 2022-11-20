use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Instant,
};

use alacritty_terminal::{grid::Dimensions, term::cell::Flags};
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use druid::{
    Command, Data, Env, EventCtx, ExtEventSink, Lens, Modifiers, Target, WidgetId,
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use itertools::Itertools;
use lapce_core::{
    command::{EditCommand, FocusCommand},
    language::LapceLanguage,
    mode::Mode,
};
use lapce_rpc::proxy::ProxyResponse;
use lsp_types::{DocumentSymbolResponse, Position, Range, SymbolKind};
use uuid::Uuid;

use crate::{
    command::{
        CommandExecuted, CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND,
        LAPCE_UI_COMMAND,
    },
    config::LapceConfig,
    data::{
        FocusArea, LapceMainSplitData, LapceTabData, LapceWorkspace,
        LapceWorkspaceType,
    },
    document::BufferContent,
    editor::EditorLocation,
    find::Find,
    keypress::{KeyMap, KeyPressData, KeyPressFocus},
    list::ListData,
    panel::PanelKind,
    proxy::{path_from_url, LapceProxy},
    terminal::TerminalSplitData,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteType {
    File,
    Line,
    GlobalSearch,
    DocumentSymbol,
    WorkspaceSymbol,
    Workspace,
    Command,
    Reference,
    ColorTheme,
    IconTheme,
    SshHost,
    Language,
}

impl PaletteType {
    fn string(&self) -> String {
        match &self {
            PaletteType::Line => "/".to_string(),
            PaletteType::DocumentSymbol => "@".to_string(),
            PaletteType::WorkspaceSymbol => "#".to_string(),
            PaletteType::GlobalSearch => "?".to_string(),
            PaletteType::Workspace => ">".to_string(),
            PaletteType::Command => ":".to_string(),
            PaletteType::File
            | PaletteType::Reference
            | PaletteType::ColorTheme
            | PaletteType::IconTheme
            | PaletteType::SshHost
            | PaletteType::Language => "".to_string(),
        }
    }

    pub fn has_preview(&self) -> bool {
        matches!(
            self,
            PaletteType::Line
                | PaletteType::DocumentSymbol
                | PaletteType::WorkspaceSymbol
                | PaletteType::GlobalSearch
                | PaletteType::Reference
        )
    }

    /// Get the palette type that it should be considered as based on the current
    /// [`PaletteType`] and the current input.
    fn get_palette_type(current_type: &PaletteType, input: &str) -> PaletteType {
        match current_type {
            PaletteType::Reference
            | PaletteType::SshHost
            | PaletteType::ColorTheme
            | PaletteType::IconTheme
            | PaletteType::Language => {
                return current_type.clone();
            }
            _ => (),
        }
        if input.is_empty() {
            return PaletteType::File;
        }
        match input {
            _ if input.starts_with('/') => PaletteType::Line,
            _ if input.starts_with('@') => PaletteType::DocumentSymbol,
            _ if input.starts_with('#') => PaletteType::WorkspaceSymbol,
            _ if input.starts_with('>') => PaletteType::Workspace,
            _ if input.starts_with(':') => PaletteType::Command,
            _ => PaletteType::File,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteIcon {
    File(String),
    Symbol(SymbolKind),
    None,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PaletteStatus {
    Inactive,
    Started,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteItemContent {
    File(PathBuf, PathBuf),
    Line(usize, String),
    TerminalLine(i32, String),
    DocumentSymbol {
        kind: SymbolKind,
        name: String,
        range: Range,
        container_name: Option<String>,
    },
    WorkspaceSymbol {
        // TODO: Include what language it is from?
        kind: SymbolKind,
        name: String,
        container_name: Option<String>,
        location: EditorLocation<Position>,
    },
    ReferenceLocation(PathBuf, EditorLocation<Position>),
    Workspace(LapceWorkspace),
    SshHost(String, String),
    Command(LapceCommand),
    ColorTheme(String),
    IconTheme(String),
    Language(String),
}

impl PaletteItemContent {
    fn select(
        &self,
        ctx: &mut EventCtx,
        preview: bool,
        preview_editor_id: WidgetId,
    ) -> bool {
        match &self {
            PaletteItemContent::File(_, full_path) => {
                if !preview {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::OpenFile(full_path.clone(), true),
                        Target::Auto,
                    ));
                }
            }
            PaletteItemContent::DocumentSymbol { range, .. } => {
                let editor_id = if preview {
                    Some(preview_editor_id)
                } else {
                    None
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToPosition(editor_id, range.start, true),
                    Target::Auto,
                ));
            }
            PaletteItemContent::WorkspaceSymbol { location, .. } => {
                let editor_id = if preview {
                    Some(preview_editor_id)
                } else {
                    None
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToLspLocation(
                        editor_id,
                        location.clone(),
                        true,
                    ),
                    Target::Auto,
                ));
            }
            PaletteItemContent::Line(line, _) => {
                let editor_id = if preview {
                    Some(preview_editor_id)
                } else {
                    None
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToLine(editor_id, *line),
                    Target::Auto,
                ));
            }
            PaletteItemContent::ReferenceLocation(_rel_path, location) => {
                let editor_id = if preview {
                    Some(preview_editor_id)
                } else {
                    None
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToLspLocation(
                        editor_id,
                        location.clone(),
                        true,
                    ),
                    Target::Auto,
                ));
            }
            PaletteItemContent::Workspace(workspace) => {
                if !preview {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SetWorkspace(workspace.clone()),
                        Target::Auto,
                    ));
                }
            }
            PaletteItemContent::ColorTheme(theme) => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SetColorTheme(theme.to_string(), preview),
                    Target::Auto,
                ));
            }
            PaletteItemContent::IconTheme(theme) => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SetIconTheme(theme.to_string(), preview),
                    Target::Auto,
                ));
            }
            PaletteItemContent::Language(name) => {
                if !preview {
                    let name = name.to_string();
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SetLanguage(name),
                        Target::Auto,
                    ))
                }
            }
            PaletteItemContent::Command(command) => {
                if !preview {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        command.clone(),
                        Target::Auto,
                    ));
                }
                return !command.is_palette_command();
            }
            PaletteItemContent::TerminalLine(line, _content) => {
                if !preview {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::TerminalJumpToLine(*line),
                        Target::Auto,
                    ));
                }
            }
            PaletteItemContent::SshHost(user, host) => {
                if !preview {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SetWorkspace(LapceWorkspace {
                            kind: LapceWorkspaceType::RemoteSSH(
                                user.to_string(),
                                host.to_string(),
                            ),
                            path: None,
                            last_open: 0,
                        }),
                        Target::Auto,
                    ));
                }
            }
        }
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub content: PaletteItemContent,
    pub filter_text: String,
    pub score: i64,
    pub indices: Vec<usize>,
}

pub struct PaletteViewLens;

#[derive(Clone, Data)]
pub struct PaletteViewData {
    pub palette: Arc<PaletteData>,
    pub find: Arc<Find>,
    pub workspace: Arc<LapceWorkspace>,
    pub main_split: LapceMainSplitData,
    pub keypress: Arc<KeyPressData>,
    pub config: Arc<LapceConfig>,
    pub focus_area: FocusArea,
    pub terminal: Arc<TerminalSplitData>,
}

impl Lens<LapceTabData, PaletteViewData> for PaletteViewLens {
    fn with<V, F: FnOnce(&PaletteViewData) -> V>(
        &self,
        data: &LapceTabData,
        f: F,
    ) -> V {
        let palette_view = data.palette_view_data();
        f(&palette_view)
    }

    fn with_mut<V, F: FnOnce(&mut PaletteViewData) -> V>(
        &self,
        data: &mut LapceTabData,
        f: F,
    ) -> V {
        let mut palette_view = data.palette_view_data();
        let result = f(&mut palette_view);
        data.palette = palette_view.palette.clone();
        data.workspace = palette_view.workspace.clone();
        data.keypress = palette_view.keypress.clone();
        data.main_split = palette_view.main_split.clone();
        data.find = palette_view.find;
        result
    }
}

/// Data to be held by the palette list
#[derive(Data, Clone)]
pub struct PaletteListData {
    /// Should only be `None` when it hasn't been updated initially  
    /// We need this just for some rendering, and not editing it.
    pub workspace: Option<Arc<LapceWorkspace>>,
    /// Should only be `None` when it hasn't been updated initially.
    /// We need this just for some rendering, and not editing it.
    pub keymaps: Option<Arc<Vec<KeyMap>>>,
    /// The mode of the current editor/terminal/none
    #[data(eq)]
    pub mode: Option<Mode>,
}

#[derive(Clone)]
pub struct PaletteData {
    pub widget_id: WidgetId,
    pub scroll_id: WidgetId,
    pub status: PaletteStatus,
    /// Holds information about the list, including the filtered items
    pub list_data: ListData<PaletteItem, PaletteListData>,
    pub proxy: Arc<LapceProxy>,
    pub palette_type: PaletteType,
    pub sender: Sender<(String, String, im::Vector<PaletteItem>)>,
    pub receiver: Option<Receiver<(String, String, im::Vector<PaletteItem>)>>,
    pub run_id: String,
    pub input: String,
    pub cursor: usize,
    pub has_nonzero_default_index: bool,
    /// The unfiltered items list
    pub total_items: im::Vector<PaletteItem>,
    pub preview_editor: WidgetId,
    pub input_editor: WidgetId,
    pub executed_commands: Rc<RefCell<HashMap<String, Instant>>>,
}

impl KeyPressFocus for PaletteViewData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "list_focus" | "palette_focus" | "modal_focus")
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.input.insert_str(palette.cursor, c);
        palette.cursor += c.len();
        self.update_palette(ctx);
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        let selected_index = self.palette.list_data.selected_index;

        // Pass any commands, like list movement, to the selector list
        Arc::make_mut(&mut self.palette)
            .list_data
            .run_command(ctx, command);

        // If the selection changed, then update the preview
        if selected_index != self.palette.list_data.selected_index {
            self.palette.preview(ctx);
        }

        match &command.kind {
            CommandKind::Focus(FocusCommand::ModalClose) => {
                self.cancel(ctx);
            }
            CommandKind::Edit(cmd) => match cmd {
                EditCommand::DeleteBackward => {
                    self.delete_backward(ctx);
                }
                EditCommand::DeleteToBeginningOfLine => {
                    self.delete_to_beginning_of_line(ctx);
                }
                _ => return CommandExecuted::No,
            },
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }
}

impl PaletteData {
    pub fn new(config: Arc<LapceConfig>, proxy: Arc<LapceProxy>) -> Self {
        let (sender, receiver) = unbounded();
        let widget_id = WidgetId::next();
        let scroll_id = WidgetId::next();
        let preview_editor = WidgetId::next();
        let mut list_data = ListData::new(
            config,
            widget_id,
            PaletteListData {
                workspace: None,
                keymaps: None,
                mode: None,
            },
        );
        // TODO: Make these configurable
        list_data.line_height = Some(25);
        list_data.max_displayed_items = 15;
        Self {
            widget_id,
            scroll_id,
            status: PaletteStatus::Inactive,
            list_data,
            proxy,
            palette_type: PaletteType::File,
            sender,
            receiver: Some(receiver),
            run_id: Uuid::new_v4().to_string(),
            input: "".to_string(),
            cursor: 0,
            has_nonzero_default_index: false,
            total_items: im::Vector::new(),
            preview_editor,
            input_editor: WidgetId::next(),
            executed_commands: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn len(&self) -> usize {
        self.current_items().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn current_items(&self) -> &im::Vector<PaletteItem> {
        if self.get_input() == "" {
            &self.total_items
        } else {
            &self.list_data.items
        }
    }

    pub fn preview(&self, ctx: &mut EventCtx) {
        if let Some(item) = self.list_data.current_selected_item() {
            item.content.select(ctx, true, self.preview_editor);
        }
    }

    pub fn get_input(&self) -> &str {
        match &self.palette_type {
            PaletteType::File
            | PaletteType::Reference
            | PaletteType::ColorTheme
            | PaletteType::IconTheme
            | PaletteType::Language
            | PaletteType::SshHost => &self.input,
            PaletteType::Line
            | PaletteType::DocumentSymbol
            | PaletteType::WorkspaceSymbol
            | PaletteType::Workspace
            | PaletteType::Command
            | PaletteType::GlobalSearch => &self.input[1..],
        }
    }
}

impl PaletteViewData {
    pub fn cancel(&mut self, ctx: &mut EventCtx) {
        match self.palette.palette_type {
            PaletteType::ColorTheme | PaletteType::IconTheme => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ReloadConfig,
                    Target::Auto,
                ));
            }
            _ => {}
        }
        let palette = Arc::make_mut(&mut self.palette);
        palette.status = PaletteStatus::Inactive;
        palette.input = "".to_string();
        palette.cursor = 0;
        palette.palette_type = PaletteType::File;
        palette.total_items.clear();
        palette.list_data.clear_items();
        if let Some(active) = *self.main_split.active_tab {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(active),
            ));
        } else {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(*self.main_split.split_id),
            ));
        }
    }

    pub fn run_references(
        &mut self,
        ctx: &mut EventCtx,
        locations: &[EditorLocation<Position>],
    ) {
        self.run(ctx, Some(PaletteType::Reference), None, true);
        let items = locations
            .iter()
            .map(|l| {
                let full_path = l.path.clone();
                let mut path = l.path.clone();
                if let Some(workspace_path) = self.workspace.path.as_ref() {
                    path = path
                        .strip_prefix(workspace_path)
                        .unwrap_or(&full_path)
                        .to_path_buf();
                }
                let filter_text = path.to_str().unwrap_or("").to_string();
                PaletteItem {
                    content: PaletteItemContent::ReferenceLocation(path, l.clone()),
                    filter_text,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();
        let palette = Arc::make_mut(&mut self.palette);
        palette.total_items = items;
        palette.preview(ctx);
        self.fill_list();
    }

    pub fn run(
        &mut self,
        ctx: &mut EventCtx,
        palette_type: Option<PaletteType>,
        input: Option<String>,
        should_init_input: bool,
    ) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.status = PaletteStatus::Started;
        palette.palette_type = palette_type.unwrap_or(PaletteType::File);
        palette.input = input.unwrap_or_else(|| palette.palette_type.string());

        // Most usages of `run` will want to initialize the input
        // However, special types like workspace-symbol-search want to avoid it
        // so that they do not cause loops, because they cause run when their
        // input changes.
        if should_init_input {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::InitPaletteInput(palette.input.clone()),
                Target::Widget(*self.main_split.tab_id),
            ));
        }
        palette.total_items.clear();
        palette.list_data.clear_items();
        palette.run_id = Uuid::new_v4().to_string();
        palette.cursor = palette.input.len();

        if let Some(active_editor_content) =
            self.main_split.active_editor().map(|e| e.content.clone())
        {
            let preview_editor = Arc::make_mut(
                self.main_split
                    .editors
                    .get_mut(&palette.preview_editor)
                    .unwrap(),
            );
            preview_editor.content = active_editor_content;
        }

        match palette.palette_type {
            PaletteType::File => {
                self.get_files(ctx);
            }
            PaletteType::Line => {
                self.get_lines(ctx);
                self.palette.preview(ctx);
            }
            PaletteType::DocumentSymbol => {
                self.get_document_symbols(ctx);
            }
            PaletteType::WorkspaceSymbol => {
                self.get_workspace_symbols(ctx);
            }
            PaletteType::Workspace => {
                self.get_workspaces(ctx);
            }
            PaletteType::Reference => {}
            PaletteType::SshHost => {
                self.get_ssh_hosts(ctx);
            }
            PaletteType::GlobalSearch => {
                self.get_global_search(ctx);
            }
            PaletteType::Command => {
                self.get_commands(ctx);
            }
            PaletteType::ColorTheme => {
                let config = self.config.clone();
                self.get_color_themes(ctx, &config);
                self.preselect_matching(ctx, &config.color_theme.name);
            }
            PaletteType::IconTheme => {
                let config = self.config.clone();
                self.get_icon_themes(ctx, &config);
                self.preselect_matching(ctx, &config.icon_theme.name);
            }
            PaletteType::Language => {
                self.get_languages(ctx);
                if let Some(editor) = self.main_split.active_editor() {
                    let doc = self.main_split.content_doc(&editor.content);
                    if let Some(syntax) = doc.syntax() {
                        let lang_name = format!("{}", syntax.language);
                        self.preselect_matching(ctx, &lang_name);
                    }
                }
            }
        }

        self.fill_list();
    }

    /// Fill the list with the stored unfiltered total items
    fn fill_list(&mut self) {
        if self.palette.input.is_empty() {
            Arc::make_mut(&mut self.palette).list_data.items =
                self.palette.total_items.clone();
        }
    }

    fn delete_backward(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        if palette.cursor == 0 {
            return;
        }

        palette.input.remove(palette.cursor - 1);
        palette.cursor -= 1;
        self.update_palette(ctx);
    }

    pub fn delete_to_beginning_of_line(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        if palette.cursor == 0 {
            return;
        }

        let start = match palette.palette_type {
            PaletteType::File
            | PaletteType::Reference
            | PaletteType::ColorTheme
            | PaletteType::IconTheme
            | PaletteType::Language
            | PaletteType::SshHost => 0,
            PaletteType::Line
            | PaletteType::DocumentSymbol
            | PaletteType::WorkspaceSymbol
            | PaletteType::Workspace
            | PaletteType::Command
            | PaletteType::GlobalSearch => 1,
        };

        if palette.cursor == start {
            palette.input = "".to_string();
            palette.cursor = 0;
        } else {
            palette.input.replace_range(start..palette.cursor, "");
            palette.cursor = start;
        }
        self.update_palette(ctx);
    }

    // TODO: This is a bit weird, its wanting to iterate over items, but it could be called before we fill the list!
    fn preselect_matching(&mut self, ctx: &mut EventCtx, matching: &str) {
        let palette = Arc::make_mut(&mut self.palette);
        if let Some((id, _)) = palette
            .total_items
            .iter()
            .enumerate()
            .find(|(_, item)| item.filter_text == matching)
        {
            palette.list_data.selected_index = id;
            palette.has_nonzero_default_index = true;
            palette.preview(ctx);
        }
    }

    pub fn select(&mut self, ctx: &mut EventCtx) {
        if self.palette.palette_type == PaletteType::Line {
            let pattern = self.palette.get_input().to_string();
            let find = Arc::make_mut(&mut self.find);
            find.visual = true;
            find.set_find(&pattern, false, false);
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::UpdateSearchInput(pattern),
                Target::Widget(*self.main_split.tab_id),
            ));
        }
        let palette = Arc::make_mut(&mut self.palette);
        if let Some(item) = palette.list_data.current_selected_item() {
            if let PaletteItemContent::Command(cmd) = &item.content {
                palette
                    .executed_commands
                    .borrow_mut()
                    .insert(cmd.kind.str().to_string(), Instant::now());
            }
            if item.content.select(ctx, false, palette.preview_editor) {
                self.cancel(ctx);
            }
        } else {
            if self.palette.palette_type == PaletteType::SshHost {
                let input = self.palette.get_input();
                let splits = input.split('@').collect::<Vec<&str>>();
                let mut splits = splits.iter().rev();
                let host = splits.next().unwrap().to_string();
                let user = splits
                    .next()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "root".to_string());
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SetWorkspace(LapceWorkspace {
                        kind: LapceWorkspaceType::RemoteSSH(user, host),
                        path: None,
                        last_open: 0,
                    }),
                    Target::Auto,
                ));
                return;
            }
            self.cancel(ctx);
        }
    }

    pub fn update_input(&mut self, ctx: &mut EventCtx, input: String) {
        let palette = Arc::make_mut(&mut self.palette);

        // WorkspaceSymbol requires sending the query to the lsp, so we refresh it when the input changes
        // If the input changed and the palette type is still/now workspace-symbol then we rerun it
        let palette_type =
            PaletteType::get_palette_type(&palette.palette_type, &input);
        if input != palette.input && palette_type == PaletteType::WorkspaceSymbol {
            self.run(ctx, Some(PaletteType::WorkspaceSymbol), Some(input), false);
            return;
        }

        // Update the current input
        palette.input = input;

        self.update_palette(ctx);
    }

    pub fn update_palette(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        if !palette.has_nonzero_default_index {
            palette.list_data.selected_index = 0;
        }
        palette.has_nonzero_default_index = false;

        let palette_type = PaletteType::get_palette_type(
            &self.palette.palette_type,
            &self.palette.input,
        );
        if self.palette.palette_type != palette_type {
            self.run(ctx, Some(palette_type), None, true);
            return;
        }

        if self.palette.get_input() == "" {
            self.palette.preview(ctx);
            Arc::make_mut(&mut self.palette).list_data.items =
                self.palette.total_items.clone();
        } else {
            // Update the filtering with the input
            let _ = self.palette.sender.send((
                self.palette.run_id.clone(),
                self.palette.get_input().to_string(),
                self.palette.total_items.clone(),
            ));
        }
    }

    fn get_files(&self, ctx: &mut EventCtx) {
        let run_id = self.palette.run_id.clone();
        let widget_id = self.palette.widget_id;
        let workspace = self.workspace.clone();
        let event_sink = ctx.get_external_handle();
        self.palette.proxy.proxy_rpc.get_files(move |result| {
            if let Ok(ProxyResponse::GetFilesResponse { items }) = result {
                let items: im::Vector<PaletteItem> = items
                    .iter()
                    .enumerate()
                    .map(|(_index, path)| {
                        let full_path = path.clone();
                        let mut path = path.clone();
                        if let Some(workspace_path) = workspace.path.as_ref() {
                            path = path
                                .strip_prefix(workspace_path)
                                .unwrap_or(&full_path)
                                .to_path_buf();
                        }
                        let filter_text = path.to_str().unwrap_or("").to_string();
                        PaletteItem {
                            content: PaletteItemContent::File(path, full_path),
                            filter_text,
                            score: 0,
                            indices: Vec::new(),
                        }
                    })
                    .collect();

                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdatePaletteItems(run_id, items),
                    Target::Widget(widget_id),
                );
            }
        });
    }

    fn get_ssh_hosts(&mut self, _ctx: &mut EventCtx) {
        let workspaces = LapceConfig::recent_workspaces().unwrap_or_default();
        let mut hosts = HashSet::new();
        for workspace in workspaces.iter() {
            if let LapceWorkspaceType::RemoteSSH(user, host) = &workspace.kind {
                hosts.insert((user.to_string(), host.to_string()));
            }
        }

        let palette = Arc::make_mut(&mut self.palette);
        palette.total_items = hosts
            .iter()
            .map(|(user, host)| PaletteItem {
                content: PaletteItemContent::SshHost(
                    user.to_string(),
                    host.to_string(),
                ),
                filter_text: format!("{user}@{host}"),
                score: 0,
                indices: vec![],
            })
            .collect();
    }

    fn get_workspaces(&mut self, _ctx: &mut EventCtx) {
        let workspaces = LapceConfig::recent_workspaces().unwrap_or_default();
        let palette = Arc::make_mut(&mut self.palette);
        palette.total_items = workspaces
            .into_iter()
            .map(|w| {
                let text = w
                    .path
                    .as_ref()
                    .unwrap()
                    .to_str()
                    .map(|p| p.to_string())
                    .unwrap();
                let filter_text = match &w.kind {
                    LapceWorkspaceType::Local => text,
                    LapceWorkspaceType::RemoteSSH(user, host) => {
                        format!("[{}@{}] {}", user, host, text)
                    }
                    LapceWorkspaceType::RemoteWSL => {
                        format!("[wsl] {text}")
                    }
                };
                PaletteItem {
                    content: PaletteItemContent::Workspace(w),
                    filter_text,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();
    }

    fn get_color_themes(&mut self, _ctx: &mut EventCtx, config: &LapceConfig) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.total_items = config
            .available_color_themes
            .values()
            .sorted_by_key(|(n, _)| n)
            .map(|(n, _)| PaletteItem {
                content: PaletteItemContent::ColorTheme(n.to_string()),
                filter_text: n.to_string(),
                score: 0,
                indices: vec![],
            })
            .collect();
    }

    fn get_icon_themes(&mut self, _ctx: &mut EventCtx, config: &LapceConfig) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.total_items = config
            .available_icon_themes
            .values()
            .sorted_by_key(|(n, _, _)| n)
            .map(|(n, _, _)| PaletteItem {
                content: PaletteItemContent::IconTheme(n.to_string()),
                filter_text: n.to_string(),
                score: 0,
                indices: vec![],
            })
            .collect();
    }

    fn get_languages(&mut self, _ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        let mut langs = LapceLanguage::languages();
        langs.push("Plain Text".to_string());
        palette.total_items = langs
            .iter()
            .sorted()
            .map(|n| PaletteItem {
                content: PaletteItemContent::Language(n.to_string()),
                filter_text: n.to_string(),
                score: 0,
                indices: vec![],
            })
            .collect();
    }

    fn get_commands(&mut self, _ctx: &mut EventCtx) {
        const EXCLUDED_ITEMS: &[&str] = &["palette.command"];

        let mut items: im::Vector<PaletteItem> = self
            .palette
            .executed_commands
            .borrow()
            .iter()
            .sorted_by_key(|(_, i)| *i)
            .rev()
            .filter_map(|(key, _)| {
                self.keypress.commands.get(key).and_then(|c| {
                    c.kind.desc().as_ref().map(|m| PaletteItem {
                        content: PaletteItemContent::Command(c.clone()),
                        filter_text: m.to_string(),
                        score: 0,
                        indices: vec![],
                    })
                })
            })
            .collect();
        items.extend(self.keypress.commands.iter().filter_map(|(_, c)| {
            if EXCLUDED_ITEMS.contains(&c.kind.str()) {
                return None;
            }

            if self
                .palette
                .executed_commands
                .borrow()
                .contains_key(c.kind.str())
            {
                return None;
            }

            c.kind.desc().as_ref().map(|m| PaletteItem {
                content: PaletteItemContent::Command(c.clone()),
                filter_text: m.to_string(),
                score: 0,
                indices: vec![],
            })
        }));

        let palette = Arc::make_mut(&mut self.palette);
        palette.total_items = items;
    }

    fn get_lines(&mut self, _ctx: &mut EventCtx) {
        if self.focus_area == FocusArea::Panel(PanelKind::Terminal) {
            if let Some(terminal) =
                self.terminal.terminals.get(&self.terminal.active_term_id)
            {
                let raw = terminal.raw.lock();
                let term = &raw.term;
                let mut items = im::Vector::new();
                let mut last_row: Option<String> = None;
                let mut current_line = term.topmost_line().0;
                for line in term.topmost_line().0..term.bottommost_line().0 {
                    let row = &term.grid()[alacritty_terminal::index::Line(line)];
                    let mut row_str = (0..row.len())
                        .map(|i| &row[alacritty_terminal::index::Column(i)])
                        .map(|c| c.c)
                        .join("");
                    if let Some(last_row) = last_row.as_ref() {
                        row_str = last_row.to_string() + &row_str;
                    } else {
                        current_line = line;
                    }
                    if row
                        .last()
                        .map(|c| c.flags.contains(Flags::WRAPLINE))
                        .unwrap_or(false)
                    {
                        last_row = Some(row_str.clone());
                    } else {
                        last_row = None;
                        let item = PaletteItem {
                            content: PaletteItemContent::TerminalLine(
                                current_line,
                                row_str.clone(),
                            ),
                            filter_text: row_str,
                            score: 0,
                            indices: vec![],
                        };
                        items.push_back(item);
                    }
                }
                let palette = Arc::make_mut(&mut self.palette);
                palette.total_items = items;
            }
            return;
        }
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };

        let doc = self.main_split.editor_doc(editor.view_id);
        let last_line_number = doc.buffer().last_line() + 1;
        let last_line_number_len = last_line_number.to_string().len();
        let palette = Arc::make_mut(&mut self.palette);
        palette.total_items = doc
            .buffer()
            .text()
            .lines(0..doc.buffer().len())
            .enumerate()
            .map(|(i, l)| {
                let line_number = i + 1;
                let text = format!(
                    "{}{} {}",
                    line_number,
                    vec![" "; last_line_number_len - line_number.to_string().len()]
                        .join(""),
                    l
                );
                PaletteItem {
                    content: PaletteItemContent::Line(line_number, text.clone()),
                    filter_text: text,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();
    }

    fn get_global_search(&mut self, _ctx: &mut EventCtx) {}

    fn get_document_symbols(&mut self, ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };

        let widget_id = self.palette.widget_id;

        if let BufferContent::File(path) = &editor.content {
            let path = path.clone();
            let run_id = self.palette.run_id.clone();
            let event_sink = ctx.get_external_handle();

            self.palette
                .proxy
                .proxy_rpc
                .get_document_symbols(path, move |result| {
                    if let Ok(ProxyResponse::GetDocumentSymbols { resp }) = result {
                        let items: im::Vector<PaletteItem> = match resp {
                            DocumentSymbolResponse::Flat(symbols) => symbols
                                .iter()
                                .map(|s| {
                                    let mut filter_text = s.name.clone();
                                    if let Some(container_name) =
                                        s.container_name.as_ref()
                                    {
                                        filter_text += container_name;
                                    }
                                    PaletteItem {
                                        content:
                                            PaletteItemContent::DocumentSymbol {
                                                kind: s.kind,
                                                name: s.name.clone(),
                                                range: s.location.range,
                                                container_name: s
                                                    .container_name
                                                    .clone(),
                                            },
                                        filter_text,
                                        score: 0,
                                        indices: Vec::new(),
                                    }
                                })
                                .collect(),
                            DocumentSymbolResponse::Nested(symbols) => symbols
                                .iter()
                                .map(|s| PaletteItem {
                                    content: PaletteItemContent::DocumentSymbol {
                                        kind: s.kind,
                                        name: s.name.clone(),
                                        range: s.range,
                                        container_name: None,
                                    },
                                    filter_text: s.name.clone(),
                                    score: 0,
                                    indices: Vec::new(),
                                })
                                .collect(),
                        };
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdatePaletteItems(run_id, items),
                            Target::Widget(widget_id),
                        );
                    }
                });
        }
    }

    fn get_workspace_symbols(&mut self, ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };

        let widget_id = self.palette.widget_id;

        // TODO: We'd like to be able to request symbols even when not in an editor.
        if let BufferContent::File(_path) = &editor.content {
            let run_id = self.palette.run_id.clone();
            let event_sink = ctx.get_external_handle();

            let query = self.palette.get_input().to_string();

            self.palette.proxy.proxy_rpc.get_workspace_symbols(
                query,
                move |result| {
                    if let Ok(ProxyResponse::GetWorkspaceSymbols { symbols }) =
                        result
                    {
                        let items: im::Vector<PaletteItem> = symbols
                            .iter()
                            .map(|s| {
                                // TODO: Should we be using filter text?
                                let mut filter_text = s.name.clone();
                                if let Some(container_name) =
                                    s.container_name.as_ref()
                                {
                                    filter_text += container_name;
                                }
                                PaletteItem {
                                    content: PaletteItemContent::WorkspaceSymbol {
                                        kind: s.kind,
                                        name: s.name.clone(),
                                        location: EditorLocation {
                                            path: path_from_url(&s.location.uri),
                                            position: Some(s.location.range.start),
                                            scroll_offset: None,
                                            history: None,
                                        },
                                        container_name: s.container_name.clone(),
                                    },
                                    filter_text,
                                    score: 0,
                                    indices: Vec::new(),
                                }
                            })
                            .collect();
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdatePaletteItems(run_id, items),
                            Target::Widget(widget_id),
                        );
                    }
                },
            );
        }
    }

    pub fn update_process(
        receiver: Receiver<(String, String, im::Vector<PaletteItem>)>,
        widget_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        fn receive_batch(
            receiver: &Receiver<(String, String, im::Vector<PaletteItem>)>,
        ) -> Result<(String, String, im::Vector<PaletteItem>)> {
            let (mut run_id, mut input, mut items) = receiver.recv()?;
            loop {
                match receiver.try_recv() {
                    Ok(update) => {
                        run_id = update.0;
                        input = update.1;
                        items = update.2;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            Ok((run_id, input, items))
        }

        let matcher = SkimMatcherV2::default().ignore_case();
        loop {
            if let Ok((run_id, input, items)) = receive_batch(&receiver) {
                let filtered_items =
                    Self::filter_items(&run_id, &input, items, &matcher);

                let _ = event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FilterPaletteItems(
                        run_id,
                        input,
                        filtered_items,
                    ),
                    Target::Widget(widget_id),
                );
            } else {
                return;
            }
        }
    }

    fn filter_items(
        _run_id: &str,
        input: &str,
        items: im::Vector<PaletteItem>,
        matcher: &SkimMatcherV2,
    ) -> im::Vector<PaletteItem> {
        // Collecting into a Vec to sort we as are hitting a worst case in
        // `im::Vector` that leads to a stack overflow
        let mut items: Vec<PaletteItem> = items
            .iter()
            .filter_map(|i| {
                if let Some((score, indices)) =
                    matcher.fuzzy_indices(&i.filter_text, input)
                {
                    let mut item = i.clone();
                    item.score = score;
                    item.indices = indices;
                    Some(item)
                } else {
                    None
                }
            })
            .collect();
        items
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Less));
        items.into()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn filter_items_can_handle_large_number_of_items() {
        let items: im::Vector<PaletteItem> = (0..100_000)
            .map(|score| PaletteItem {
                content: PaletteItemContent::ColorTheme("".to_string()),
                filter_text: "s".to_string(),
                score,
                indices: vec![],
            })
            .collect();

        let matcher = SkimMatcherV2::default().ignore_case();

        // This should not trigger a stack overflow
        // Previous implementation of this function would crash the program
        let _view = PaletteViewData::filter_items("1", "s", items, &matcher);
    }
}
