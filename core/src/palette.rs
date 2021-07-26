use anyhow::{anyhow, Result};
use bit_vec::BitVec;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use druid::{
    kurbo::{Line, Rect},
    piet::TextAttribute,
    widget::Container,
    widget::FillStrat,
    widget::IdentityWrapper,
    widget::Svg,
    widget::SvgData,
    Affine, Command, ExtEventSink, FontFamily, FontWeight, Insets, KeyEvent, Lens,
    Target, Vec2, WidgetId, WindowId,
};
use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme,
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll},
    BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
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
use usvg;
use uuid::Uuid;

use crate::{
    command::LapceCommand,
    command::LapceUICommand,
    command::LAPCE_COMMAND,
    command::LAPCE_UI_COMMAND,
    data::{LapceMainSplitData, LapceTabData},
    editor::EditorSplitState,
    explorer::ICONS_DIR,
    keypress::{KeyPressData, KeyPressFocus},
    movement::Movement,
    proxy::LapceProxy,
    scroll::{LapceScroll, LapceScrollNew},
    state::LapceFocus,
    state::LapceWorkspace,
    state::LapceWorkspaceType,
    state::LAPCE_APP_STATE,
    state::{LapceUIState, Mode},
    svg::{file_svg_new, symbol_svg_new},
    theme::LapceTheme,
};

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteType {
    File,
    Line,
    DocumentSymbol,
    Workspace,
    Command,
    Reference,
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
    DocumentSymbol {
        kind: SymbolKind,
        name: String,
        range: Range,
        container_name: Option<String>,
    },
}

