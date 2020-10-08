use druid::{
    kurbo::{Line, Rect},
    widget::Container,
    widget::IdentityWrapper,
    Command, KeyEvent, Target, WidgetId,
};
use druid::{
    theme, BoxConstraints, Color, Cursor, Data, Env, Event, EventCtx,
    LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size,
    UpdateCtx, Widget, WidgetExt, WidgetPod,
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
use std::sync::{Arc, Mutex};
use std::thread;

use crate::{
    command::LapceCommand, command::LapceUICommand, command::LAPCE_COMMAND,
    command::LAPCE_UI_COMMAND, editor::EditorSplitState, scroll::LapceScroll,
    state::LapceFocus, state::LapceState, theme::LapceTheme,
};

#[derive(Clone, Debug)]
enum PaletteKind {
    File,
}

#[derive(Clone, Debug)]
pub struct PaletteItem {
    kind: PaletteKind,
    text: String,
    score: Score,
}

#[derive(Clone)]
pub struct PaletteState {
    widget_id: Option<WidgetId>,
    pub scroll_widget_id: WidgetId,
    input: String,
    cursor: usize,
    items: Vec<PaletteItem>,
    filtered_items: Vec<PaletteItem>,
    index: usize,
}

impl PaletteState {
    pub fn new() -> PaletteState {
        PaletteState {
            widget_id: None,
            scroll_widget_id: WidgetId::next(),
            items: Vec::new(),
            filtered_items: Vec::new(),
            input: "".to_string(),
            cursor: 0,
            index: 0,
        }
    }
}

impl PaletteState {
    pub fn run(&mut self) {
        self.items = self.get_files();
    }

    pub fn cancel(&mut self) {
        self.input = "".to_string();
        self.cursor = 0;
        self.index = 0;
    }

    fn ensure_visible(&self, ctx: &mut EventCtx, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rect = Rect::ZERO
            .with_origin(Point::new(0.0, self.index as f64 * line_height))
            .with_size(Size::new(10.0, line_height));
        let margin = (0.0, 0.0);

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureVisible((rect, margin)),
            Target::Widget(self.scroll_widget_id),
        ));
    }

    pub fn key_event(&mut self, key: &KeyEvent) {}

    pub fn insert(&mut self, ctx: &mut EventCtx, content: &str, env: &Env) {
        self.input.insert_str(self.cursor, content);
        self.cursor += content.len();
        self.index = 0;
        self.filter_items();
        self.ensure_visible(ctx, env);
    }

    pub fn move_cursor(&mut self, n: i64) {
        let cursor = (self.cursor as i64 + n)
            .max(0i64)
            .min(self.input.len() as i64) as usize;
        if self.cursor == cursor {
            return;
        }
        self.cursor = cursor;
    }

    pub fn delete_backward(&mut self, ctx: &mut EventCtx, env: &Env) {
        if self.cursor == 0 {
            return;
        }

        self.input.remove(self.cursor - 1);
        self.cursor = self.cursor - 1;
        self.index = 0;
        self.filter_items();
        self.ensure_visible(ctx, env);
    }

    pub fn delete_to_beginning_of_line(
        &mut self,
        ctx: &mut EventCtx,
        env: &Env,
    ) {
        if self.cursor == 0 {
            return;
        }

        self.input.replace_range(..self.cursor, "");
        self.cursor = 0;
        self.index = 0;
        self.filter_items();
        self.ensure_visible(ctx, env);
    }

    pub fn filter_items(&mut self) {
        let mut filtered_items: Vec<PaletteItem> = Vec::new();
        for item in &self.items {
            if has_match(&self.input, &item.text) {
                let result = locate(&self.input, &item.text);
                let filtered_item = PaletteItem {
                    kind: item.kind.clone(),
                    text: item.text.clone(),
                    score: result.score,
                };
                let index =
                    match filtered_items.binary_search_by(|other_item| {
                        filtered_item
                            .score
                            .partial_cmp(&other_item.score)
                            .unwrap()
                    }) {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                filtered_items.insert(index, filtered_item);
            }
        }
        self.filtered_items = filtered_items;
    }

    fn get_files(&self) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        let mut dirs = Vec::new();
        dirs.push(PathBuf::from("./"));
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    if path.as_path().to_str().unwrap().to_string()
                        != "./target"
                    {
                        dirs.push(path);
                    }
                } else {
                    let file = path.as_path().to_str().unwrap().to_string();
                    items.push(PaletteItem {
                        kind: PaletteKind::File,
                        text: file,
                        score: 0.0,
                    });
                }
            }
        }
        items
    }

    pub fn select(
        &mut self,
        ctx: &mut EventCtx,
        editor_split: &mut EditorSplitState,
    ) {
        let items = if self.input != "" {
            &self.filtered_items
        } else {
            &self.items
        };
        if items.is_empty() {
            return;
        }
        editor_split.open_file(ctx, &items[self.index].text);
        self.cancel();
    }

    pub fn change_index(&mut self, ctx: &mut EventCtx, n: i64, env: &Env) {
        let items = if self.input != "" {
            &self.filtered_items
        } else {
            &self.items
        };

        self.index = if self.index as i64 + n < 0 {
            (items.len() + self.index) as i64 + n
        } else if self.index as i64 + n > items.len() as i64 - 1 {
            self.index as i64 + n - items.len() as i64
        } else {
            self.index as i64 + n
        } as usize;

        self.ensure_visible(ctx, env);
    }
}

