use alacritty_terminal::{grid::Dimensions, term::cell::Flags};
use anyhow::Result;
use bit_vec::BitVec;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use druid::{
    kurbo::Rect,
    piet::{Svg, TextAttribute},
    Command, ExtEventSink, FontFamily, FontWeight, Lens,
    Target, WidgetId, WindowId,
};
use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use fzyr::Score;
use itertools::Itertools;
use lsp_types::{DocumentSymbolResponse, Location, Position, Range, SymbolKind};
use serde_json;
use std::path::PathBuf;
use std::sync::Arc;
use std::cmp::Ordering;
use std::collections::HashSet;
use usvg;
use uuid::Uuid;

use crate::{
    buffer::BufferContent,
    command::LAPCE_UI_COMMAND,
    command::{CommandExecuted, LapceCommand, LAPCE_NEW_COMMAND},
    command::{LapceCommandNew, LapceUICommand},
    config::{Config, LapceTheme},
    data::{
        FocusArea, LapceEditorData,
        LapceMainSplitData, LapceTabData, PanelKind,
    },
    editor::{EditorLocationNew, LapceEditorView},
    find::Find,
    keypress::{KeyPressData, KeyPressFocus},
    movement::Movement,
    proxy::LapceProxy,
    scroll::{LapceIdentityWrapper, LapceScrollNew},
    state::LapceWorkspace,
    state::LapceWorkspaceType,
    state::Mode,
    svg::{file_svg_new, symbol_svg_new},
    terminal::TerminalSplitData,
};

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteType {
    File,
    Line,
    GlobalSearch,
    DocumentSymbol,
    Workspace,
    Command,
    Reference,
    Theme,
    SshHost,
}

impl PaletteType {
    fn string(&self) -> String {
        match &self {
            PaletteType::File => "".to_string(),
            PaletteType::Line => "/".to_string(),
            PaletteType::DocumentSymbol => "@".to_string(),
            PaletteType::GlobalSearch => "?".to_string(),
            PaletteType::Workspace => ">".to_string(),
            PaletteType::Command => ":".to_string(),
            PaletteType::Reference => "".to_string(),
            PaletteType::Theme => "".to_string(),
            PaletteType::SshHost => "".to_string(),
        }
    }