impl PaletteItemContent {
    fn select(&self, ctx: &mut EventCtx) {
        match &self {
            PaletteItemContent::File(_, full_path) => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::OpenFile(full_path.clone()),
                    Target::Auto,
                ));
            }
            PaletteItemContent::DocumentSymbol {
                kind,
                name,
                range,
                container_name,
            } => todo!(),
        }
    }

    fn paint(&self, ctx: &mut PaintCtx, line: usize, indices: &[usize], env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let (svg, text, text_indices, hint, hint_indices) = match &self {
            PaletteItemContent::File(path, _) => {
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
        };

        if let Some(svg) = svg.as_ref() {
            let width = 14.0;
            let height = 14.0;
            let rect = Size::new(width, height).to_rect().with_origin(Point::new(
                (line_height - width) / 2.0 + 5.0,
                (line_height - height) / 2.0 + line_height * line as f64,
            ));
            svg.paint(ctx, rect, None);
        }

        let focus_color = Color::rgb8(0, 0, 0);

        let mut text_layout = ctx
            .text()
            .new_text_layout(text.clone())
            .font(FontFamily::SYSTEM_UI, 14.0)
            .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
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
        let x = line_height + 5.0;
        let y = line_height * line as f64 + 4.0;
        let point = Point::new(x, y);
        ctx.draw_text(&text_layout, point);

        if hint != "" {
            let text_x = text_layout.hit_test_text_position(text.len()).point.x;
            let mut text_layout = ctx
                .text()
                .new_text_layout(hint)
                .font(FontFamily::SYSTEM_UI, 13.0)
                .text_color(env.get(LapceTheme::EDITOR_FOREGROUND).with_alpha(0.6));
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
    pub workspace: Arc<LapceWorkspace>,
    pub main_split: LapceMainSplitData,
    pub keypress: Arc<KeyPressData>,
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
        result
    }
}

#[derive(Clone)]
pub struct PaletteData {
    widget_id: WidgetId,
    status: PaletteStatus,
    proxy: Arc<LapceProxy>,
    palette_type: PaletteType,
    sender: Sender<(String, String, Vec<NewPaletteItem>)>,
    receiver: Option<Receiver<(String, String, Vec<NewPaletteItem>)>>,
    run_id: String,
    input: String,
    cursor: usize,
    index: usize,
    items: Vec<NewPaletteItem>,
    filtered_items: Vec<NewPaletteItem>,
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
                self.next();
            }
            LapceCommand::ListPrevious => {
                self.previous();
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
        Self {
            widget_id,
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
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::CancelPalette,
            Target::Widget(palette.widget_id),
        ));
    }

    pub fn run(&mut self, ctx: &mut EventCtx, palette_type: Option<PaletteType>) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.status = PaletteStatus::Started;
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::RunPalette,
            Target::Widget(palette.widget_id),
        ));
        palette.palette_type = palette_type.unwrap_or(PaletteType::File);
        palette.input = palette.palette_type.string();
        palette.items = Vec::new();
        palette.filtered_items = Vec::new();
        palette.run_id = Uuid::new_v4().to_string();
        palette.cursor = palette.input.len();
        match &palette.palette_type {
            &PaletteType::DocumentSymbol => {
                self.get_document_symbols(ctx);
            }
            _ => self.get_files(ctx),
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

    pub fn next(&mut self) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index =
            Movement::Down.update_index(palette.index, palette.len(), 1, true);
    }

    pub fn previous(&mut self) {
        let palette = Arc::make_mut(&mut self.palette);
        palette.index =
            Movement::Up.update_index(palette.index, palette.len(), 1, true);
    }

    pub fn select(&mut self, ctx: &mut EventCtx) {
        let palette = Arc::make_mut(&mut self.palette);
        if let Some(item) = palette.get_item() {
            item.content.select(ctx);
        }
        self.cancel(ctx);
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
        let workspace = (*self.workspace).clone();
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
                            let path =
                                path.strip_prefix(&workspace.path).unwrap_or(path);
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

    fn get_document_symbols(&self, ctx: &mut EventCtx) {
        let editor = self.main_split.active_editor();
        let widget_id = self.palette.widget_id;
        let buffer_id = self.main_split.open_files.get(&editor.buffer).unwrap();
        let run_id = self.palette.run_id.clone();
        let event_sink = ctx.get_external_handle();
        self.palette.proxy.get_document_symbols(
            *buffer_id,
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
        ) -> (String, String, Vec<NewPaletteItem>) {
            let (mut run_id, mut input, mut items) = receiver.recv().unwrap();
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
            (run_id, input, items)
        }

        let matcher = SkimMatcherV2::default();
        loop {
            let (run_id, input, items) = receive_batch(&receiver);
            let filtered_items =
                Self::filter_items(&run_id, &input, items, &matcher);
            event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::FilterPaletteItems(run_id, input, filtered_items),
                Target::Widget(widget_id),
            );
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

#[derive(Clone)]
pub struct PaletteState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub widget_id: WidgetId,
    pub scroll_widget_id: WidgetId,
    scroll_offset: f64,
    input: String,
    cursor: usize,
    items: Vec<PaletteItem>,
    filtered_items: Vec<PaletteItem>,
    index: usize,
    palette_type: PaletteType,
    run_id: String,
    rev: u64,
    sender: Sender<u64>,
}

impl PaletteState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> PaletteState {
        let widget_id = WidgetId::next();
        let (sender, receiver) = unbounded();
        let state = PaletteState {
            window_id,
            tab_id,
            widget_id,
            scroll_widget_id: WidgetId::next(),
            items: Vec::new(),
            filtered_items: Vec::new(),
            input: "".to_string(),
            scroll_offset: 0.0,
            cursor: 0,
            index: 0,
            rev: 0,
            sender,
            palette_type: PaletteType::File,
            run_id: Uuid::new_v4().to_string(),
        };
        thread::spawn(move || {
            start_filter_process(window_id, tab_id, widget_id, receiver);
        });
        state
    }

    pub fn run(&mut self, palette_type: Option<PaletteType>) {
        self.palette_type = palette_type.unwrap_or(PaletteType::File);
        self.run_id = Uuid::new_v4().to_string();
        self.rev += 1;
        match &self.palette_type {
            &PaletteType::Line => {
                self.input = "/".to_string();
                self.cursor = 1;
                self.items = self.get_lines().unwrap_or(Vec::new());
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .save_selection();
            }
            &PaletteType::DocumentSymbol => {
                self.input = "@".to_string();
                self.cursor = 1;
                self.get_document_symbols();
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .save_selection();
            }
            _ => self.get_files(),
        }
    }

    pub fn run_references(&mut self, locations: Vec<Location>) {
        self.palette_type = PaletteType::Reference;
        self.run_id = Uuid::new_v4().to_string();
        self.rev += 1;
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        let items = locations
            .iter()
            .map(|location| PaletteItem {
                window_id,
                tab_id,
                kind: PaletteType::Reference,
                text: location.uri.as_str().to_string(),
                hint: None,
                position: None,
                location: Some(location.clone()),
                path: None,
                score: 0.0,
                index: 0,
                match_mask: BitVec::new(),
                icon: PaletteIcon::None,
                workspace: None,
                command: None,
            })
            .collect::<Vec<PaletteItem>>();
        self.items = items;
        LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .editor_split
            .lock()
            .save_selection();
        LAPCE_APP_STATE
            .submit_ui_command(LapceUICommand::FilterItems, self.widget_id);
    }

    pub fn cancel(&mut self, ctx: &mut EventCtx, ui_state: &mut LapceUIState) {
        match &self.palette_type {
            &PaletteType::Line => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .restore_selection(ctx, ui_state);
            }
            &PaletteType::DocumentSymbol => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .restore_selection(ctx, ui_state);
            }
            &PaletteType::Reference => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .restore_selection(ctx, ui_state);
            }
            _ => (),
        }
        self.reset(ctx);
    }

    pub fn reset(&mut self, ctx: &mut EventCtx) {
        self.input = "".to_string();
        self.cursor = 0;
        self.index = 0;
        self.items = Vec::new();
        self.palette_type = PaletteType::File;
    }

    // fn ensure_visible(&self, ctx: &mut EventCtx, env: &Env) {
    //     let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
    //     let rect = Rect::ZERO
    //         .with_origin(Point::new(0.0, self.index as f64 * line_height))
    //         .with_size(Size::new(10.0, line_height));
    //     let margin = (0.0, 0.0);
    //     ctx.submit_command(Command::new(
    //         LAPCE_UI_COMMAND,
    //         LapceUICommand::EnsureVisible((rect, margin, None)),
    //         Target::Widget(self.scroll_widget_id),
    //     ));
    // }

    pub fn key_event(&mut self, key: &KeyEvent) {}

    fn get_palette_type(&self) -> PaletteType {
        if self.palette_type == PaletteType::Reference {
            return PaletteType::Reference;
        }
        if self.input == "" {
            return PaletteType::File;
        }
        match self.input {
            _ if self.input.starts_with("/") => PaletteType::Line,
            _ if self.input.starts_with("@") => PaletteType::DocumentSymbol,
            _ if self.input.starts_with(">") => PaletteType::Workspace,
            _ if self.input.starts_with(":") => PaletteType::Command,
            _ => PaletteType::File,
        }
    }

    pub fn insert(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        content: &str,
        env: &Env,
    ) {
        self.input.insert_str(self.cursor, content);
        self.cursor += content.len();
        self.update_palette();
    }

    fn update_palette(&mut self) {
        self.index = 0;
        self.rev += 1;
        let palette_type = self.get_palette_type();
        if self.palette_type != palette_type {
            self.palette_type = palette_type;
            self.run_id = Uuid::new_v4().to_string();
            match &self.palette_type {
                &PaletteType::File => self.get_files(),
                &PaletteType::Line => {
                    self.items = self.get_lines().unwrap_or(Vec::new())
                }
                &PaletteType::DocumentSymbol => {
                    self.get_document_symbols();
                }
                &PaletteType::Workspace => self.items = self.get_workspaces(),
                &PaletteType::Command => self.items = self.get_commands(),
                _ => (),
            }
            LAPCE_APP_STATE
                .submit_ui_command(LapceUICommand::RequestPaint, self.widget_id);
            return;
        } else {
            self.sender.send(self.rev);
        }
        LAPCE_APP_STATE
            .submit_ui_command(LapceUICommand::RequestPaint, self.widget_id);
    }

    pub fn move_cursor(&mut self, ctx: &mut EventCtx, n: i64) {
        let cursor = (self.cursor as i64 + n)
            .max(0i64)
            .min(self.input.len() as i64) as usize;
        if self.cursor == cursor {
            return;
        }
        self.cursor = cursor;
        self.request_paint(ctx);
    }

    pub fn delete_backward(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) {
        if self.cursor == 0 {
            return;
        }

        self.input.remove(self.cursor - 1);
        self.cursor = self.cursor - 1;
        self.update_palette();
    }

    pub fn delete_to_beginning_of_line(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) {
        if self.cursor == 0 {
            return;
        }

        let start = match &self.palette_type {
            &PaletteType::File => 0,
            &PaletteType::Reference => 0,
            &PaletteType::Line => 1,
            &PaletteType::DocumentSymbol => 1,
            &PaletteType::Workspace => 1,
            &PaletteType::Command => 1,
        };

        if self.cursor == start {
            self.input = "".to_string();
            self.cursor = 0;
        } else {
            self.input.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
        self.update_palette();
    }

    pub fn get_input(&self) -> &str {
        match &self.palette_type {
            PaletteType::File => &self.input,
            PaletteType::Reference => &self.input,
            PaletteType::Line => &self.input[1..],
            PaletteType::DocumentSymbol => &self.input[1..],
            PaletteType::Workspace => &self.input[1..],
            PaletteType::Command => &self.input[1..],
        }
    }

    fn get_commands(&self) -> Vec<PaletteItem> {
        let commands = vec![("Open Folder", LapceCommand::OpenFolder)];
        commands
            .iter()
            .enumerate()
            .map(|(i, c)| PaletteItem {
                window_id: self.window_id,
                tab_id: self.tab_id,
                icon: PaletteIcon::None,
                kind: PaletteType::Command,
                text: c.0.to_string(),
                hint: None,
                score: 0.0,
                index: i,
                match_mask: BitVec::new(),
                workspace: None,
                position: None,
                location: None,
                path: None,
                command: Some(c.1.clone()),
            })
            .collect()
    }

    fn get_workspaces(&self) -> Vec<PaletteItem> {
        let workspaces = vec![
            LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: PathBuf::from("/Users/Lulu/lapce"),
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
        workspaces
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let text = w.path.to_str().unwrap();
                let text = match &w.kind {
                    LapceWorkspaceType::Local => text.to_string(),
                    LapceWorkspaceType::RemoteSSH(user, host) => {
                        format!("[{}@{}] {}", user, host, text)
                    }
                };
                PaletteItem {
                    window_id: self.window_id,
                    tab_id: self.tab_id,
                    icon: PaletteIcon::None,
                    kind: PaletteType::Workspace,
                    text,
                    hint: None,
                    location: None,
                    score: 0.0,
                    index: i,
                    match_mask: BitVec::new(),
                    workspace: Some(w.clone()),
                    position: None,
                    path: None,
                    command: None,
                }
            })
            .collect()
    }

    fn get_document_symbols(&self) -> Option<()> {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        let editor = editor_split.editors.get(&editor_split.active)?;
        let buffer_id = editor.buffer_id?;
        let buffer = editor_split.buffers.get(&buffer_id)?;
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        let run_id = self.run_id.clone();
        let widget_id = self.widget_id;
        state.proxy.lock().as_ref().unwrap().get_document_symbols(
            buffer_id,
            Box::new(move |result| {
                if let Ok(res) = result {
                    let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
                    if *state.focus.lock() != LapceFocus::Palette {
                        return;
                    }
                    let mut palette = state.palette.lock();
                    if palette.run_id != run_id {
                        return;
                    }
                    let resp: Result<DocumentSymbolResponse, serde_json::Error> =
                        serde_json::from_value(res);
                    if let Ok(resp) = resp {
                        let items: Vec<PaletteItem> = match resp {
                            DocumentSymbolResponse::Flat(symbols) => symbols
                                .iter()
                                .enumerate()
                                .map(|(i, s)| PaletteItem {
                                    window_id,
                                    tab_id,
                                    kind: PaletteType::DocumentSymbol,
                                    text: s.name.clone(),
                                    hint: s.container_name.clone(),
                                    position: Some(s.location.range.start),
                                    path: None,
                                    location: None,
                                    score: 0.0,
                                    index: i,
                                    match_mask: BitVec::new(),
                                    icon: PaletteIcon::Symbol(s.kind),
                                    workspace: None,
                                    command: None,
                                })
                                .collect(),
                            DocumentSymbolResponse::Nested(symbols) => symbols
                                .iter()
                                .enumerate()
                                .map(|(i, s)| PaletteItem {
                                    window_id,
                                    tab_id,
                                    kind: PaletteType::DocumentSymbol,
                                    text: s.name.clone(),
                                    hint: None,
                                    path: None,
                                    location: None,
                                    position: Some(s.range.start),
                                    score: 0.0,
                                    index: i,
                                    match_mask: BitVec::new(),
                                    icon: PaletteIcon::Symbol(s.kind),
                                    workspace: None,
                                    command: None,
                                })
                                .collect(),
                        };
                        palette.items = items;
                        if palette.get_input() != "" {
                            palette.update_palette();
                        }
                        LAPCE_APP_STATE.submit_ui_command(
                            LapceUICommand::RequestPaint,
                            widget_id,
                        );
                    }
                }
            }),
        );
        None
        // let resp = state.lsp.lock().get_document_symbols(buffer)?;
        // Some(match resp {
        //     DocumentSymbolResponse::Flat(symbols) => symbols
        //         .iter()
        //         .enumerate()
        //         .map(|(i, s)| PaletteItem {
        //             window_id: self.window_id,
        //             tab_id: self.tab_id,
        //             kind: PaletteType::DocumentSymbol,
        //             text: s.name.clone(),
        //             hint: s.container_name.clone(),
        //             position: Some(s.location.range.start),
        //             path: None,
        //             score: 0.0,
        //             index: i,
        //             match_mask: BitVec::new(),
        //             icon: PaletteIcon::Symbol(s.kind),
        //             workspace: None,
        //             command: None,
        //         })
        //         .collect(),
        //     DocumentSymbolResponse::Nested(symbols) => symbols
        //         .iter()
        //         .enumerate()
        //         .map(|(i, s)| PaletteItem {
        //             window_id: self.window_id,
        //             tab_id: self.tab_id,
        //             kind: PaletteType::DocumentSymbol,
        //             text: s.name.clone(),
        //             hint: None,
        //             path: None,
        //             position: Some(s.range.start),
        //             score: 0.0,
        //             index: i,
        //             match_mask: BitVec::new(),
        //             icon: PaletteIcon::Symbol(s.kind),
        //             workspace: None,
        //             command: None,
        //         })
        //         .collect(),
        // })
    }

    fn get_lines(&self) -> Option<Vec<PaletteItem>> {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        let editor = editor_split.editors.get(&editor_split.active)?;
        let buffer_id = editor.buffer_id?;
        let buffer = editor_split.buffers.get(&buffer_id)?;
        Some(
            buffer
                .rope
                .lines(0..buffer.len())
                .enumerate()
                .map(|(i, l)| PaletteItem {
                    window_id: self.window_id,
                    tab_id: self.tab_id,
                    kind: PaletteType::Line,
                    text: format!("{}: {}", i, l.to_string()),
                    hint: None,
                    position: None,
                    path: None,
                    location: None,
                    score: 0.0,
                    index: i,
                    match_mask: BitVec::new(),
                    icon: PaletteIcon::None,
                    workspace: None,
                    command: None,
                })
                .collect(),
        )
    }

    fn get_files(&self) {
        let workspace_path = LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .workspace
            .lock()
            .path
            .clone();
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        let run_id = self.run_id.clone();
        let widget_id = self.widget_id;
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        state
            .proxy
            .lock()
            .as_ref()
            .unwrap()
            .get_files(Box::new(move |result| {
                if let Ok(res) = result {
                    let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
                    if *state.focus.lock() != LapceFocus::Palette {
                        return;
                    }
                    let mut palette = state.palette.lock();
                    if palette.run_id != run_id {
                        return;
                    }

                    let resp: Result<Vec<PathBuf>, serde_json::Error> =
                        serde_json::from_value(res);
                    if let Ok(resp) = resp {
                        let items: Vec<PaletteItem> = resp
                            .iter()
                            .enumerate()
                            .map(|(index, path)| {
                                let text = path
                                    .file_name()
                                    .unwrap()
                                    .to_str()
                                    .unwrap()
                                    .to_string();
                                let folder = path.parent().unwrap();
                                let folder = if let Ok(folder) =
                                    folder.strip_prefix(&workspace_path)
                                {
                                    folder
                                } else {
                                    folder
                                };
                                let icon = if let Some(exten) = path.extension() {
                                    match exten.to_str().unwrap() {
                                        "rs" => {
                                            PaletteIcon::File("rust".to_string())
                                        }
                                        "md" => {
                                            PaletteIcon::File("markdown".to_string())
                                        }
                                        "cc" => PaletteIcon::File("cpp".to_string()),
                                        s => PaletteIcon::File(s.to_string()),
                                    }
                                } else {
                                    PaletteIcon::None
                                };
                                let hint = folder.to_str().unwrap().to_string();
                                PaletteItem {
                                    window_id,
                                    tab_id,
                                    kind: PaletteType::File,
                                    text,
                                    hint: Some(hint),
                                    position: None,
                                    path: Some(path.clone()),
                                    location: None,
                                    score: 0.0,
                                    index,
                                    match_mask: BitVec::new(),
                                    icon,
                                    workspace: None,
                                    command: None,
                                }
                            })
                            .collect();

                        palette.items = items;
                        if palette.get_input() != "" {
                            palette.update_palette();
                        } else {
                            LAPCE_APP_STATE.submit_ui_command(
                                LapceUICommand::RequestPaint,
                                widget_id,
                            );
                        }
                    }
                }
                println!("get files result");
            }));
    }

    fn get_local_files(&self) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        let mut dirs = Vec::new();
        let mut index = 0;
        let workspace_path = LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .workspace
            .lock()
            .path
            .clone();
        dirs.push(workspace_path.clone());
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_name().to_str().unwrap().starts_with(".") {
                    continue;
                }
                if path.is_dir() {
                    if !path
                        .as_path()
                        .to_str()
                        .unwrap()
                        .to_string()
                        .ends_with("target")
                    {
                        dirs.push(path);
                    }
                } else {
                    let text =
                        path.file_name().unwrap().to_str().unwrap().to_string();
                    let folder = path.parent().unwrap();
                    let folder =
                        if let Ok(folder) = folder.strip_prefix(&workspace_path) {
                            folder
                        } else {
                            folder
                        };
                    let icon = if let Some(exten) = path.extension() {
                        match exten.to_str().unwrap() {
                            "rs" => PaletteIcon::File("rust".to_string()),
                            "md" => PaletteIcon::File("markdown".to_string()),
                            "cc" => PaletteIcon::File("cpp".to_string()),
                            s => PaletteIcon::File(s.to_string()),
                        }
                    } else {
                        PaletteIcon::None
                    };
                    // let file = path.as_path().to_str().unwrap().to_string();
                    let hint = folder.to_str().unwrap().to_string();
                    items.push(PaletteItem {
                        window_id: self.window_id,
                        tab_id: self.tab_id,
                        kind: PaletteType::File,
                        text,
                        hint: Some(hint),
                        position: None,
                        path: Some(path),
                        location: None,
                        score: 0.0,
                        index,
                        match_mask: BitVec::new(),
                        icon,
                        workspace: None,
                        command: None,
                    });
                    index += 1;
                }
            }
        }
        items
    }

    pub fn current_items(&self) -> &Vec<PaletteItem> {
        if self.get_input() == "" {
            &self.items
        } else {
            &self.filtered_items
        }
    }

    pub fn get_item(&self) -> Option<&PaletteItem> {
        let items = self.current_items();
        if items.is_empty() {
            return None;
        }
        Some(&items[self.index])
    }

    pub fn preview(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) {
        let item = self.get_item();
        if item.is_none() {
            return;
        }
        let item = item.unwrap();
        match &item.kind {
            &PaletteType::Line => {
                item.select(ctx, ui_state, env);
            }
            &PaletteType::Reference => {
                item.select(ctx, ui_state, env);
            }
            &PaletteType::DocumentSymbol => {
                item.select(ctx, ui_state, env);
            }
            _ => (),
        }
    }

    pub fn select(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) {
        let items = self.current_items();
        if items.is_empty() {
            return;
        }
        items[self.index].select(ctx, ui_state, env);
        self.reset(ctx);
    }

    pub fn request_paint(&self, ctx: &mut EventCtx) {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::RequestPaint,
            Target::Widget(self.widget_id),
        ))
    }

    pub fn change_index(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        n: i64,
        env: &Env,
    ) {
        let items = self.current_items();

        self.index = if self.index as i64 + n < 0 {
            (items.len() + self.index) as i64 + n
        } else if self.index as i64 + n > items.len() as i64 - 1 {
            self.index as i64 + n - items.len() as i64
        } else {
            self.index as i64 + n
        } as usize;

        // self.ensure_visible(ctx, env);
        self.request_paint(ctx);
        self.preview(ctx, ui_state, env);
    }
}

