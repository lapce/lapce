use alacritty_terminal::{grid::Dimensions, term::cell::Flags};
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use druid::{Command, ExtEventSink, Lens, Modifiers, Target, WidgetId};
use druid::{Data, Env, EventCtx};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use itertools::Itertools;
use lapce_core::command::{EditCommand, FocusCommand};
use lapce_core::language::LapceLanguage;
use lapce_core::mode::Mode;
use lapce_core::movement::Movement;
use lapce_rpc::proxy::ProxyResponse;
use lsp_types::{DocumentSymbolResponse, Position, Range, SymbolKind};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use crate::command::CommandKind;
use crate::data::{LapceWorkspace, LapceWorkspaceType};
use crate::document::BufferContent;
use crate::editor::EditorLocation;
use crate::panel::PanelKind;
use crate::proxy::path_from_url;
use crate::{
    command::LAPCE_UI_COMMAND,
    command::{CommandExecuted, LAPCE_COMMAND},
    command::{LapceCommand, LapceUICommand},
    config::Config,
    data::{FocusArea, LapceMainSplitData, LapceTabData},
    find::Find,
    keypress::{KeyPressData, KeyPressFocus},
    proxy::LapceProxy,
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
    Theme,
    SshHost,
    Language,
}

impl PaletteType {
    fn string(&self) -> String {
        match &self {
            PaletteType::File => "".to_string(),
            PaletteType::Line => "/".to_string(),
            PaletteType::DocumentSymbol => "@".to_string(),
            PaletteType::WorkspaceSymbol => "#".to_string(),
            PaletteType::GlobalSearch => "?".to_string(),
            PaletteType::Workspace => ">".to_string(),
            PaletteType::Command => ":".to_string(),
            PaletteType::Reference => "".to_string(),
            PaletteType::Theme => "".to_string(),
            PaletteType::SshHost => "".to_string(),
            PaletteType::Language => "".to_string(),
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
            | PaletteType::Theme
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

#[derive(Clone, Debug)]
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
    Theme(String),
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
                        LapceUICommand::OpenFile(full_path.clone()),
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
                    LapceUICommand::JumpToPosition(editor_id, range.start),
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
                    LapceUICommand::JumpToLspLocation(editor_id, location.clone()),
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
                    LapceUICommand::JumpToLspLocation(editor_id, location.clone()),
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
            PaletteItemContent::Theme(theme) => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SetTheme(theme.to_string(), preview),
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

#[derive(Clone, Debug)]
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
    pub config: Arc<Config>,
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

#[derive(Clone)]
pub struct PaletteData {
    pub widget_id: WidgetId,
    pub scroll_id: WidgetId,
    pub status: PaletteStatus,
    pub proxy: Arc<LapceProxy>,
    pub palette_type: PaletteType,
    pub sender: Sender<(String, String, Vec<PaletteItem>)>,
    pub receiver: Option<Receiver<(String, String, Vec<PaletteItem>)>>,
    pub run_id: String,
    pub input: String,
    pub cursor: usize,
    pub index: usize,
    pub items: Vec<PaletteItem>,
    pub filtered_items: Vec<PaletteItem>,
    pub preview_editor: WidgetId,
    pub input_editor: WidgetId,
}

impl KeyPressFocus for PaletteViewData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "list_focus" | "palette_focus" | "modal_focus")
    }

    // fn run_command(
    //     &mut self,
    //     ctx: &mut EventCtx,
    //     command: &LapceCommand,
    //     _count: Option<usize>,
    //     _mods: Modifiers,
    //     _env: &Env,
    // ) -> CommandExecuted {
    //     match command {
    //         LapceCommand::ModalClose => {
    //             self.cancel(ctx);
    //         }
    //         LapceCommand::DeleteBackward => {
    //             self.delete_backward(ctx);
    //         }
    //         LapceCommand::DeleteToBeginningOfLine => {
    //             self.delete_to_beginning_of_line(ctx);
    //         }
    //         LapceCommand::ListNext => {
    //             self.next(ctx);
    //         }
    //         LapceCommand::ListPrevious => {
    //             self.previous(ctx);
    //         }
    //         LapceCommand::ListSelect => {
    //             self.select(ctx);
    //         }
    //         _ => return CommandExecuted::No,
    //     }
    //     CommandExecuted::Yes
    // }

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
        match &command.kind {
            CommandKind::Focus(cmd) => match cmd {
                FocusCommand::ModalClose => {
                    self.cancel(ctx);
                }
                FocusCommand::ListNext => {
                    self.next(ctx);
                }
                FocusCommand::ListNextPage => {
                    self.next_page(ctx);
                }
                FocusCommand::ListPrevious => {
                    self.previous(ctx);
                }
                FocusCommand::ListPreviousPage => {
                    self.previous_page(ctx);
                }
                FocusCommand::ListSelect => {
                    self.select(ctx);
                }
                _ => return CommandExecuted::No,
            },
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
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let (sender, receiver) = unbounded();
        let widget_id = WidgetId::next();
        let scroll_id = WidgetId::next();
        let preview_editor = WidgetId::next();
        Self {
            widget_id,
            scroll_id,
            status: PaletteStatus::Inactive,
            proxy,
            palette_type: PaletteType::File,
            sender,
            receiver: Some(receiver),
            run_id: Uuid::new_v4().to_string(),
            input: "".to_string(),
            cursor: 0,
            index: 0,
            items: Vec::new(),
            filtered_items: Vec::new(),
            preview_editor,
            input_editor: WidgetId::next(),
        }
    }

