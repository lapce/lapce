use bit_vec::BitVec;
use druid::{
    kurbo::{Line, Rect},
    piet::TextAttribute,
    widget::Container,
    widget::IdentityWrapper,
    Command, FontFamily, FontWeight, KeyEvent, Target, WidgetId,
};
use druid::{
    piet::{Text, TextLayoutBuilder},
    theme, BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetExt, WidgetPod,
};
use druid::{
    widget::{CrossAxisAlignment, Flex, FlexParams, Label, Scroll},
    TextLayout,
};
use fzyr::{has_match, locate, Score};
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
    command::LAPCE_UI_COMMAND, editor::EditorSplitState, scroll::LapceScroll,
    state::LapceFocus, state::LapceUIState, state::LAPCE_STATE, theme::LapceTheme,
};

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteType {
    File,
    Line,
}

#[derive(Clone, Debug)]
pub struct PaletteItem {
    kind: PaletteType,
    text: String,
    score: Score,
    index: usize,
    match_mask: BitVec,
}

#[derive(Clone)]
pub struct PaletteState {
    pub widget_id: WidgetId,
    pub scroll_widget_id: WidgetId,
    input: String,
    cursor: usize,
    items: Vec<PaletteItem>,
    index: usize,
    palette_type: PaletteType,
}

impl PaletteState {
    pub fn new() -> PaletteState {
        PaletteState {
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
                LAPCE_STATE.editor_split.lock().save_selection();
            }
            _ => self.items = self.get_files(),
        }
    }

    pub fn cancel(&mut self, ctx: &mut EventCtx, ui_state: &mut LapceUIState) {
        match &self.palette_type {
            &PaletteType::Line => {
                LAPCE_STATE
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
        }
    }

    pub fn filter_items(&mut self, ctx: &mut EventCtx) {
        let input = self.get_input().to_string();
        for item in self.items.iter_mut() {
            if input == "" {
                item.score = -1.0 - item.index as f64;
                item.match_mask = BitVec::new();
            } else {
                if has_match(&input, &item.text) {
                    let result = locate(&input, &item.text);
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

    fn get_lines(&self) -> Option<Vec<PaletteItem>> {
        let editor_split = LAPCE_STATE.editor_split.lock();
        let editor = editor_split.editors.get(&editor_split.active)?;
        let buffer_id = editor.buffer_id?;
        let buffer = editor_split.buffers.get(&buffer_id)?;
        Some(
            buffer
                .rope
                .lines(0..buffer.len())
                .enumerate()
                .map(|(i, l)| PaletteItem {
                    kind: PaletteType::Line,
                    text: format!("{}: {}", i, l.to_string()),
                    score: 0.0,
                    index: i,
                    match_mask: BitVec::new(),
                })
                .collect(),
        )
    }

    fn get_files(&self) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        let mut dirs = Vec::new();
        let mut index = 0;
        dirs.push(PathBuf::from("./"));
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_name().to_str().unwrap().starts_with(".") {
                    continue;
                }
                if path.is_dir() {
                    if path.as_path().to_str().unwrap().to_string() != "./target" {
                        dirs.push(path);
                    }
                } else {
                    let file = path.as_path().to_str().unwrap().to_string();
                    items.push(PaletteItem {
                        kind: PaletteType::File,
                        text: file,
                        score: 0.0,
                        index,
                        match_mask: BitVec::new(),
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
    content: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    input: WidgetPod<LapceUIState, Box<dyn Widget<LapceUIState>>>,
    rect: Rect,
}

pub struct PaletteInput {}

pub struct PaletteContent {}

impl Palette {
    pub fn new(scroll_id: WidgetId) -> Palette {
        let palette_input = PaletteInput::new()
            .padding((5.0, 5.0, 5.0, 5.0))
            .background(LapceTheme::EDITOR_BACKGROUND)
            .padding((5.0, 5.0, 5.0, 5.0));
        let palette_content = LapceScroll::new(PaletteContent::new())
            .vertical()
            .with_id(scroll_id)
            .padding((5.0, 0.0, 5.0, 0.0));
        let palette = Palette {
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
    pub fn new() -> PaletteInput {
        PaletteInput {}
    }
}

impl PaletteContent {
    pub fn new() -> PaletteContent {
        PaletteContent {}
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
        if *LAPCE_STATE.focus.lock() != LapceFocus::Palette {
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
        let palette = LAPCE_STATE.palette.lock();
        let items_len = palette.current_items().len();
        let height = { line_height * items_len as f64 };
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        let palette = LAPCE_STATE.palette.lock();
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
                    if let Some(background) = LAPCE_STATE.theme.get("background") {
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
                    Point::new(0.0, (start + i) as f64 * line_height),
                );
            }
        }
    }
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
        let palette = LAPCE_STATE.palette.lock();
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let text = palette.input.clone();
        let cursor = palette.cursor;
        let mut text_layout = TextLayout::new(text.as_ref());
        text_layout.set_text_color(LapceTheme::EDITOR_FOREGROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        let line = text_layout.cursor_line_for_text_position(cursor);
        ctx.stroke(line, &env.get(LapceTheme::EDITOR_FOREGROUND), 1.0);
        text_layout.draw(ctx, Point::new(0.0, 0.0));
    }
}

impl PaletteItem {
    pub fn select(
        &self,
        ctx: &mut EventCtx,
        ui_state: &mut LapceUIState,
        env: &Env,
    ) {
        match &self.kind {
            &PaletteType::File => {
                let mut path = PathBuf::from_str(&self.text)
                    .unwrap()
                    .canonicalize()
                    .unwrap();
                LAPCE_STATE.editor_split.lock().open_file(
                    ctx,
                    ui_state,
                    path.to_str().unwrap(),
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
                LAPCE_STATE
                    .editor_split
                    .lock()
                    .jump_to_line(ctx, ui_state, line, env);
            }
        }
    }
}