pub fn start_filter_process(
    window_id: WindowId,
    tab_id: WidgetId,
    widget_id: WidgetId,
    receiver: Receiver<u64>,
) -> Result<()> {
    loop {
        let rev = receiver.recv()?;
        let (input, mut items) = {
            let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
            let palette = state.palette.lock();
            if palette.rev != rev {
                continue;
            }
            (palette.get_input().to_string(), palette.items.clone())
        };

        let items = filter_items(&input, items);

        let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
        let mut palette = state.palette.lock();
        if palette.rev != rev {
            continue;
        }
        palette.filtered_items = items;
        LAPCE_APP_STATE.submit_ui_command(LapceUICommand::FilterItems, widget_id);
    }
}

pub struct NewPalette {
    widget_id: WidgetId,
    container: WidgetPod<PaletteViewData, Box<dyn Widget<PaletteViewData>>>,
}

impl NewPalette {
    pub fn new(data: &PaletteData) -> Self {
        let padding = 6.0;
        let input = NewPaletteInput::new()
            .padding((padding, padding, padding, padding * 2.0))
            .background(LapceTheme::EDITOR_BACKGROUND)
            .padding((padding, padding, padding, padding));
        let content = LapceScrollNew::new(NewPaletteContent::new()).vertical();
        let container = PaletteContainer::new();
        Self {
            widget_id: data.widget_id,
            container: WidgetPod::new(container).boxed(),
        }
    }
}

