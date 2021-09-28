use anyhow::{anyhow, Result};
use bit_vec::BitVec;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use druid::{
    kurbo::{Line, Rect},
    piet::{Svg, TextAttribute},
    widget::Container,
    widget::FillStrat,
    widget::IdentityWrapper,
    widget::SvgData,
    Affine, Command, ExtEventSink, FontFamily, FontWeight, Insets, KeyEvent, Lens,
    Target, Vec2, WidgetId, WindowId,
};
use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme, BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RenderContext, Size, TextLayout, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use fzyr::{has_match, locate, Score};
use lsp_types::{DocumentSymbolResponse, Location, Position, Range, SymbolKind};
use serde_json::{self, json, Value};
use std::cmp::Ordering;
use std::fs::{self, DirEntry};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use strum::{EnumMessage, IntoEnumIterator};
use usvg;
use uuid::Uuid;

use crate::{
    command::LapceCommand,
    command::LapceUICommand,
    command::LAPCE_COMMAND,
    command::LAPCE_UI_COMMAND,
    config::{Config, LapceTheme},
    data::{
        EditorKind, LapceEditorData, LapceEditorLens, LapceEditorViewData,
        LapceMainSplitData, LapceTabData,
    },
    editor::{EditorLocationNew, LapceEditorContainer, LapceEditorView},
    keypress::{KeyPressData, KeyPressFocus},
    movement::Movement,
    proxy::LapceProxy,
    scroll::{LapceIdentityWrapper, LapceScroll, LapceScrollNew},
    state::LapceFocus,
    state::LapceWorkspace,
    state::LapceWorkspaceType,
    state::Mode,
    svg::{file_svg_new, symbol_svg_new},
    theme::OldLapceTheme,
};

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteType {
    File,
    Line,
    DocumentSymbol,
    Workspace,
    Command,
    Reference,
    Theme,
}

impl PaletteType {
    fn string(&self) -> String {
        match &self {
            PaletteType::File => "".to_string(),
            PaletteType::Line => "/".to_string(),
            PaletteType::DocumentSymbol => "@".to_string(),
            PaletteType::Workspace => ">".to_string(),
            PaletteType::Command => ":".to_string(),
            PaletteType::Reference => "".to_string(),
            PaletteType::Theme => "".to_string(),
        }
    }