pub struct Palette {
    content: WidgetPod<LapceState, Box<dyn Widget<LapceState>>>,
    input: WidgetPod<LapceState, Box<dyn Widget<LapceState>>>,
    rect: Rect,
}

pub struct PaletteInput {}

pub struct PaletteContent {}

impl Palette {
    pub fn new(scroll_id: WidgetId) -> Palette {
        let palette_input = PaletteInput::new()
            .padding((5.0, 5.0, 5.0, 5.0))
            .border(LapceTheme::PALETTE_INPUT_BORDER, 1.0)
            .background(LapceTheme::PALETTE_INPUT_BACKGROUND)
            .padding((5.0, 5.0, 5.0, 5.0));
        let palette_content = IdentityWrapper::wrap(
            LapceScroll::new(PaletteContent::new()).vertical(),
            scroll_id,
        )
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

    pub fn run(&self) {
        // LAPCE_STATE.palette.lock().unwrap().items = self.get_files();
        self.update_height();
    }

    fn change_index(&self, n: i64) {
        // let input = self.state.lock().unwrap().input.clone();
        // let app_font = self.app.config.font.lock().unwrap().clone();
        // let size = self.get_rect().size();
        // let items = if input == "" {
        //     self.state.lock().unwrap().items.clone()
        // } else {
        //     self.state.lock().unwrap().filtered_items.clone()
        // };
        // let index = self.state.lock().unwrap().index;

        // let new_index = if index as i64 + n < 0 {
        //     items.len() as i64 + index as i64 + n
        // } else if index as i64 + n > items.len() as i64 - 1 {
        //     index as i64 + n - items.len() as i64
        // } else {
        //     index as i64 + n
        // } as usize;

        // self.state.lock().unwrap().index = new_index;
        // self.content.ensure_visble(
        //     Rect::from_origin_size(
        //         Point::new(0.0, new_index as f64 * app_font.lineheight()),
        //         Size::new(size.width, app_font.lineheight()),
        //     ),
        //     0.0,
        //     0.0,
        // );
        // self.invalidate();
    }

    fn update_height(&self) {
        // let height = {
        //     let input = &self.state.lock().unwrap().input.clone();
        //     let items = if input == "" {
        //         self.state.lock().unwrap().items.clone()
        //     } else {
        //         self.state.lock().unwrap().filtered_items.clone()
        //     };
        //     let size = self.get_rect().size();
        //     let padding = 5.0;
        //     let lines = if items.len() > 10 { 10 } else { items.len() };
        //     let app_font = self.app.config.font.lock().unwrap().clone();
        //     self.content.set_content_size(
        //         size.width,
        //         items.len() as f64 * app_font.lineheight(),
        //     );
        //     padding * 2.0
        //         + app_font.lineheight()
        //         + lines as f64 * app_font.lineheight()
        // };
        // let size = self.get_rect().size();
        // if height != size.height {
        //     self.invalidate();
        //     self.set_size(size.width, height);
        // }
    }

    fn get_files(&self) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        let mut dirs = Vec::new();
        dirs.push(PathBuf::from("./"));
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    if path.as_path().to_str().unwrap().to_string()
                        != "./target"
                    {
                        dirs.push(path);
                    }
                } else {
                    let file = path.as_path().to_str().unwrap().to_string();
                    items.push(PaletteItem {
                        kind: PaletteKind::File,
                        text: file,
                        score: 0.0,
                    });
                }
            }
        }
        items
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

