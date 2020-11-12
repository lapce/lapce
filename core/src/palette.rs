use bit_vec::BitVec;
use druid::{
    kurbo::{Line, Rect},
    piet::TextAttribute,
    widget::Container,
    widget::IdentityWrapper,
    widget::Svg,
    widget::SvgData,
    Affine, Command, FontFamily, FontWeight, KeyEvent, Target, Vec2, WidgetId,
    WindowId,
};
use druid::{
    piet::{Text, TextLayout as PietTextLayout, TextLayoutBuilder},
    theme, BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};
use druid::{
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll},
    TextLayout,
};
use fzyr::{has_match, locate, Score};
use lsp_types::{DocumentSymbolResponse, Location, Position, SymbolKind};
use serde_json::{self, json, Value};
use std::cmp::Ordering;
use std::fs::{self, DirEntry};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::{
    command::LapceCommand, command::LapceUICommand, command::LAPCE_COMMAND,
    command::LAPCE_UI_COMMAND, editor::EditorSplitState, explorer::ICONS_DIR,
    scroll::LapceScroll, ssh::SshSession, state::LapceFocus, state::LapceUIState,
    state::LapceWorkspace, state::LapceWorkspaceType, state::LAPCE_APP_STATE,
    theme::LapceTheme,
};

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteType {
    File,
    Line,
    DocumentSymbol,
    Workspace,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteIcon {
    File(String),
    Symbol(SymbolKind),
    None,
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
    path: Option<PathBuf>,
    workspace: Option<LapceWorkspace>,
}

#[derive(Clone)]
pub struct PaletteState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub widget_id: WidgetId,
    pub scroll_widget_id: WidgetId,
    input: String,
    cursor: usize,
    items: Vec<PaletteItem>,
    index: usize,
    palette_type: PaletteType,
}

impl PaletteState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> PaletteState {
        PaletteState {
            window_id,
            tab_id,
            widget_id: WidgetId::next(),
            scroll_widget_id: WidgetId::next(),
            items: Vec::new(),
            input: "".to_string(),
            cursor: 0,
            index: 0,
            palette_type: PaletteType::File,
        }
    }
}