impl Widget<PaletteViewData> for NewPalette {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut PaletteViewData,
        env: &Env,
    ) {
        match event {
            Event::WindowConnected => {
                let receiver =
                    Arc::make_mut(&mut data.palette).receiver.take().unwrap();
                let event_sink = ctx.get_external_handle();
                let widget_id = self.widget_id;
                thread::spawn(move || {
                    PaletteViewData::update_process(receiver, widget_id, event_sink);
                });
            }
            Event::KeyDown(key_event) => {
                let mut keypress = data.keypress.clone();
                let mut_keypress = Arc::make_mut(&mut keypress);
                mut_keypress.key_down(ctx, key_event, data, env);
                data.keypress = keypress;
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::RunPalette => {
                        ctx.request_focus();
                    }
                    LapceUICommand::CancelPalette => {
                        ctx.resign_focus();
                    }
                    LapceUICommand::UpdatePaletteItems(run_id, items) => {
                        let palette = Arc::make_mut(&mut data.palette);
                        if &palette.run_id == run_id {
                            palette.items = items.to_owned();
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
        data: &PaletteViewData,
        env: &Env,
    ) {
        self.container.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &PaletteViewData,
        data: &PaletteViewData,
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
        data: &PaletteViewData,
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, env: &Env) {
        if data.palette.status == PaletteStatus::Inactive {
            return;
        }

        self.container.paint(ctx, data, env);
    }
}

pub struct PaletteContainer {
    content_size: Size,
    input: WidgetPod<PaletteViewData, Box<dyn Widget<PaletteViewData>>>,
    content: WidgetPod<
        PaletteViewData,
        LapceScrollNew<PaletteViewData, NewPaletteContent>,
    >,
}

impl PaletteContainer {
    pub fn new() -> Self {
        let padding = 6.0;
        let input = NewPaletteInput::new()
            .padding((padding, padding, padding, padding * 2.0))
            .background(LapceTheme::EDITOR_BACKGROUND)
            .padding((padding, padding, padding, padding));
        let content = LapceScrollNew::new(NewPaletteContent::new()).vertical();
        Self {
            content_size: Size::ZERO,
            input: WidgetPod::new(input).boxed(),
            content: WidgetPod::new(content),
        }
    }

    fn ensure_item_visble(
        &mut self,
        ctx: &mut UpdateCtx,
        data: &PaletteViewData,
        env: &Env,
    ) {
        let width = ctx.size().width;
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rect = Size::new(width, line_height)
            .to_rect()
            .with_origin(Point::new(0.0, data.palette.index as f64 * line_height));
        self.content.widget_mut().scroll_to_visible(
            rect,
            |d| ctx.request_timer(d),
            env,
        );
    }
}

impl Widget<PaletteViewData> for PaletteContainer {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut PaletteViewData,
        env: &Env,
    ) {
        self.input.event(ctx, event, data, env);
        self.content.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &PaletteViewData,
        env: &Env,
    ) {
        self.input.lifecycle(ctx, event, data, env);
        self.content.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &PaletteViewData,
        data: &PaletteViewData,
        env: &Env,
    ) {
        if old_data.palette.input != data.palette.input
            || old_data.palette.index != data.palette.index
        {
            self.ensure_item_visble(ctx, data, env);
        }
        self.input.update(ctx, data, env);
        self.content.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteViewData,
        env: &Env,
    ) -> Size {
        let width = 600.0;

        let bc = BoxConstraints::tight(Size::new(width, bc.max().height));
        let input_size = self.input.layout(ctx, &bc, data, env);
        self.input.set_origin(ctx, data, env, Point::ZERO);

        let max_items = 15;
        let height = max_items.min(data.palette.len());
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let height = line_height * height as f64;
        let bc = BoxConstraints::tight(Size::new(width, height));
        let content_size = self.content.layout(ctx, &bc, data, env);
        self.content
            .set_origin(ctx, data, env, Point::new(0.0, input_size.height));

        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));
        let self_size =
            Size::new(width, input_size.height + content_size.height + 6.0);
        self.content_size = self_size;
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, env: &Env) {
        let blur_color = Color::grey8(180);
        let shadow_width = 5.0;
        let rect = self.content_size.to_rect();
        ctx.blurred_rect(rect, shadow_width, &blur_color);
        ctx.fill(rect, &env.get(LapceTheme::EDITOR_SELECTION_COLOR));

        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);
    }
}