    pub fn len(&self) -> usize {
        self.current_items().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn current_items(&self) -> &Vec<PaletteItem> {
        if self.get_input() == "" {
            &self.items
        } else {
            &self.filtered_items
        }
    }

    pub fn preview(&self, ctx: &mut EventCtx) {
        if let Some(item) = self.get_item() {
            item.content.select(ctx, true, self.preview_editor);
        }
    }

    pub fn get_item(&self) -> Option<&PaletteItem> {
        let items = self.current_items();
        if items.is_empty() {
            return None;
        }
        Some(&items[self.index])
    }

    pub fn get_input(&self) -> &str {
        match &self.palette_type {
            PaletteType::File => &self.input,
            PaletteType::Reference => &self.input,
            PaletteType::Theme => &self.input,
            PaletteType::Language => &self.input,
            PaletteType::SshHost => &self.input,
            PaletteType::Line => &self.input[1..],
            PaletteType::DocumentSymbol => &self.input[1..],
            PaletteType::WorkspaceSymbol => &self.input[1..],
            PaletteType::Workspace => &self.input[1..],
            PaletteType::Command => &self.input[1..],
            PaletteType::GlobalSearch => &self.input[1..],
        }
    }
}

// TODO: Make this configurable
/// The maximum number of palette items to display per 'page'
pub const MAX_PALETTE_ITEMS: usize = 15;
impl PaletteViewData {
    pub fn cancel(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.status = PaletteStatus::Inactive;
        palette.input = "".to_string();
        palette.cursor = 0;
        palette.index = 0;
        palette.palette_type = PaletteType::File;
        palette.items.clear();
        palette.filtered_items.clear();
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
        self.run(ctx, Some(PaletteType::Reference), None);
        let items: Vec<PaletteItem> = locations
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
        palette.items = items;
        palette.preview(ctx);
    }