    fn has_preview(&self) -> bool {
        match &self {
            PaletteType::Line
            | PaletteType::DocumentSymbol
            | PaletteType::GlobalSearch
            | PaletteType::Reference => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteIcon {
    File(String),
    Symbol(SymbolKind),
    None,
}

#[derive(Clone, PartialEq)]
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
    ReferenceLocation(PathBuf, EditorLocationNew),
    Workspace(LapceWorkspace),
    SshHost(String, String),
    Command(LapceCommandNew),
    Theme(String),
}

impl PaletteItemContent {
    fn select(
        &self,
        ctx: &mut EventCtx,
        preview: bool,
        preview_editor_id: WidgetId,
    ) -> Option<PaletteType> {
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
            #[allow(unused_variables)]
            PaletteItemContent::DocumentSymbol {
                kind,
                name,
                range,
                container_name,
            } => {
                let editor_id = if preview {
                    Some(preview_editor_id)
                } else {
                    None
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToPosition(editor_id, range.start.clone()),
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
                    LapceUICommand::JumpToLocation(editor_id, location.clone()),
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
            PaletteItemContent::Command(command) => {
                if !preview {
                    ctx.submit_command(Command::new(
                        LAPCE_NEW_COMMAND,
                        command.clone(),
                        Target::Auto,
                    ));
                }
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
        None
    }

    fn paint(
        &self,
        ctx: &mut PaintCtx,
        line: usize,
        indices: &[usize],
        line_height: f64,
        config: &Config,
    ) {
        let (svg, text, text_indices, hint, hint_indices) = match &self {
            PaletteItemContent::File(path, _) => file_paint_items(path, indices),
            #[allow(unused_variables)]
            PaletteItemContent::DocumentSymbol {
                kind,
                name,
                range,
                container_name,
            } => {
                let text = name.to_string();
                let hint = container_name.clone().unwrap_or("".to_string());
                let text_indices = indices
                    .iter()
                    .filter_map(|i| {
                        let i = *i;
                        if i < text.len() {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect();
                let hint_indices = indices
                    .iter()
                    .filter_map(|i| {
                        let i = *i;
                        if i >= text.len() {
                            Some(i - text.len())
                        } else {
                            None
                        }
                    })
                    .collect();
                (symbol_svg_new(kind), text, text_indices, hint, hint_indices)
            }
            PaletteItemContent::Line(_, text) => {
                (None, text.clone(), indices.to_vec(), "".to_string(), vec![])
            }
            PaletteItemContent::ReferenceLocation(rel_path, _location) => {
                file_paint_items(rel_path, indices)
            }
            PaletteItemContent::Workspace(w) => {
                let text = w.path.as_ref().unwrap().to_str().unwrap();
                let text = match &w.kind {
                    LapceWorkspaceType::Local => text.to_string(),
                    LapceWorkspaceType::RemoteSSH(user, host) => {
                        format!("[{}@{}] {}", user, host, text)
                    }
                };
                (None, text, indices.to_vec(), "".to_string(), vec![])
            }
            PaletteItemContent::Command(command) => (
                None,
                command
                    .palette_desc
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or("".to_string()),
                indices.to_vec(),
                "".to_string(),
                vec![],
            ),
            PaletteItemContent::Theme(theme) => (
                None,
                theme.to_string(),
                indices.to_vec(),
                "".to_string(),
                vec![],
            ),
            PaletteItemContent::TerminalLine(_line, content) => (
                None,
                content.clone(),
                indices.to_vec(),
                "".to_string(),
                vec![],
            ),
            PaletteItemContent::SshHost(user, host) => (
                None,
                format!("{}@{}",user,host),
                indices.to_vec(),
                "".to_string(),
                vec![],
            ),
        };

        if let Some(svg) = svg.as_ref() {
            let width = 14.0;
            let height = 14.0;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (line_height - width) / 2.0 + 5.0,
                (line_height - height) / 2.0 + line_height * line as f64,
            ));
            ctx.draw_svg(&svg, rect, None);
        }

        let svg_x = match &self {
            &PaletteItemContent::Line(_, _) | &PaletteItemContent::Workspace(_) => {
                0.0
            }
            _ => line_height,
        };

        let focus_color = config.get_color_unchecked(LapceTheme::EDITOR_FOCUS);

        let mut text_layout = ctx
            .text()
            .new_text_layout(text.clone())
            .font(FontFamily::SYSTEM_UI, 14.0)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );
        for i in &text_indices {
            let i = *i;
            text_layout = text_layout.range_attribute(
                i..i + 1,
                TextAttribute::TextColor(focus_color.clone()),
            );
            text_layout = text_layout
                .range_attribute(i..i + 1, TextAttribute::Weight(FontWeight::BOLD));
        }
        let text_layout = text_layout.build().unwrap();
        let x = svg_x + 5.0;
        let y = line_height * line as f64 + 4.0;
        let point = Point::new(x, y);
        ctx.draw_text(&text_layout, point);

        if hint != "" {
            let text_x = text_layout.size().width;
            let mut text_layout = ctx
                .text()
                .new_text_layout(hint)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(
                    config.get_color_unchecked(LapceTheme::EDITOR_DIM).clone(),
                );
            for i in &hint_indices {
                let i = *i;
                text_layout = text_layout.range_attribute(
                    i..i + 1,
                    TextAttribute::TextColor(focus_color.clone()),
                );
                text_layout = text_layout.range_attribute(
                    i..i + 1,
                    TextAttribute::Weight(FontWeight::BOLD),
                );
            }
            let text_layout = text_layout.build().unwrap();
            ctx.draw_text(
                &text_layout,
                Point::new(x + text_x + 4.0, line as f64 * line_height + 5.0),
            );
        }
    }
}

#[derive(Clone, Debug)]
pub struct NewPaletteItem {
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
        data.find = palette_view.find.clone();
        result
    }
}

#[derive(Clone)]
pub struct PaletteData {
    pub widget_id: WidgetId,
    pub scroll_id: WidgetId,
    pub status: PaletteStatus,
    proxy: Arc<LapceProxy>,
    palette_type: PaletteType,
    sender: Sender<(String, String, Vec<NewPaletteItem>)>,
    pub receiver: Option<Receiver<(String, String, Vec<NewPaletteItem>)>>,
    run_id: String,
    input: String,
    cursor: usize,
    index: usize,
    items: Vec<NewPaletteItem>,
    filtered_items: Vec<NewPaletteItem>,
    pub preview_editor: WidgetId,
}

impl KeyPressFocus for PaletteViewData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "list_focus" => true,
            "palette_focus" => true,
            _ => false,
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _env: &Env,
    ) -> CommandExecuted {
        match command {
            LapceCommand::PaletteCancel => {
                self.cancel(ctx);
            }
            LapceCommand::DeleteBackward => {
                self.delete_backward(ctx);
            }
            LapceCommand::DeleteToBeginningOfLine => {
                self.delete_to_beginning_of_line(ctx);
            }
            LapceCommand::ListNext => {
                self.next(ctx);
            }
            LapceCommand::ListPrevious => {
                self.previous(ctx);
            }
            LapceCommand::ListSelect => {
                self.select(ctx);
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.input.insert_str(palette.cursor, c);
        palette.cursor += c.len();
        self.update_palette(ctx);
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
        }
    }

    fn len(&self) -> usize {
        self.current_items().len()
    }

    pub fn current_items(&self) -> &Vec<NewPaletteItem> {
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

    pub fn get_item(&self) -> Option<&NewPaletteItem> {
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
            PaletteType::SshHost => &self.input,
            PaletteType::Line => &self.input[1..],
            PaletteType::DocumentSymbol => &self.input[1..],
            PaletteType::Workspace => &self.input[1..],
            PaletteType::Command => &self.input[1..],
            PaletteType::GlobalSearch => &self.input[1..],
        }
    }
}

impl PaletteViewData {
    fn cancel(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.status = PaletteStatus::Inactive;
        palette.input = "".to_string();
        palette.cursor = 0;
        palette.index = 0;
        palette.palette_type = PaletteType::File;
        palette.items.clear();
        palette.filtered_items.clear();
        if ctx.is_focused() {
            ctx.resign_focus();
        }
    }

    pub fn run_references(
        &mut self,
        ctx: &mut EventCtx,
        locations: &Vec<EditorLocationNew>,
    ) {
        self.run(ctx, Some(PaletteType::Reference));
        let items: Vec<NewPaletteItem> = locations
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
                NewPaletteItem {
                    content: PaletteItemContent::ReferenceLocation(
                        path.to_path_buf(),
                        l.clone(),
                    ),
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

    pub fn run(&mut self, ctx: &mut EventCtx, palette_type: Option<PaletteType>) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.status = PaletteStatus::Started;
        palette.palette_type = palette_type.unwrap_or(PaletteType::File);
        palette.input = palette.palette_type.string();
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

        match &palette.palette_type {
            &PaletteType::File => {
                self.get_files(ctx);
            }
            &PaletteType::Line => {
                self.get_lines(ctx);
                self.palette.preview(ctx);
            }
            &PaletteType::DocumentSymbol => {
                self.get_document_symbols(ctx);
            }
            &PaletteType::Workspace => {
                self.get_workspaces(ctx);
            }
            &PaletteType::Reference => {}
            &PaletteType::SshHost => {
                self.get_ssh_hosts(ctx);
            }
            &PaletteType::GlobalSearch => {
                self.get_global_search(ctx);
            }
            &PaletteType::Command => {
                self.get_commands(ctx);
            }
            &PaletteType::Theme => {
                let config = self.config.clone();
                self.get_themes(ctx, &config);
            }
        }
    }

    fn delete_backward(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        if palette.cursor == 0 {
            return;
        }

        palette.input.remove(palette.cursor - 1);
        palette.cursor = palette.cursor - 1;
        self.update_palette(ctx);
    }

    pub fn delete_to_beginning_of_line(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        if palette.cursor == 0 {
            return;
        }

        let start = match &palette.palette_type {
            &PaletteType::File => 0,
            &PaletteType::Reference => 0,
            &PaletteType::Theme => 0,
            &PaletteType::SshHost => 0,
            &PaletteType::Line => 1,
            &PaletteType::DocumentSymbol => 1,
            &PaletteType::Workspace => 1,
            &PaletteType::Command => 1,
            &PaletteType::GlobalSearch => 1,
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

    pub fn previous(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index =
            Movement::Up.update_index(palette.index, palette.len(), 1, true);
        palette.preview(ctx);
    }

    pub fn select(&mut self, ctx: &mut EventCtx) {
        if self.palette.palette_type == PaletteType::Line {
            Arc::make_mut(&mut self.find).set_find(
                self.palette.get_input(),
                false,
                false,
                false,
            );
        }
        let palette = Arc::make_mut(&mut self.palette);
        if let Some(item) = palette.get_item() {
            if let Some(palette_type) =
                item.content.select(ctx, false, palette.preview_editor)
            {
                self.run(ctx, Some(palette_type));
            } else {
                self.cancel(ctx);
            }
        } else {
            if self.palette.palette_type == PaletteType::SshHost {
                let input = self.palette.get_input();
                let splits = input.split("@").collect::<Vec<&str>>();
                let mut splits = splits.iter().rev();
                let host = splits.next().unwrap().to_string();
                let user = splits
                    .next()
                    .map(|s| s.to_string())
                    .unwrap_or("root".to_string());
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

    fn update_palette(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index = 0;
        let palette_type = self.get_palette_type();
        if self.palette.palette_type != palette_type {
            self.run(ctx, Some(palette_type));
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

    fn get_palette_type(&self) -> PaletteType {
        match self.palette.palette_type {
            PaletteType::Reference | PaletteType::SshHost => {
                return self.palette.palette_type.clone();
            }
            _ => (),
        }
        if self.palette.input == "" {
            return PaletteType::File;
        }
        match self.palette.input {
            _ if self.palette.input.starts_with("/") => PaletteType::Line,
            _ if self.palette.input.starts_with("@") => PaletteType::DocumentSymbol,
            _ if self.palette.input.starts_with(">") => PaletteType::Workspace,
            _ if self.palette.input.starts_with(":") => PaletteType::Command,
            _ => PaletteType::File,
        }
    }

    fn get_files(&self, ctx: &mut EventCtx) {
        let run_id = self.palette.run_id.clone();
        let widget_id = self.palette.widget_id;
        let workspace = self.workspace.clone();
        let event_sink = ctx.get_external_handle();
        self.palette.proxy.get_files(Box::new(move |result| {
            if let Ok(res) = result {
                let resp: Result<Vec<PathBuf>, serde_json::Error> =
                    serde_json::from_value(res);
                if let Ok(resp) = resp {
                    let items: Vec<NewPaletteItem> = resp
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
                            let filter_text =
                                path.to_str().unwrap_or("").to_string();
                            NewPaletteItem {
                                content: PaletteItemContent::File(
                                    path.to_owned(),
                                    full_path,
                                ),
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
            }
        }));
    }

    #[allow(unused_variables)]
    fn get_ssh_hosts(&mut self, ctx: &mut EventCtx) {
        let workspaces = Config::recent_workspaces().unwrap_or(Vec::new());
        let mut hosts = HashSet::new();
        for workspace in workspaces.iter() {
            match &workspace.kind {
                LapceWorkspaceType::Local => (),
                LapceWorkspaceType::RemoteSSH(user, host) => {
                    hosts.insert((user.to_string(), host.to_string()));
                }
            }
        }

        let palette = Arc::make_mut(&mut self.palette);
        palette.items = hosts
            .iter()
            .map(|(user, host)| NewPaletteItem {
                content: PaletteItemContent::SshHost(
                    user.to_string(),
                    host.to_string(),
                ),
                filter_text: format!("{}@{}",user,host),
                score: 0,
                indices: vec![],
            })
            .collect();
    }
    
    #[allow(unused_variables)]
    fn get_workspaces(&mut self, ctx: &mut EventCtx) {
        let workspaces = Config::recent_workspaces().unwrap_or(Vec::new());
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
                    LapceWorkspaceType::Local => text.to_string(),
                    LapceWorkspaceType::RemoteSSH(user, host) => {
                        format!("[{}@{}] {}", user, host, text)
                    }
                };
                NewPaletteItem {
                    content: PaletteItemContent::Workspace(w),
                    filter_text,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();
    }
    
    #[allow(unused_variables)]
    fn get_themes(&mut self, ctx: &mut EventCtx, config: &Config) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.items = config
            .themes
            .keys()
            .map(|n| NewPaletteItem {
                content: PaletteItemContent::Theme(n.to_string()),
                filter_text: n.to_string(),
                score: 0,
                indices: vec![],
            })
            .collect();
    }
    
    #[allow(unused_variables)]
    fn get_commands(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.items = self
            .keypress
            .commands
            .iter()
            .filter_map(|(_, c)| {
                c.palette_desc.as_ref().map(|m| NewPaletteItem {
                    content: PaletteItemContent::Command(c.clone()),
                    filter_text: m.to_string(),
                    score: 0,
                    indices: vec![],
                })
            })
            .collect();
    }
    
    #[allow(unused_variables)]
    fn get_lines(&mut self, ctx: &mut EventCtx) {
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
                        let item = NewPaletteItem {
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

        let buffer = self.main_split.editor_buffer(editor.view_id);
        let last_line_number = buffer.last_line() + 1;
        let last_line_number_len = last_line_number.to_string().len();
        let palette = Arc::make_mut(&mut self.palette);
        palette.items = buffer
            .rope
            .lines(0..buffer.len())
            .enumerate()
            .map(|(i, l)| {
                let line_number = i + 1;
                let text = format!(
                    "{}{} {}",
                    vec![" "; last_line_number_len - line_number.to_string().len()]
                        .join(""),
                    line_number,
                    l.to_string()
                );
                NewPaletteItem {
                    content: PaletteItemContent::Line(line_number, text.clone()),
                    filter_text: text,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();
    }
    
    #[allow(unused_variables)]
    fn get_global_search(&mut self, ctx: &mut EventCtx) {}

    fn get_document_symbols(&mut self, ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let editor = match editor {
            Some(editor) => editor,
            None => return,
        };

        let widget_id = self.palette.widget_id;

        if let BufferContent::File(path) = &editor.content {
            let path = path.clone();
            let buffer_id = self.main_split.open_files.get(&path).unwrap().id;
            let run_id = self.palette.run_id.clone();
            let event_sink = ctx.get_external_handle();

            self.palette.proxy.get_document_symbols(
                buffer_id,
                Box::new(move |result| {
                    if let Ok(res) = result {
                        let resp: Result<DocumentSymbolResponse, serde_json::Error> =
                            serde_json::from_value(res);
                        if let Ok(resp) = resp {
                            let items: Vec<NewPaletteItem> = match resp {
                                DocumentSymbolResponse::Flat(symbols) => symbols
                                    .iter()
                                    .map(|s| {
                                        let mut filter_text = s.name.clone();
                                        if let Some(container_name) =
                                            s.container_name.as_ref()
                                        {
                                            filter_text += container_name;
                                        }
                                        NewPaletteItem {
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
                                    .map(|s| NewPaletteItem {
                                        content:
                                            PaletteItemContent::DocumentSymbol {
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
                    }
                }),
            );
        }
    }

    pub fn update_process(
        receiver: Receiver<(String, String, Vec<NewPaletteItem>)>,
        widget_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        fn receive_batch(
            receiver: &Receiver<(String, String, Vec<NewPaletteItem>)>,
        ) -> Result<(String, String, Vec<NewPaletteItem>)> {
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
                let filtered_items = Self::filter_items(&run_id, &input, items, &matcher);
                
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
    
    #[allow(unused_variables)]
    fn filter_items(
        run_id: &str,
        input: &str,
        items: Vec<NewPaletteItem>,
        matcher: &SkimMatcherV2,
    ) -> Vec<NewPaletteItem> {
        let mut items: Vec<NewPaletteItem> = items
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

#[derive(Clone, Debug)]
pub struct PaletteItem {
    #[allow(dead_code)]
    window_id: WindowId,
    
    #[allow(dead_code)]
    tab_id: WidgetId,
    
    #[allow(dead_code)]
    icon: PaletteIcon,
    
    #[allow(dead_code)]
    kind: PaletteType,
    
    #[allow(dead_code)]
    text: String,
    
    #[allow(dead_code)]
    hint: Option<String>,
    
    #[allow(dead_code)]
    score: Score,
    
    #[allow(dead_code)]
    index: usize,
    
    #[allow(dead_code)]
    match_mask: BitVec,
    
    #[allow(dead_code)]
    position: Option<Position>,
    
    #[allow(dead_code)]
    location: Option<Location>,
    
    #[allow(dead_code)]
    path: Option<PathBuf>,
    
    #[allow(dead_code)]
    workspace: Option<LapceWorkspace>,
    
    #[allow(dead_code)]
    command: Option<LapceCommand>,
}

pub struct NewPalette {
    widget_id: WidgetId,
    container: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl NewPalette {
    pub fn new(data: &PaletteData, preview_editor: &LapceEditorData) -> Self {
        let container = PaletteContainer::new(data, preview_editor);
        Self {
            widget_id: data.widget_id,
            container: WidgetPod::new(container).boxed(),
        }
    }
}

impl Widget<LapceTabData> for NewPalette {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseDown(_)
            | Event::MouseMove(_)
            | Event::Wheel(_)
            | Event::MouseUp(_) => {
                if data.palette.status == PaletteStatus::Inactive {
                    return;
                }
            }
            _ => (),
        }
        match event {
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                let mut_keypress = Arc::make_mut(&mut keypress);
                let mut palette_data = data.palette_view_data();
                mut_keypress.key_down(ctx, key_event, &mut palette_data, env);
                data.palette = palette_data.palette.clone();
                data.keypress = keypress;
                data.workspace = palette_data.workspace.clone();
                data.main_split = palette_data.main_split.clone();
                data.find = palette_data.find.clone();
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::RunPalette(palette_type) => {
                        ctx.request_focus();
                        ctx.set_handled();
                        let mut palette_data = data.palette_view_data();
                        palette_data.run(ctx, palette_type.to_owned());
                        data.palette = palette_data.palette.clone();
                        data.keypress = palette_data.keypress.clone();
                        data.workspace = palette_data.workspace.clone();
                        data.main_split = palette_data.main_split.clone();
                    }
                    LapceUICommand::RunPaletteReferences(locations) => {
                        ctx.request_focus();
                        let mut palette_data = data.palette_view_data();
                        palette_data.run_references(ctx, locations);
                        data.palette = palette_data.palette.clone();
                        data.keypress = palette_data.keypress.clone();
                        data.workspace = palette_data.workspace.clone();
                        data.main_split = palette_data.main_split.clone();
                    }
                    LapceUICommand::CancelPalette => {
                        let mut palette_data = data.palette_view_data();
                        palette_data.cancel(ctx);
                        data.palette = palette_data.palette.clone();
                    }
                    LapceUICommand::UpdatePaletteItems(run_id, items) => {
                        let palette = Arc::make_mut(&mut data.palette);
                        if &palette.run_id == run_id {
                            palette.items = items.to_owned();
                            palette.preview(ctx);
                            if palette.get_input() != "" {
                                let _ = palette.sender.send((
                                    palette.run_id.clone(),
                                    palette.get_input().to_string(),
                                    palette.items.clone(),
                                ));
                            }
                        }
                    }
                    LapceUICommand::FilterPaletteItems(
                        run_id,
                        input,
                        filtered_items,
                    ) => {
                        let palette = Arc::make_mut(&mut data.palette);
                        if &palette.run_id == run_id && &palette.get_input() == input
                        {
                            palette.filtered_items = filtered_items.to_owned();
                            palette.preview(ctx);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.container.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        match event {
            LifeCycle::FocusChanged(is_focused) => {
                ctx.request_paint();
                if !is_focused {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::CancelPalette,
                        Target::Widget(data.palette.widget_id),
                    ));
                }
            }
            _ => (),
        }
        self.container.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if !old_data.palette.same(&data.palette) {
            ctx.request_local_layout();
            ctx.request_paint();
        }

        self.container.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let width = 600.0;
        let self_size = Size::new(width, bc.max().height);

        let bc = BoxConstraints::tight(self_size);
        self.container.layout(ctx, &bc, data, env);
        self.container.set_origin(ctx, data, env, Point::ZERO);
        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.palette.status == PaletteStatus::Inactive {
            return;
        }

        self.container.paint(ctx, data, env);
    }
}

pub struct PaletteContainer {
    input_size: Size,
    content_size: Size,
    line_height: f64,
    input: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    content: WidgetPod<
        LapceTabData,
        LapceIdentityWrapper<
            LapceScrollNew<LapceTabData, Box<dyn Widget<LapceTabData>>>,
        >,
    >,
    preview: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
}

impl PaletteContainer {
    pub fn new(data: &PaletteData, preview_editor: &LapceEditorData) -> Self {
        let padding = 6.0;
        let input = NewPaletteInput::new()
            .padding((padding, padding, padding, padding))
            .padding((padding, padding, padding, padding))
            .lens(PaletteViewLens);
        let content = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(
                NewPaletteContent::new().lens(PaletteViewLens).boxed(),
            )
            .vertical(),
            data.scroll_id,
        );
        let preview = LapceEditorView::new(preview_editor.view_id);
        Self {
            input_size: Size::ZERO,
            content_size: Size::ZERO,
            input: WidgetPod::new(input.boxed()),
            content: WidgetPod::new(content),
            preview: WidgetPod::new(preview.boxed()),
            line_height: 25.0,
        }
    }

    fn ensure_item_visble(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &LapceTabData,
        env: &Env,
    ) {
        let width = ctx.size().width;
        let rect =
            Size::new(width, self.line_height)
                .to_rect()
                .with_origin(Point::new(
                    0.0,
                    data.palette.index as f64 * self.line_height,
                ));
        if self
            .content
            .widget_mut()
            .inner_mut()
            .scroll_to_visible(rect, env)
        {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::ResetFade,
                Target::Widget(data.palette.scroll_id),
            ));
        }
    }
}

impl Widget<LapceTabData> for PaletteContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.input.event(ctx, event, data, env);
        self.content.event(ctx, event, data, env);
        self.preview.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.input.lifecycle(ctx, event, data, env);
        self.content.lifecycle(ctx, event, data, env);
        self.preview.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if old_data.palette.input != data.palette.input
            || old_data.palette.index != data.palette.index
        {
            self.ensure_item_visble(ctx, data, env);
        }
        self.input.update(ctx, data, env);
        self.content.update(ctx, data, env);
        self.preview.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let width = 600.0;
        let max_height = bc.max().height;

        let bc = BoxConstraints::tight(Size::new(width, bc.max().height));
        let input_size = self.input.layout(ctx, &bc, data, env);
        self.input.set_origin(ctx, data, env, Point::ZERO);
        self.input_size = input_size;

        let max_items = 15;
        let height = max_items.min(data.palette.len());
        let height = self.line_height * height as f64;
        let bc = BoxConstraints::tight(Size::new(width, height));
        let content_size = self.content.layout(ctx, &bc, data, env);
        self.content
            .set_origin(ctx, data, env, Point::new(0.0, input_size.height));
        let mut content_height = content_size.height;
        if content_height > 0.0 {
            content_height += 6.0;
        }

        let max_preview_height = max_height
            - input_size.height
            - max_items as f64 * self.line_height
            - 6.0;
        let preview_height = if data.palette.palette_type.has_preview() {
            if content_height > 0.0 {
                max_preview_height
            } else {
                0.0
            }
        } else {
            0.0
        };
        let bc = BoxConstraints::tight(Size::new(width, max_preview_height));
        let _preview_size = self.preview.layout(ctx, &bc, data, env);
        self.preview.set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, input_size.height + content_height),
        );

        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));
        let self_size =
            Size::new(width, input_size.height + content_height + preview_height);
        self.content_size = self_size;
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let shadow_width = 5.0;
        let rect = self.content_size.to_rect();
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PALETTE_BACKGROUND),
        );
        ctx.fill(
            self.input_size.to_rect().inflate(-6.0, -6.0),
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
        );

        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);

        if data.palette.current_items().len() > 0
            && data.palette.palette_type.has_preview()
        {
            let rect = self.preview.layout_rect();
            ctx.fill(
                rect,
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );
            self.preview.paint(ctx, data, env);
        }
    }
}

pub struct PaletteInput {
    #[allow(dead_code)]
    window_id: WindowId,
    
    #[allow(dead_code)]
    tab_id: WidgetId,
}

pub struct PaletteContent {
    #[allow(dead_code)]
    window_id: WindowId,
    
    #[allow(dead_code)]
    tab_id: WidgetId,
    
    #[allow(dead_code)]
    max_items: usize,
}

pub struct NewPaletteInput {}

impl NewPaletteInput {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget<PaletteViewData> for NewPaletteInput {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut PaletteViewData,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &PaletteViewData,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &PaletteViewData,
        _env: &Env,
    ) -> Size {
        Size::new(bc.max().width, 14.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, _env: &Env) {
        let text = data.palette.input.clone();
        let cursor = data.palette.cursor;

        let text_layout =
            if text == "" && data.palette.palette_type == PaletteType::SshHost {
                ctx.text()
                    .new_text_layout(
                        "Enter your SSH details, like user@host".to_string(),
                    )
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_DIM)
                            .clone(),
                    )
                    .build()
                    .unwrap()
            } else {
                ctx.text()
                    .new_text_layout(text)
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap()
            };

        let line = text_layout.cursor_line_for_text_position(cursor);
        ctx.stroke(
            line,
            data.config
                .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            1.0,
        );
        ctx.draw_text(&text_layout, Point::new(0.0, 0.0));
    }
}

pub struct NewPaletteContent {
    mouse_down: usize,
    line_height: f64,
}

impl NewPaletteContent {
    pub fn new() -> Self {
        Self {
            mouse_down: 0,
            line_height: 25.0,
        }
    }
}

impl Widget<PaletteViewData> for NewPaletteContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut PaletteViewData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(_mouse_event) => {
                ctx.set_cursor(&druid::Cursor::Pointer);
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                let line = (mouse_event.pos.y / self.line_height).floor() as usize;
                self.mouse_down = line;
                ctx.set_handled();
            }
            Event::MouseUp(mouse_event) => {
                let line = (mouse_event.pos.y / self.line_height).floor() as usize;
                if line == self.mouse_down {
                    let palette = Arc::make_mut(&mut data.palette);
                    palette.index = line;
                    data.select(ctx);
                }
                ctx.set_handled();
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &PaletteViewData,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteViewData,
        _env: &Env,
    ) -> Size {
        let height = self.line_height * data.palette.len() as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, _env: &Env) {
        let rect = ctx.region().bounding_box();
        let size = ctx.size();

        let items = data.palette.current_items();

        let start_line = (rect.y0 / self.line_height).floor() as usize;
        let end_line = (rect.y1 / self.line_height).ceil() as usize;

        for line in start_line..end_line {
            if line >= items.len() {
                break;
            }
            if line == data.palette.index {
                ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(0.0, line as f64 * self.line_height))
                        .with_size(Size::new(size.width, self.line_height)),
                    data.config.get_color_unchecked(LapceTheme::PALETTE_CURRENT),
                );
            }

            let item = &items[line];
            item.content.paint(
                ctx,
                line,
                &item.indices,
                self.line_height,
                &data.config,
            );
        }
    }
}

pub struct PalettePreview {}

impl PalettePreview {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget<PaletteViewData> for PalettePreview {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut PaletteViewData,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &PaletteViewData,
        _data: &PaletteViewData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteViewData,
        _env: &Env,
    ) -> Size {
        if data.palette.palette_type.has_preview() {
            bc.max()
        } else {
            Size::ZERO
        }
    }

    fn paint(&mut self, _ctx: &mut PaintCtx, _data: &PaletteViewData, _env: &Env) {}
}

impl PaletteInput {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> PaletteInput {
        PaletteInput { window_id, tab_id }
    }
}

impl PaletteContent {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        max_items: usize,
    ) -> PaletteContent {
        PaletteContent {
            window_id,
            tab_id,
            max_items,
        }
    }
}

fn file_paint_items(
    path: &PathBuf,
    indices: &[usize],
) -> (Option<Svg>, String, Vec<usize>, String, Vec<usize>) {
    let svg = file_svg_new(path);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let folder = path
        .parent()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let folder_len = folder.len();
    let text_indices: Vec<usize> = indices
        .iter()
        .filter_map(|i| {
            let i = *i;
            if folder_len > 0 {
                if i > folder_len {
                    Some(i - folder_len - 1)
                } else {
                    None
                }
            } else {
                Some(i)
            }
        })
        .collect();
    let hint_indices: Vec<usize> = indices
        .iter()
        .filter_map(|i| {
            let i = *i;
            if i < folder_len {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    (Some(svg), file_name, text_indices, folder, hint_indices)
}

pub fn svg_tree_size(svg_tree: &usvg::Tree) -> Size {
    match *svg_tree.root().borrow() {
        usvg::NodeKind::Svg(svg) => Size::new(svg.size.width(), svg.size.height()),
        _ => Size::ZERO,
    }
}