pub struct Palette {
    window_id: WindowId,
    tab_id: WidgetId,
    content: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    input: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    input_height: f64,
    max_items: usize,
    rect: Rect,
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
        let mut text_layout = TextLayout::<String>::from_text(&text);
        text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        let line = text_layout.cursor_line_for_text_position(cursor);
        ctx.stroke(line, &env.get(LapceTheme::EDITOR_FOREGROUND), 1.0);
        text_layout.draw(ctx, Point::new(0.0, 0.0));
    }
}

pub struct NewPaletteContent {}

impl NewPaletteContent {
    pub fn new() -> Self {
        Self {}
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
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let height = line_height * data.palette.len() as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteViewData, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        let size = ctx.size();

        let items = data.palette.current_items();

        for rect in rects {
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
                        &env.get(LapceTheme::EDITOR_BACKGROUND),
                    );
                }

                let item = &items[line];
                item.content.paint(ctx, line, &item.indices, env);
            }
        }
    }
}

impl Palette {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        scroll_id: WidgetId,
    ) -> Palette {
        let padding = 6.0;
        let max_items = 15;
        let palette_input = PaletteInput::new(window_id, tab_id)
            .padding((padding, padding, padding, padding * 2.0))
            .background(LapceTheme::EDITOR_BACKGROUND)
            .padding((padding, padding, padding, padding));
        let palette_content = PaletteContent::new(window_id, tab_id, max_items)
            .with_id(scroll_id)
            .padding((padding, 0.0, padding, padding));
        let palette = Palette {
            window_id,
            tab_id,
            input: WidgetPod::new(palette_input).boxed(),
            content: WidgetPod::new(palette_content).boxed(),
            rect: Rect::ZERO
                .with_origin(Point::new(50.0, 50.0))
                .with_size(Size::new(100.0, 50.0)),
            input_height: 0.0,
            max_items,
        };
        palette
    }

    fn cancel(&self) {
        // LAPCE_STATE.palette.lock().unwrap().input = "".to_string();
        // LAPCE_STATE.palette.lock().unwrap().cursor = 0;
        // LAPCE_STATE.palette.lock().unwrap().index = 0;
        // self.content.set_scroll(0.0, 0.0);
        // self.hide();
    }
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