impl PaletteState {
    pub fn run(&mut self, palette_type: Option<PaletteType>) {
        self.palette_type = palette_type.unwrap_or(PaletteType::File);
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
                self.items = self.get_document_symbols().unwrap_or(Vec::new());
                LAPCE_APP_STATE
                    .get_tab_state(&self.window_id, &self.tab_id)
                    .editor_split
                    .lock()
                    .save_selection();
            }
            _ => self.items = self.get_files(),
        }
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

    fn ensure_visible(&self, ctx: &mut EventCtx, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rect = Rect::ZERO
            .with_origin(Point::new(0.0, self.index as f64 * line_height))
            .with_size(Size::new(10.0, line_height));
        let margin = (0.0, 0.0);

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureVisible((rect, margin, None)),
            Target::Widget(self.scroll_widget_id),
        ));
    }

    pub fn key_event(&mut self, key: &KeyEvent) {}

    fn get_palette_type(&self) -> PaletteType {
        if self.input == "" {
            return PaletteType::File;
        }
        match self.input {
            _ if self.input.starts_with("/") => PaletteType::Line,
            _ if self.input.starts_with("@") => PaletteType::DocumentSymbol,
            _ if self.input.starts_with(">") => PaletteType::Workspace,
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
        self.update_palette(ctx, ui_state, env);
    }

    fn update_palette(
        &mut self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) {
        self.index = 0;
        let palette_type = self.get_palette_type();
        if self.palette_type != palette_type {
            self.palette_type = palette_type;
            match &self.palette_type {
                &PaletteType::File => self.items = self.get_files(),
                &PaletteType::Line => {
                    self.items = self.get_lines().unwrap_or(Vec::new())
                }
                &PaletteType::DocumentSymbol => {
                    self.items = self.get_document_symbols().unwrap_or(Vec::new())
                }
                &PaletteType::Workspace => self.items = self.get_workspaces(),
            }
            self.request_layout(ctx);
        } else {
            self.filter_items(ctx);
            self.preview(ctx, ui_state, env);
        }
        self.ensure_visible(ctx, env);
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
        self.update_palette(ctx, ui_state, env);
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
            &PaletteType::Line => 1,
            &PaletteType::DocumentSymbol => 1,
            &PaletteType::Workspace => 1,
        };

        if self.cursor == start {
            self.input = "".to_string();
            self.cursor = 0;
        } else {
            self.input.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
        self.update_palette(ctx, ui_state, env);
    }

    pub fn get_input(&self) -> &str {
        match &self.palette_type {
            PaletteType::File => &self.input,
            PaletteType::Line => &self.input[1..],
            PaletteType::DocumentSymbol => &self.input[1..],
            PaletteType::Workspace => &self.input[1..],
        }
    }

    pub fn filter_items(&mut self, ctx: &mut EventCtx) {
        let input = self.get_input().to_string();
        for item in self.items.iter_mut() {
            if input == "" {
                item.score = -1.0 - item.index as f64;
                item.match_mask = BitVec::new();
            } else {
                let text = item.get_text();
                if has_match(&input, &text) {
                    let result = locate(&input, &text);
                    item.score = result.score;
                    item.match_mask = result.match_mask;
                } else {
                    item.score = f64::NEG_INFINITY;
                }
            }
        }
        self.items
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Less));
        self.request_layout(ctx);
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
                    score: 0.0,
                    index: i,
                    match_mask: BitVec::new(),
                    workspace: Some(w.clone()),
                    position: None,
                    path: None,
                }
            })
            .collect()
    }

    fn get_document_symbols(&self) -> Option<Vec<PaletteItem>> {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let editor_split = state.editor_split.lock();
        let editor = editor_split.editors.get(&editor_split.active)?;
        let buffer_id = editor.buffer_id?;
        let buffer = editor_split.buffers.get(&buffer_id)?;
        let resp = state.lsp.lock().get_document_symbols(buffer)?;
        Some(match resp {
            DocumentSymbolResponse::Flat(symbols) => symbols
                .iter()
                .enumerate()
                .map(|(i, s)| PaletteItem {
                    window_id: self.window_id,
                    tab_id: self.tab_id,
                    kind: PaletteType::DocumentSymbol,
                    text: s.name.clone(),
                    hint: s.container_name.clone(),
                    position: Some(s.location.range.start),
                    path: None,
                    score: 0.0,
                    index: i,
                    match_mask: BitVec::new(),
                    icon: PaletteIcon::Symbol(s.kind),
                    workspace: None,
                })
                .collect(),
            DocumentSymbolResponse::Nested(symbols) => symbols
                .iter()
                .enumerate()
                .map(|(i, s)| PaletteItem {
                    window_id: self.window_id,
                    tab_id: self.tab_id,
                    kind: PaletteType::DocumentSymbol,
                    text: s.name.clone(),
                    hint: None,
                    path: None,
                    position: Some(s.range.start),
                    score: 0.0,
                    index: i,
                    match_mask: BitVec::new(),
                    icon: PaletteIcon::Symbol(s.kind),
                    workspace: None,
                })
                .collect(),
        })
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
                    score: 0.0,
                    index: i,
                    match_mask: BitVec::new(),
                    icon: PaletteIcon::None,
                    workspace: None,
                })
                .collect(),
        )
    }

    fn get_files(&self) -> Vec<PaletteItem> {
        let workspace_type = LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .workspace
            .lock()
            .kind
            .clone();
        match workspace_type {
            LapceWorkspaceType::RemoteSSH(user, host) => {
                self.get_ssh_files(&user, &host)
            }
            LapceWorkspaceType::Local => self.get_local_files(),
        }
    }

    fn get_ssh_files(&self, user: &str, host: &str) -> Vec<PaletteItem> {
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let mut ssh_session = state.ssh_session.lock();
        if ssh_session.is_none() {
            if let Ok(session) = SshSession::new(user, host) {
                *ssh_session = Some(session);
            } else {
                return Vec::new();
            }
        }
        let ssh_session = ssh_session.as_mut().unwrap();
        let workspace_path = state.workspace.lock().path.clone();
        let dir = workspace_path.to_str().unwrap();
        if let Ok(paths) = ssh_session.read_dir(dir) {
            return paths
                .iter()
                .enumerate()
                .map(|(index, p)| {
                    let path = PathBuf::from(p);
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
                            "go" => PaletteIcon::File("go_small".to_string()),
                            "cc" => PaletteIcon::File("cpp".to_string()),
                            s => PaletteIcon::File(s.to_string()),
                        }
                    } else {
                        PaletteIcon::None
                    };
                    let hint = folder.to_str().unwrap().to_string();
                    PaletteItem {
                        window_id: self.window_id,
                        tab_id: self.tab_id,
                        icon,
                        hint: Some(hint),
                        index,
                        kind: PaletteType::File,
                        text,
                        position: None,
                        path: Some(path),
                        score: 0.0,
                        match_mask: BitVec::new(),
                        workspace: None,
                    }
                })
                .collect();
        }
        Vec::new()
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
                        score: 0.0,
                        index,
                        match_mask: BitVec::new(),
                        icon,
                        workspace: None,
                    });
                    index += 1;
                }
            }
        }
        items
    }

    pub fn current_items(&self) -> Vec<&PaletteItem> {
        self.items
            .iter()
            .filter(|i| i.score != f64::NEG_INFINITY)
            .collect()
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

    pub fn request_layout(&self, ctx: &mut EventCtx) {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::RequestLayout,
            Target::Widget(self.widget_id),
        ))
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

        self.ensure_visible(ctx, env);
        self.request_paint(ctx);
        self.preview(ctx, ui_state, env);
    }
}