impl Widget<LapceState> for Palette {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
        env: &Env,
    ) {
        self.content.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceState,
        env: &Env,
    ) {
        self.content.lifecycle(ctx, event, data, env);
        self.input.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &Env,
    ) {
        if data.palette.same(&old_data.palette) {
            return;
        }

        if data.focus == LapceFocus::Palette {
            if old_data.focus == LapceFocus::Palette {
                self.input.update(ctx, data, env);
                self.content.update(ctx, data, env);
            } else {
                ctx.request_layout();
            }
        } else {
            if old_data.focus == LapceFocus::Palette {
                ctx.request_paint();
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceState,
        env: &Env,
    ) -> Size {
        // let flex_size = self.flex.layout(ctx, bc, data, env);
        let input_size = self.input.layout(ctx, bc, data, env);
        self.input.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(input_size),
        );
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
        if data.focus != LapceFocus::Palette {
            return;
        }
        let rects = ctx.region().rects();
        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);
    }
}

impl Widget<LapceState> for PaletteContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &Env,
    ) {
        if data.palette.index != old_data.palette.index {
            ctx.request_paint()
        }
        if data.palette.filtered_items.len()
            != old_data.palette.filtered_items.len()
        {
            ctx.request_layout()
        }
        if data.palette.items.len() != old_data.palette.items.len() {
            ctx.request_layout()
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceState,
        env: &Env,
    ) -> Size {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let items_len = if data.palette.input != "" {
            data.palette.filtered_items.len()
        } else {
            data.palette.items.len()
        };
        let height = { line_height * items_len as f64 };
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        // let state = LAPCE_STATE.palette.lock().unwrap();
        for rect in rects {
            let start = (rect.y0 / line_height).floor() as usize;
            let items = {
                let items = if data.palette.input != "" {
                    &data.palette.filtered_items
                } else {
                    &data.palette.items
                };
                let items_len = items.len();
                &items[start
                    ..((rect.y1 / line_height).floor() as usize + 1)
                        .min(items_len)]
                    .to_vec()
            };

            for (i, item) in items.iter().enumerate() {
                if data.palette.index == start + i {
                    ctx.fill(
                        Rect::ZERO
                            .with_origin(Point::new(
                                rect.x0,
                                (start + i) as f64 * line_height,
                            ))
                            .with_size(Size::new(rect.width(), line_height)),
                        &Color::rgb8(50, 50, 50),
                    )
                }
                let mut text_layout = TextLayout::new(item.text.as_ref());
                text_layout.rebuild_if_needed(ctx.text(), env);
                text_layout.draw(
                    ctx,
                    Point::new(0.0, (start + i) as f64 * line_height),
                );
            }
        }
    }
}

impl Widget<LapceState> for PaletteInput {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceState,
        data: &LapceState,
        env: &Env,
    ) {
        if old_data.palette.input != data.palette.input
            || old_data.palette.cursor != data.palette.cursor
        {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceState,
        env: &Env,
    ) -> Size {
        Size::new(bc.max().width, env.get(LapceTheme::EDITOR_LINE_HEIGHT))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceState, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let text = data.palette.input.clone();
        let cursor = data.palette.cursor;
        // let text = LAPCE_STATE.palette.lock().unwrap().input.clone();
        // let cursor = LAPCE_STATE.palette.lock().unwrap().cursor;
        let mut text_layout = TextLayout::new(text.as_ref());
        text_layout.set_text_color(LapceTheme::PALETTE_INPUT_FOREROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        let line = text_layout.cursor_line_for_text_position(cursor);
        ctx.stroke(line, &env.get(LapceTheme::PALETTE_INPUT_FOREROUND), 1.0);
        text_layout.draw(ctx, Point::new(0.0, 0.0));
    }
}