impl Widget<LapceUIState> for Palette {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => self.content.event(ctx, event, data, env),
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            ctx.request_paint();
                        }
                        LapceUICommand::FilterItems => {
                            let state = LAPCE_APP_STATE
                                .get_tab_state(&self.window_id, &self.tab_id);
                            let mut palette = state.palette.lock();
                            palette.preview(ctx, data, env);
                            ctx.request_paint();
                        }
                        _ => (),
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
        self.content.lifecycle(ctx, event, data, env);
        self.input.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let input_size = self.input.layout(ctx, bc, data, env);
        self.input_height = input_size.height;
        self.input
            .set_layout_rect(ctx, data, env, Rect::ZERO.with_size(input_size));
        let content_bc = BoxConstraints::new(
            Size::ZERO,
            Size::new(bc.max().width, bc.max().height - input_size.height),
        );
        let content_size = self.content.layout(ctx, &content_bc, data, env);
        self.content
            .set_origin(ctx, data, env, Point::new(0.0, input_size.height));
        ctx.set_paint_insets((10.0, 10.0, 10.0, 10.0));
        Size::new(bc.max().width, self.input_height + content_size.height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        {
            if *state.focus.lock() != LapceFocus::Palette {
                return;
            }
        }

        let shadow_width = 5.0;
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let items_len = {
            let palette = state.palette.lock();
            palette.current_items().len()
        };
        let items_len = if items_len > self.max_items {
            self.max_items
        } else {
            items_len
        };
        let height = line_height * items_len as f64
            + if items_len > 0 { 6.0 } else { 0.0 }
            + self.input_height;

        let size = Size::new(ctx.size().width, height);
        let rect = size.to_rect();
        let blur_color = Color::grey8(100);
        ctx.blurred_rect(rect, shadow_width, &blur_color);
        ctx.fill(rect, &env.get(LapceTheme::EDITOR_SELECTION_COLOR));

        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);
    }
}