    fn has_preview(&self) -> bool {
        match &self {
            PaletteType::File | PaletteType::Workspace | PaletteType::Command => {
                false
            }
            _ => true,
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
    DocumentSymbol {
        kind: SymbolKind,
        name: String,
        range: Range,
        container_name: Option<String>,
    },
    ReferenceLocation(PathBuf, EditorLocationNew),
    Workspace(LapceWorkspace),
    Command(LapceCommand),
    Theme(String),
}

impl PaletteItemContent {
    fn select(&self, ctx: &mut EventCtx, preview: bool) -> Option<PaletteType> {
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
            PaletteItemContent::DocumentSymbol {
                kind,
                name,
                range,
                container_name,
            } => {
                let kind = if preview {
                    EditorKind::PalettePreview
                } else {
                    EditorKind::SplitActive
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToPosition(kind, range.start.clone()),
                    Target::Auto,
                ));
            }
            PaletteItemContent::Line(line, _) => {
                let kind = if preview {
                    EditorKind::PalettePreview
                } else {
                    EditorKind::SplitActive
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToLine(kind, *line),
                    Target::Auto,
                ));
            }
            PaletteItemContent::ReferenceLocation(rel_path, location) => {
                let kind = if preview {
                    EditorKind::PalettePreview
                } else {
                    EditorKind::SplitActive
                };
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::JumpToLocation(kind, location.clone()),
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
                    LapceUICommand::SetTheme(theme.to_string()),
                    Target::Auto,
                ));
            }
            PaletteItemContent::Command(command) => match command {
                LapceCommand::ChangeTheme => {
                    if !preview {
                        return Some(PaletteType::Theme);
                    }
                }
                LapceCommand::OpenFolder => {
                    if !preview {
                        let event_sink = ctx.get_external_handle();
                        thread::spawn(move || {
                            if let Some(folder) =
                                tinyfiledialogs::select_folder_dialog(
                                    "Open folder",
                                    "./",
                                )
                            {
                                event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::SetWorkspace(LapceWorkspace {
                                        kind: LapceWorkspaceType::Local,
                                        path: PathBuf::from(folder),
                                    }),
                                    Target::Auto,
                                );
                            }
                        });
                    }
                }
                _ => (),
            },
        }
        None
    }

    fn paint(
        &self,
        ctx: &mut PaintCtx,
        line: usize,
        indices: &[usize],
        config: &Config,
    ) {
        let line_height = config.editor.line_height as f64;
        let (svg, text, text_indices, hint, hint_indices) = match &self {
            PaletteItemContent::File(path, _) => file_paint_items(path, indices),
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
            PaletteItemContent::ReferenceLocation(rel_path, location) => {
                file_paint_items(rel_path, indices)
            }
            PaletteItemContent::Workspace(w) => {
                let text = w.path.to_str().unwrap();
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
                    .get_message()
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

        let focus_color = Color::rgb8(0, 0, 0);

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
                    config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone()
                        .with_alpha(0.6),
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
    content: PaletteItemContent,
    filter_text: String,
    score: i64,
    indices: Vec<usize>,
}

pub struct PaletteViewLens;

#[derive(Clone, Data)]
pub struct PaletteViewData {
    pub palette: Arc<PaletteData>,
    pub workspace: Option<Arc<LapceWorkspace>>,
    pub main_split: LapceMainSplitData,
    pub keypress: Arc<KeyPressData>,
    pub config: Arc<Config>,
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
        result
    }
}

#[derive(Clone)]
pub struct PaletteData {
    pub widget_id: WidgetId,
    pub scroll_id: WidgetId,
    status: PaletteStatus,
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
        count: Option<usize>,
        env: &Env,
    ) {
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
            _ => {}
        }
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
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
            item.content.select(ctx, true);
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
            PaletteType::Line => &self.input[1..],
            PaletteType::DocumentSymbol => &self.input[1..],
            PaletteType::Workspace => &self.input[1..],
            PaletteType::Command => &self.input[1..],
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
                if let Some(workspace) = self.workspace.as_ref() {
                    path = path
                        .strip_prefix(&workspace.path)
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

        let active_path = self.main_split.active_editor().buffer.clone();
        self.main_split
            .editor_kind_mut(&EditorKind::PalettePreview)
            .buffer = active_path;

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
            &PaletteType::Line => 1,
            &PaletteType::DocumentSymbol => 1,
            &PaletteType::Workspace => 1,
            &PaletteType::Command => 1,
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
        let palette = Arc::make_mut(&mut self.palette);
        if let Some(item) = palette.get_item() {
            if let Some(palette_type) = item.content.select(ctx, false) {
                self.run(ctx, Some(palette_type));
            } else {
                self.cancel(ctx);
            }
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
            self.palette.sender.send((
                self.palette.run_id.clone(),
                self.palette.get_input().to_string(),
                self.palette.items.clone(),
            ));
        } else {
            self.palette.preview(ctx);
        }
    }

    fn get_palette_type(&self) -> PaletteType {
        if self.palette.palette_type == PaletteType::Reference {
            return PaletteType::Reference;
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
                        .map(|(index, path)| {
                            let full_path = path.clone();
                            let mut path = path.clone();
                            if let Some(workspace) = workspace.as_ref() {
                                path = path
                                    .strip_prefix(&workspace.path)
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
                    event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdatePaletteItems(run_id, items),
                        Target::Widget(widget_id),
                    );
                }
            }
        }));
    }

    fn get_workspaces(&mut self, ctx: &mut EventCtx) {
        let workspaces = vec![
            LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: PathBuf::from("/Users/Lulu/lapce"),
            },
            LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: PathBuf::from("/Users/Lulu/piet-wgpu"),
            },
            LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: PathBuf::from("/Users/Lulu/druid"),
            },
            LapceWorkspace {
                kind: LapceWorkspaceType::RemoteSSH(
                    "root".to_string(),
                    "10.154.0.5".to_string(),
                ),
                path: PathBuf::from("/root/nebula"),
            },
            LapceWorkspace {
                kind: LapceWorkspaceType::RemoteSSH(
                    "dz".to_string(),
                    "10.132.0.2".to_string(),
                ),
                path: PathBuf::from("/home/dz/go/src/galaxy"),
            },
            LapceWorkspace {
                kind: LapceWorkspaceType::RemoteSSH(
                    "dz".to_string(),
                    "10.132.0.2".to_string(),
                ),
                path: PathBuf::from("/home/dz/go/src/tardis"),
            },
            LapceWorkspace {
                kind: LapceWorkspaceType::RemoteSSH(
                    "dz".to_string(),
                    "10.132.0.2".to_string(),
                ),
                path: PathBuf::from("/home/dz/cosmos"),
            },
        ];
        let palette = Arc::make_mut(&mut self.palette);
        palette.items = workspaces
            .into_iter()
            .map(|w| {
                let text = w.path.to_str().unwrap();
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

    fn get_commands(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.items = LapceCommand::iter()
            .filter_map(|c| {
                c.get_message().map(|m| NewPaletteItem {
                    content: PaletteItemContent::Command(c.clone()),
                    filter_text: m.to_string(),
                    score: 0,
                    indices: vec![],
                })
            })
            .collect();
    }

    fn get_lines(&mut self, ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let buffer = self.main_split.open_files.get(&editor.buffer).unwrap();
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

    fn get_document_symbols(&mut self, ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let widget_id = self.palette.widget_id;
        let buffer_id = self.main_split.open_files.get(&editor.buffer).unwrap().id;
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
                        event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdatePaletteItems(run_id, items),
                            Target::Widget(widget_id),
                        );
                    }
                }
            }),
        );
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
                let filtered_items =
                    Self::filter_items(&run_id, &input, items, &matcher);
                event_sink.submit_command(
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
        run_id: &str,
        input: &str,
        items: Vec<NewPaletteItem>,
        matcher: &SkimMatcherV2,
    ) -> Vec<NewPaletteItem> {
        let mut items: Vec<NewPaletteItem> = items
            .iter()
            .filter_map(|i| {
                if let Some((score, mut indices)) =
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
    window_id: WindowId,
    tab_id: WidgetId,
    icon: PaletteIcon,
    kind: PaletteType,
    text: String,
    hint: Option<String>,
    score: Score,
    index: usize,
    match_mask: BitVec,
    position: Option<Position>,
    location: Option<Location>,
    path: Option<PathBuf>,
    workspace: Option<LapceWorkspace>,
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
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                let mut_keypress = Arc::make_mut(&mut keypress);
                let mut palette_data = data.palette_view_data();
                mut_keypress.key_down(ctx, key_event, &mut palette_data, env);
                data.palette = palette_data.palette.clone();
                data.keypress = keypress;
                data.workspace = palette_data.workspace.clone();
                data.main_split = palette_data.main_split.clone();
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
                                palette.sender.send((
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
    content_size: Size,
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
            .padding((padding, padding, padding, padding * 2.0))
            .padding((padding, padding, padding, padding))
            .lens(PaletteViewLens);
        let content = LapceIdentityWrapper::wrap(
            LapceScrollNew::new(
                NewPaletteContent::new().lens(PaletteViewLens).boxed(),
            )
            .vertical(),
            data.scroll_id,
        );
        let preview = LapceEditorView::new(preview_editor);
        Self {
            content_size: Size::ZERO,
            input: WidgetPod::new(input.boxed()),
            content: WidgetPod::new(content),
            preview: WidgetPod::new(preview.boxed()),
        }
    }

    fn ensure_item_visble(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &LapceTabData,
        env: &Env,
    ) {
        let width = ctx.size().width;
        let line_height = data.config.editor.line_height as f64;
        let rect = Size::new(width, line_height)
            .to_rect()
            .with_origin(Point::new(0.0, data.palette.index as f64 * line_height));
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

        let max_items = 15;
        let height = max_items.min(data.palette.len());
        let line_height = data.config.editor.line_height as f64;
        let height = line_height * height as f64;
        let bc = BoxConstraints::tight(Size::new(width, height));
        let content_size = self.content.layout(ctx, &bc, data, env);
        self.content
            .set_origin(ctx, data, env, Point::new(0.0, input_size.height));
        let mut content_height = content_size.height;
        if content_height > 0.0 {
            content_height += 6.0;
        }

        let max_preview_height =
            max_height - input_size.height - max_items as f64 * line_height - 6.0;
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
        let preview_size = self.preview.layout(ctx, &bc, data, env);
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

        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);

        if data.palette.current_items().len() > 0
            && data.palette.palette_type.has_preview()
        {
            self.preview.paint(ctx, data, env);
        }
    }
}

pub struct PaletteInput {
    window_id: WindowId,
    tab_id: WidgetId,
}

pub struct PaletteContent {
    window_id: WindowId,
    tab_id: WidgetId,
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
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut PaletteViewData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &PaletteViewData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &PaletteViewData,
        data: &PaletteViewData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteViewData,
        env: &Env,
    ) -> Size {
        Size::new(bc.max().width, 13.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, env: &Env) {
        let text = data.palette.input.clone();
        let cursor = data.palette.cursor;

        let text_layout = ctx
            .text()
            .new_text_layout(text)
            .font(FontFamily::SYSTEM_UI, 14.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
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
}

impl NewPaletteContent {
    pub fn new() -> Self {
        Self { mouse_down: 0 }
    }
}

impl Widget<PaletteViewData> for NewPaletteContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut PaletteViewData,
        env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                ctx.set_cursor(&druid::Cursor::Pointer);
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                let line_height = data.config.editor.line_height as f64;
                let line = (mouse_event.pos.y / line_height).floor() as usize;
                self.mouse_down = line;
                ctx.set_handled();
            }
            Event::MouseUp(mouse_event) => {
                let line_height = data.config.editor.line_height as f64;
                let line = (mouse_event.pos.y / line_height).floor() as usize;
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
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &PaletteViewData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &PaletteViewData,
        data: &PaletteViewData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteViewData,
        env: &Env,
    ) -> Size {
        let line_height = data.config.editor.line_height as f64;
        let height = line_height * data.palette.len() as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, env: &Env) {
        let line_height = data.config.editor.line_height as f64;
        let rect = ctx.region().bounding_box();
        let size = ctx.size();

        let items = data.palette.current_items();

        let start_line = (rect.y0 / line_height).floor() as usize;
        let end_line = (rect.y1 / line_height).ceil() as usize;

        for line in start_line..end_line {
            if line >= items.len() {
                break;
            }
            if line == data.palette.index {
                ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(0.0, line as f64 * line_height))
                        .with_size(Size::new(size.width, line_height)),
                    data.config.get_color_unchecked(LapceTheme::PALETTE_CURRENT),
                );
            }

            let item = &items[line];
            item.content.paint(ctx, line, &item.indices, &data.config);
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
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut PaletteViewData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &PaletteViewData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &PaletteViewData,
        data: &PaletteViewData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteViewData,
        env: &Env,
    ) -> Size {
        match data.palette.palette_type {
            PaletteType::File
            | PaletteType::Command
            | PaletteType::Workspace
            | PaletteType::Theme => Size::ZERO,
            PaletteType::DocumentSymbol
            | PaletteType::Line
            | PaletteType::Reference => bc.max(),
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, env: &Env) {}
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
    let svg = file_svg_new(
        &path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string(),
    );
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
    (svg, file_name, text_indices, folder, hint_indices)
}

pub fn svg_tree_size(svg_tree: &usvg::Tree) -> Size {
    match *svg_tree.root().borrow() {
        usvg::NodeKind::Svg(svg) => Size::new(svg.size.width(), svg.size.height()),
        _ => Size::ZERO,
    }
}