pub struct Palette {
    window_id: WindowId,
    tab_id: WidgetId,
    content: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    input: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    rect: Rect,
}

pub struct PaletteInput {
    window_id: WindowId,
    tab_id: WidgetId,
}

pub struct PaletteContent {
    window_id: WindowId,
    tab_id: WidgetId,
}

impl Palette {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        scroll_id: WidgetId,
    ) -> Palette {
        let palette_input = PaletteInput::new(window_id, tab_id)
            .padding((5.0, 5.0, 5.0, 5.0))
            .background(LapceTheme::EDITOR_BACKGROUND)
            .padding((5.0, 5.0, 5.0, 5.0));
        let palette_content =
            LapceScroll::new(PaletteContent::new(window_id, tab_id))
                .vertical()
                .with_id(scroll_id)
                .padding((5.0, 0.0, 5.0, 0.0));
        let palette = Palette {
            window_id,
            tab_id,
            input: WidgetPod::new(palette_input).boxed(),
            content: WidgetPod::new(palette_content).boxed(),
            rect: Rect::ZERO
                .with_origin(Point::new(50.0, 50.0))
                .with_size(Size::new(100.0, 50.0)),
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
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> PaletteContent {
        PaletteContent { window_id, tab_id }
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
        // if data.palette.same(&old_data.palette) {
        //     return;
        // }

        // if data.focus == LapceFocus::Palette {
        //     if old_data.focus == LapceFocus::Palette {
        //         self.input.update(ctx, data, env);
        //         self.content.update(ctx, data, env);
        //     } else {
        //         ctx.request_layout();
        //     }
        // } else {
        //     if old_data.focus == LapceFocus::Palette {
        //         ctx.request_paint();
        //     }
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        // let flex_size = self.flex.layout(ctx, bc, data, env);
        let input_size = self.input.layout(ctx, bc, data, env);
        self.input
            .set_layout_rect(ctx, data, env, Rect::ZERO.with_size(input_size));
        let content_bc = BoxConstraints::new(
            Size::ZERO,
            Size::new(bc.max().width, bc.max().height - input_size.height),
        );
        let content_size = self.content.layout(ctx, &content_bc, data, env);
        self.content.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO
                .with_origin(Point::new(0.0, input_size.height))
                .with_size(content_size),
        );
        // flex_size
        let size =
            Size::new(bc.max().width, content_size.height + input_size.height);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        if *LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .focus
            .lock()
            != LapceFocus::Palette
        {
            return;
        }
        let rects = ctx.region().rects();
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
        // if data.palette.index != old_data.palette.index {
        //     ctx.request_paint()
        // }
        // if data.palette.filtered_items.len()
        //     != old_data.palette.filtered_items.len()
        // {
        //     ctx.request_layout()
        // }
        // if data.palette.items.len() != old_data.palette.items.len() {
        //     ctx.request_layout()
        // }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let palette = state.palette.lock();
        let items_len = palette.current_items().len();
        let height = { line_height * items_len as f64 };
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        let state = LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
        let palette = state.palette.lock();
        for rect in rects {
            let start = (rect.y0 / line_height).floor() as usize;
            let items = {
                let items = palette.current_items();
                let items_len = items.len();
                &items[start
                    ..((rect.y1 / line_height).floor() as usize + 1).min(items_len)]
                    .to_vec()
            };

            for (i, item) in items.iter().enumerate() {
                if palette.index == start + i {
                    if let Some(background) = LAPCE_APP_STATE.theme.get("background")
                    {
                        ctx.fill(
                            Rect::ZERO
                                .with_origin(Point::new(
                                    rect.x0,
                                    (start + i) as f64 * line_height,
                                ))
                                .with_size(Size::new(rect.width(), line_height)),
                            background,
                        )
                    }
                }
                match &item.icon {
                    PaletteIcon::File(exten) => {
                        if let Some(svg_data) = file_svg(&exten) {
                            let x = 1.0;
                            let y = (start + i) as f64 * line_height + 2.0;
                            let affine = Affine::new([0.5, 0.0, 0.0, 0.5, x, y]);
                            svg_data.to_piet(affine, ctx);
                        }
                    }
                    PaletteIcon::Symbol(symbol) => {
                        if let Some(svg) = symbol_svg(&symbol) {
                            svg.to_piet(
                                Affine::translate(Vec2::new(
                                    1.0,
                                    (start + i) as f64 * line_height + 2.0,
                                )),
                                ctx,
                            );
                        }
                    }
                    _ => (),
                }
                let mut text_layout = ctx
                    .text()
                    .new_text_layout(item.text.clone())
                    .font(FontFamily::SYSTEM_UI, 14.0)
                    .text_color(env.get(LapceTheme::EDITOR_FOREGROUND));
                // if item.hint.is_some() {
                //     // text_layout = text_layout.range_attribute(
                //     //     item.text.len()
                //     //         ..item.text.len()
                //     //             + item.hint.as_ref().unwrap().len()
                //     //             + 1,
                //     //     TextAttribute::FontSize(13.0),
                //     // );
                //     text_layout = text_layout.range_attribute(
                //         item.text.len()
                //             ..item.text.len()
                //                 + item.hint.as_ref().unwrap().len()
                //                 + 1,
                //         TextAttribute::TextColor(
                //             env.get(LapceTheme::EDITOR_FOREGROUND).with_alpha(0.8),
                //         ),
                //     );
                // }
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
                    Point::new(20.0, (start + i) as f64 * line_height),
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
                            20.0 + text_x + 5.0,
                            (start + i) as f64 * line_height + 1.0,
                        ),
                    );
                }
            }
        }
    }
}

fn file_svg(exten: &str) -> Option<SvgData> {
    Some(
        SvgData::from_str(
            ICONS_DIR
                .get_file(format!("file_type_{}.svg", exten))?
                .contents_utf8()?,
        )
        .ok()?,
    )
}

fn symbol_svg(kind: &SymbolKind) -> Option<SvgData> {
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

    Some(
        SvgData::from_str(
            ICONS_DIR
                .get_file(format!("symbol-{}.svg", kind_str))
                .unwrap()
                .contents_utf8()?,
        )
        .ok()?,
    )
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
        Size::new(bc.max().width, env.get(LapceTheme::EDITOR_LINE_HEIGHT))
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
                        env,
                    );
            }
            &PaletteType::Workspace => {
                let state =
                    LAPCE_APP_STATE.get_tab_state(&self.window_id, &self.tab_id);
                *state.workspace.lock() = self.workspace.clone().unwrap();
                *state.ssh_session.lock() = None;
                state.start_plugin();
                ctx.request_paint();
            }
        }
    }
}