impl Widget<LapceUIState> for PaletteContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let height = line_height * self.max_items as f64;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let mut palette = state.palette.lock();
        let height = line_height * self.max_items as f64;
        for rect in rects {
            let items = palette.current_items();
            let items_height = items.len() as f64 * line_height;
            let current_line_offset = palette.index as f64 * line_height;
            let scroll_offset = if palette.scroll_offset
                < current_line_offset + line_height - height
            {
                (current_line_offset + line_height - height)
                    .min(items_height - height)
            } else if palette.scroll_offset > current_line_offset {
                current_line_offset
            } else {
                palette.scroll_offset
            };

            let start = (scroll_offset / line_height).floor() as usize;

            for line in start..start + self.max_items {
                if line >= items.len() {
                    break;
                }
                let item = &items[line];
                if palette.index == line {
                    if let Some(background) = LAPCE_APP_STATE.theme.get("background")
                    {
                        ctx.fill(
                            Rect::ZERO
                                .with_origin(Point::new(
                                    rect.x0,
                                    line as f64 * line_height - scroll_offset,
                                ))
                                .with_size(Size::new(rect.width(), line_height)),
                            background,
                        )
                    }
                }
                if let Some((svg_data, svg_tree)) = match &item.icon {
                    PaletteIcon::File(exten) => file_svg(&exten),
                    PaletteIcon::Symbol(symbol) => symbol_svg(&symbol),
                    _ => None,
                } {
                    let svg_size = svg_tree_size(&svg_tree);
                    let scale = 13.0 / svg_size.height;
                    let affine = Affine::new([
                        scale,
                        0.0,
                        0.0,
                        scale,
                        1.0,
                        line as f64 * line_height + 5.0 - scroll_offset,
                    ]);
                    svg_data.to_piet(affine, ctx);
                }
                let mut text_layout = ctx
                    .text()
                    .new_text_layout(item.text.clone())
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
                for (i, _) in item.text.chars().enumerate() {
                    if item.match_mask.get(i).unwrap_or(false) {
                        text_layout = text_layout.range_attribute(
                            i..i + 1,
                            TextAttribute::TextColor(Color::rgb8(0, 0, 0)),
                        );
                        text_layout = text_layout.range_attribute(
                            i..i + 1,
                            TextAttribute::Weight(FontWeight::BOLD),
                        );
                    }
                }
                let text_layout = text_layout.build().unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        20.0,
                        line as f64 * line_height + 4.0 - scroll_offset,
                    ),
                );

                let text_x =
                    text_layout.hit_test_text_position(item.text.len()).point.x;
                let text_len = item.text.len();
                if let Some(hint) = item.hint.as_ref() {
                    let mut text_layout = ctx
                        .text()
                        .new_text_layout(hint.clone())
                        .font(FontFamily::SYSTEM_UI, 13.0)
                        .text_color(
                            env.get(LapceTheme::EDITOR_FOREGROUND).with_alpha(0.6),
                        );
                    for (i, _) in item.text.chars().enumerate() {
                        if item.match_mask.get(i + 1 + text_len).unwrap_or(false) {
                            text_layout = text_layout.range_attribute(
                                i..i + 1,
                                TextAttribute::TextColor(Color::rgb8(0, 0, 0)),
                            );
                            text_layout = text_layout.range_attribute(
                                i..i + 1,
                                TextAttribute::Weight(FontWeight::BOLD),
                            );
                        }
                    }
                    let text_layout = text_layout.build().unwrap();
                    ctx.draw_text(
                        &text_layout,
                        Point::new(
                            20.0 + text_x + 4.0,
                            line as f64 * line_height + 5.0 - scroll_offset,
                        ),
                    );
                }
            }
            if height < items_height {
                let scroll_bar_height = height * (height / items_height);
                let scroll_y = height * (scroll_offset / items_height);
                let scroll_bar_width = 10.0;
                ctx.render_ctx.fill(
                    Rect::ZERO
                        .with_origin(Point::new(
                            ctx.size().width - scroll_bar_width,
                            scroll_y,
                        ))
                        .with_size(Size::new(scroll_bar_width, scroll_bar_height)),
                    &env.get(theme::SCROLLBAR_COLOR),
                );
            }
            palette.scroll_offset = scroll_offset;
        }
    }
}