    pub fn run(
        &mut self,
        ctx: &mut EventCtx,
        palette_type: Option<PaletteType>,
        input: Option<String>,
    ) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.status = PaletteStatus::Started;
        palette.palette_type = palette_type.unwrap_or(PaletteType::File);
        palette.input = input.unwrap_or_else(|| palette.palette_type.string());
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::InitPaletteInput(palette.input.clone()),
            Target::Widget(*self.main_split.tab_id),
        ));
        palette.items = Vec::new();
        palette.filtered_items = Vec::new();
        palette.run_id = Uuid::new_v4().to_string();
        palette.cursor = palette.input.len();
        palette.index = 0;

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
            PaletteType::Theme => {
                let config = self.config.clone();
                self.get_themes(ctx, &config);
            }
            PaletteType::Language => {
                self.get_languages(ctx);
            }
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
            PaletteType::File => 0,
            PaletteType::Reference => 0,
            PaletteType::Theme => 0,
            PaletteType::Language => 0,
            PaletteType::SshHost => 0,
            PaletteType::Line => 1,
            PaletteType::DocumentSymbol => 1,
            PaletteType::WorkspaceSymbol => 1,
            PaletteType::Workspace => 1,
            PaletteType::Command => 1,
            PaletteType::GlobalSearch => 1,
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

    pub fn next(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index =
            Movement::Down.update_index(palette.index, palette.len(), 1, true);
        palette.preview(ctx);
    }

    pub fn next_page(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index = Movement::Down.update_index(
            palette.index,
            palette.len(),
            MAX_PALETTE_ITEMS - 1,
            false,
        );
        palette.preview(ctx);
    }

    pub fn previous(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index =
            Movement::Up.update_index(palette.index, palette.len(), 1, true);
        palette.preview(ctx);
    }

    pub fn previous_page(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index = Movement::Up.update_index(
            palette.index,
            palette.len(),
            MAX_PALETTE_ITEMS - 1,
            false,
        );
        palette.preview(ctx);
    }

    pub fn select(&mut self, ctx: &mut EventCtx) {
        if self.palette.palette_type == PaletteType::Line {
            let pattern = self.palette.get_input().to_string();
            let find = Arc::make_mut(&mut self.find);
            find.visual = true;
            find.set_find(&pattern, false, false, false);
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::UpdateSearchInput(pattern),
                Target::Widget(*self.main_split.tab_id),
            ));
        }
        let palette = Arc::make_mut(&mut self.palette);
        if let Some(item) = palette.get_item() {
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
            self.run(ctx, Some(PaletteType::WorkspaceSymbol), Some(input));
            return;
        }

        // Update the current input
        palette.input = input;

        self.update_palette(ctx)
    }

    pub fn update_palette(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index = 0;

        let palette_type = PaletteType::get_palette_type(
            &self.palette.palette_type,
            &self.palette.input,
        );
        if self.palette.palette_type != palette_type {
            self.run(ctx, Some(palette_type), None);
            return;
        }

        if self.palette.get_input() != "" {
            let _ = self.palette.sender.send((
                self.palette.run_id.clone(),
                self.palette.get_input().to_string(),
                self.palette.items.clone(),
            ));
        } else {
            self.palette.preview(ctx);
        }
    }

    fn get_files(&self, ctx: &mut EventCtx) {
        let run_id = self.palette.run_id.clone();
        let widget_id = self.palette.widget_id;
        let workspace = self.workspace.clone();
        let event_sink = ctx.get_external_handle();
        self.palette.proxy.proxy_rpc.get_files(move |result| {
            if let Ok(ProxyResponse::GetFilesResponse { items }) = result {
                let items: Vec<PaletteItem> = items
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
        let workspaces = Config::recent_workspaces().unwrap_or_default();
        let mut hosts = HashSet::new();
        for workspace in workspaces.iter() {
            if let LapceWorkspaceType::RemoteSSH(user, host) = &workspace.kind {
                hosts.insert((user.to_string(), host.to_string()));
            }
        }

        let palette = Arc::make_mut(&mut self.palette);
        palette.items = hosts
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
        let workspaces = Config::recent_workspaces().unwrap_or_default();
        let palette = Arc::make_mut(&mut self.palette);
        palette.items = workspaces
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

    fn get_themes(&mut self, _ctx: &mut EventCtx, config: &Config) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.items = config
            .available_themes
            .values()
            .sorted_by_key(|(n, _)| n)
            .map(|(n, _)| PaletteItem {
                content: PaletteItemContent::Theme(n.to_string()),
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
        palette.items = langs
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

        let palette = Arc::make_mut(&mut self.palette);
        palette.items = self
            .keypress
            .commands
            .iter()
            .filter_map(|(_, c)| {
                if EXCLUDED_ITEMS.contains(&c.kind.str()) {
                    return None;
                }

                c.kind.desc().as_ref().map(|m| PaletteItem {
                    content: PaletteItemContent::Command(c.clone()),
                    filter_text: m.to_string(),
                    score: 0,
                    indices: vec![],
                })
            })
            .collect();
    }

    fn get_lines(&mut self, _ctx: &mut EventCtx) {
        if self.focus_area == FocusArea::Panel(PanelKind::Terminal) {
            if let Some(terminal) =
                self.terminal.terminals.get(&self.terminal.active_term_id)
            {
                let raw = terminal.raw.lock();
                let term = &raw.term;
                let mut items = Vec::new();
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
                        items.push(item);
                    }
                }
                let palette = Arc::make_mut(&mut self.palette);
                palette.items = items;
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
        palette.items = doc
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
                        let items: Vec<PaletteItem> = match resp {
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
                        let items: Vec<PaletteItem> = symbols
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
        receiver: Receiver<(String, String, Vec<PaletteItem>)>,
        widget_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        fn receive_batch(
            receiver: &Receiver<(String, String, Vec<PaletteItem>)>,
        ) -> Result<(String, String, Vec<PaletteItem>)> {
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
        items: Vec<PaletteItem>,
        matcher: &SkimMatcherV2,
    ) -> Vec<PaletteItem> {
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
        items
    }
}