fn get_svg(name: &str) -> Option<(SvgData, usvg::Tree)> {
    let content = ICONS_DIR.get_file(name)?.contents_utf8()?;

    let opt = usvg::Options {
        keep_named_groups: false,
        ..usvg::Options::default()
    };
    let usvg_tree = usvg::Tree::from_str(&content, &opt).ok()?;

    Some((SvgData::from_str(&content).ok()?, usvg_tree))
}

pub fn file_svg(exten: &str) -> Option<(SvgData, usvg::Tree)> {
    get_svg(&format!("file_type_{}.svg", exten))
}

fn symbol_svg(kind: &SymbolKind) -> Option<(SvgData, usvg::Tree)> {
    let kind_str = match kind {
        SymbolKind::Array => "array",
        SymbolKind::Boolean => "boolean",
        SymbolKind::Class => "class",
        SymbolKind::Constant => "constant",
        SymbolKind::EnumMember => "enum-member",
        SymbolKind::Enum => "enum",
        SymbolKind::Event => "event",
        SymbolKind::Field => "field",
        SymbolKind::File => "file",
        SymbolKind::Interface => "interface",
        SymbolKind::Key => "key",
        SymbolKind::Function => "method",
        SymbolKind::Method => "method",
        SymbolKind::Object => "namespace",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Number => "numeric",
        SymbolKind::Operator => "operator",
        SymbolKind::TypeParameter => "parameter",
        SymbolKind::Property => "property",
        SymbolKind::String => "string",
        SymbolKind::Struct => "structure",
        SymbolKind::Variable => "variable",
        _ => return None,
    };

    get_svg(&format!("symbol-{}.svg", kind_str))
}

impl Widget<LapceUIState> for PaletteInput {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &Env,
    ) {
        // if old_data.palette.input != data.palette.input
        //     || old_data.palette.cursor != data.palette.cursor
        // {
        //     ctx.request_paint();
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        Size::new(bc.max().width, 13.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let palette = state.palette.lock();
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let text = palette.input.clone();
        let cursor = palette.cursor;
        let mut text_layout = TextLayout::<String>::from_text(&text);
        text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        let line = text_layout.cursor_line_for_text_position(cursor);
        ctx.stroke(line, &env.get(LapceTheme::EDITOR_FOREGROUND), 1.0);
        text_layout.draw(ctx, Point::new(0.0, 0.0));
    }
}

impl PaletteItem {
    fn get_text(&self) -> String {
        if let Some(hint) = &self.hint {
            format!("{} {}", self.text, hint)
        } else {
            self.text.clone()
        }
    }

    pub fn select(
        &self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) {
        match &self.kind {
            &PaletteType::File => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                let mut editor_split = state.editor_split.lock();
                editor_split.save_jump_location();
                editor_split.open_file(
                    ctx,
                    ui_state,
                    self.path.as_ref().unwrap().to_str().unwrap(),
                );
            }
            &PaletteType::Line => {
                let line = self
                    .text
                    .splitn(2, ":")
                    .next()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .jump_to_line(ctx, ui_state, line, env);
            }
            &PaletteType::DocumentSymbol => {
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .jump_to_postion(
                        ctx,
                        ui_state,
                        self.position.as_ref().unwrap(),
                        0.75,
                        env,
                    );
            }
            &PaletteType::Workspace => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                *state.workspace.lock() = self.workspace.clone().unwrap();
                state.start_proxy();
                ctx.request_paint();
            }
            &PaletteType::Command => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                state.editor_split.lock().run_command(
                    ctx,
                    ui_state,
                    None,
                    self.command.clone().unwrap(),
                    env,
                );
            }
            &PaletteType::Reference => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                let mut editor_split = state.editor_split.lock();
                editor_split.go_to_location(
                    ctx,
                    ui_state,
                    self.location.as_ref().unwrap(),
                    env,
                );
                editor_split.window_portion(ctx, 0.75, env);
            }
        }
    }
}

fn filter_items(input: &str, items: Vec<PaletteItem>) -> Vec<PaletteItem> {
    let mut items: Vec<PaletteItem> = items
        .iter()
        .filter_map(|i| {
            let text = i.get_text();
            if has_match(&input, &text) {
                let result = locate(&input, &text);
                let mut item = i.clone();
                item.score = result.score;
                item.match_mask = result.match_mask;
                Some(item)
            } else {
                None
            }
        })
        .collect();
    items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Less));
    items
}

pub fn svg_tree_size(svg_tree: &usvg::Tree) -> Size {
    match *svg_tree.root().borrow() {
        usvg::NodeKind::Svg(svg) => Size::new(svg.size.width(), svg.size.height()),
        _ => Size::ZERO,
    }
}
