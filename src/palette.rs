use druid::{
    kurbo::{Line, Rect},
    widget::Container,
    Target, WidgetId,
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
    command::CraneCommand, command::CraneUICommand, command::CRANE_COMMAND,
    command::CRANE_UI_COMMAND, state::CRANE_STATE, theme::CraneTheme,
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

pub struct PaletteState {
    widget_id: Option<WidgetId>,
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
            items: Vec::new(),
            filtered_items: Vec::new(),
            input: "".to_string(),
            cursor: 0,
            index: 0,
        }
    }
}

impl PaletteState {
    pub fn set_widget_id(&mut self, id: WidgetId) {
        self.widget_id = Some(id);
    }

    pub fn run(&mut self) {
        self.items = self.get_files();
        let target = Target::Global;
        let target = { Target::Widget(self.widget_id.unwrap().clone()) };
        CRANE_STATE
            .ui_sink
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .submit_command(CRANE_UI_COMMAND, CraneUICommand::Show, target);
    }

    pub fn cancel(&mut self) {
        self.input = "".to_string();
        self.cursor = 0;
    }

    pub fn insert(&mut self, content: &str) {
        self.input.insert_str(self.cursor, content);
        self.cursor += content.len();
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
}

pub struct Palette<T> {
    content: WidgetPod<T, Box<dyn Widget<T>>>,
    input: WidgetPod<T, Box<dyn Widget<T>>>,
    rect: Rect,
}

pub struct PaletteWrapper<T> {
    palette: WidgetPod<T, Box<dyn Widget<T>>>,
    hidden: bool,
}

pub struct PaletteInput {}

pub struct PaletteContent {}

impl<T: Data> Palette<T> {
    pub fn new() -> Palette<T> {
        let palette_input = PaletteInput::new()
            .padding((5.0, 5.0, 5.0, 5.0))
            .border(CraneTheme::PALETTE_INPUT_BORDER, 1.0)
            .background(CraneTheme::PALETTE_INPUT_BACKGROUND)
            .padding((5.0, 5.0, 5.0, 5.0));
        let palette_content = Scroll::new(PaletteContent::new())
            .vertical()
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
        CRANE_STATE.palette.lock().unwrap().items = self.get_files();
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
        CRANE_STATE.palette.lock().unwrap().input = "".to_string();
        CRANE_STATE.palette.lock().unwrap().cursor = 0;
        CRANE_STATE.palette.lock().unwrap().index = 0;
        // self.content.set_scroll(0.0, 0.0);
        // self.hide();
    }

    // fn layout(&self) {
    // self.row.set_rect(self.get_rect().with_origin(Point::ZERO));
    // let parent = self.parent();
    // if parent.is_none() {
    //     return;
    // }
    // let parent_width = parent.unwrap().get_rect().size().width;
    // let width = self.get_rect().size().width;
    // let old_x = self.get_rect().origin().x;

    // let x = if parent_width >= width {
    //     ((parent_width - width) / 2.0).floor()
    // } else {
    //     0.0
    // };
    // if old_x != x {
    //     self.set_pos(x, 0.0)
    // }
    // }

    // fn paint_input(&self, paint_ctx: &mut PaintCtx) {
    //     let app_font = self.app.config.font.lock().unwrap();
    //     let padding = 5.0;
    //     let size = self.get_rect().size();
    //     paint_ctx.fill(
    //         Rect::from_origin_size(
    //             Point::new(padding, padding),
    //             Size::new(size.width - padding * 2.0, app_font.lineheight()),
    //         ),
    //         &Color::rgb8(0, 0, 0),
    //     );
    //     paint_ctx.fill(
    //         Rect::from_origin_size(
    //             Point::new(padding + 1.0, padding + 1.0),
    //             Size::new(
    //                 size.width - padding * 2.0 - 2.0,
    //                 app_font.lineheight() - 2.0,
    //             ),
    //         ),
    //         &Color::rgb8(33, 37, 43),
    //     );

    //     let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
    //     let input = &self.state.lock().unwrap().input.clone();
    //     let cursor = self.state.lock().unwrap().cursor;
    //     let font = paint_ctx
    //         .text()
    //         .new_font_by_name("", 13.0)
    //         .unwrap()
    //         .build()
    //         .unwrap();
    //     let layout = paint_ctx
    //         .text()
    //         .new_text_layout(&font, &input)
    //         .unwrap()
    //         .build()
    //         .unwrap();
    //     paint_ctx.draw_text(
    //         &layout,
    //         Point::new(
    //             padding * 2.0,
    //             padding + app_font.ascent + app_font.linespace / 2.0,
    //         ),
    //         &Color::rgba8(fg.r, fg.g, fg.b, 255),
    //     );

    //     let x = paint_ctx
    //         .text()
    //         .new_text_layout(&font, &input[..cursor])
    //         .unwrap()
    //         .build()
    //         .unwrap()
    //         .width();
    //     paint_ctx.fill(
    //         Rect::from_origin_size(
    //             Point::new(padding * 2.0 + x, padding + app_font.linespace / 2.0 - 2.0),
    //             Size::new(1.0, app_font.ascent + app_font.descent + 4.0),
    //         ),
    //         &Color::rgba8(fg.r, fg.g, fg.b, 255),
    //     );
    // }

    // fn paint(&self, paint_ctx: &mut PaintCtx) {}
    //     self.paint_input(paint_ctx);
    //     let size = self.get_rect().size();

    //     let padding = 5.0;

    //     let app_font = self.app.config.font.lock().unwrap();
    //     let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
    //     let index = self.state.lock().unwrap().index;

    //     paint_ctx.fill(
    //         Rect::from_origin_size(
    //             Point::new(
    //                 0.0,
    //                 padding * 2.0 + app_font.lineheight() * (index + 1) as f64,
    //             ),
    //             Size::new(size.width, app_font.lineheight()),
    //         ),
    //         &Color::rgba8(fg.r, fg.g, fg.b, 20),
    //     );

    //     let input = &self.state.lock().unwrap().input.clone();

    //     let font = paint_ctx
    //         .text()
    //         .new_font_by_name("", 13.0)
    //         .unwrap()
    //         .build()
    //         .unwrap();

    //     let mut i = 0;
    //     let start = 0;
    //     let height = self.widget_state.lock().unwrap().size().height;
    //     let lineheight = app_font.lineheight();
    //     let line_count = (height / lineheight) as usize + 2;

    //     let items = if input == "" {
    //         self.state.lock().unwrap().items.clone()
    //     } else {
    //         self.state.lock().unwrap().filtered_items.clone()
    //     };

    //     let end = if start + line_count > items.len() {
    //         items.len()
    //     } else {
    //         start + line_count
    //     };

    //     for item in items.get(start..end).unwrap() {
    //         let layout = paint_ctx
    //             .text()
    //             .new_text_layout(&font, &item.text)
    //             .unwrap()
    //             .build()
    //             .unwrap();
    //         paint_ctx.draw_text(
    //             &layout,
    //             Point::new(
    //                 padding * 2.0,
    //                 padding * 2.0
    //                     + app_font.lineheight() * (i + 1) as f64
    //                     + app_font.ascent
    //                     + app_font.linespace / 2.0,
    //             ),
    //             &Color::rgba8(fg.r, fg.g, fg.b, 255),
    //         );
    //         i += 1;
    //     }

    //     // paint_ctx.restore();
    // }

    // fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    // fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    // fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
    //     false
    // }
}

// impl CommandRunner for Palette {
//     fn run(&self, cmd: Cmd, key_input: KeyInput) {
//         match cmd.clone().cmd.unwrap() {
//             Command::Escape => {
//                 self.cancel();
//                 self.invalidate();
//             }
//             Command::Execute => {
//                 let input = self.state.lock().unwrap().input.clone();
//                 let items = if input == "" {
//                     self.state.lock().unwrap().items.clone()
//                 } else {
//                     self.state.lock().unwrap().filtered_items.clone()
//                 };
//                 let index = self.state.lock().unwrap().index;
//                 let file_path = items[index].text.clone();
//                 self.cancel();
//                 self.app.get_active_editor().load_file(file_path);
//             }
//             Command::DeleteToBeginningOfLine => {
//                 let cursor = self.state.lock().unwrap().cursor;
//                 self.state.lock().unwrap().input.replace_range(..cursor, "");
//                 self.state.lock().unwrap().cursor = 0;
//                 self.state.lock().unwrap().index = 0;
//                 self.content.set_scroll(0.0, 0.0);
//                 self.invalidate();
//                 self.filter_items();
//                 self.invalidate();
//             }
//             Command::MoveDown => {
//                 self.change_index(1);
//             }
//             Command::MoveUp => {
//                 self.change_index(-1);
//             }
//             Command::DeleteBackward => {
//                 let cursor = self.state.lock().unwrap().cursor;
//                 if cursor == 0 {
//                     return;
//                 }
//                 self.state.lock().unwrap().input.remove(cursor - 1);
//                 self.state.lock().unwrap().cursor = cursor - 1;
//                 self.state.lock().unwrap().index = 0;
//                 self.content.set_scroll(0.0, 0.0);
//                 self.invalidate();
//                 self.filter_items();
//                 self.invalidate();
//             }
//             Command::Unknown => {
//                 if key_input.text != ""
//                     && !key_input.mods.ctrl
//                     && !key_input.mods.alt
//                     && !key_input.mods.meta
//                 {
//                     let cursor = self.state.lock().unwrap().cursor;
//                     self.state
//                         .lock()
//                         .unwrap()
//                         .input
//                         .insert_str(cursor, &key_input.text);
//                     self.state.lock().unwrap().cursor =
//                         cursor + key_input.text.len();
//                     self.state.lock().unwrap().index = 0;
//                     self.content.set_scroll(0.0, 0.0);
//                     self.invalidate();
//                     self.filter_items();
//                     self.invalidate();
//                 }
//             }
//             _ => (),
//         }
//     }
// }

impl PaletteInput {
    pub fn new() -> PaletteInput {
        PaletteInput {}
    }

    // fn layout(&self) {}

    // fn paint(&self, paint_ctx: &mut PaintCtx) {
    // let app_font = self.app.config.font.lock().unwrap().clone();
    // let padding = 5.0;
    // let size = self.get_rect().size();
    // paint_ctx.fill(
    //     Rect::from_origin_size(
    //         Point::new(padding, padding),
    //         Size::new(size.width - padding * 2.0, app_font.lineheight()),
    //     ),
    //     &Color::rgb8(0, 0, 0),
    // );
    // paint_ctx.fill(
    //     Rect::from_origin_size(
    //         Point::new(padding + 1.0, padding + 1.0),
    //         Size::new(
    //             size.width - padding * 2.0 - 2.0,
    //             app_font.lineheight() - 2.0,
    //         ),
    //     ),
    //     &Color::rgb8(33, 37, 43),
    // );

    // let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
    // let input = &self.state.lock().unwrap().input.clone();
    // let cursor = self.state.lock().unwrap().cursor;
    // let font = paint_ctx
    //     .text()
    //     .new_font_by_name("", 13.0)
    //     .unwrap()
    //     .build()
    //     .unwrap();
    // let layout = paint_ctx
    //     .text()
    //     .new_text_layout(&font, &input)
    //     .unwrap()
    //     .build()
    //     .unwrap();
    // paint_ctx.draw_text(
    //     &layout,
    //     Point::new(
    //         padding * 2.0,
    //         padding + app_font.ascent + app_font.linespace / 2.0,
    //     ),
    //     &Color::rgba8(fg.r, fg.g, fg.b, 255),
    // );

    // let x = paint_ctx
    //     .text()
    //     .new_text_layout(&font, &input[..cursor])
    //     .unwrap()
    //     .build()
    //     .unwrap()
    //     .width();
    // paint_ctx.fill(
    //     Rect::from_origin_size(
    //         Point::new(
    //             padding * 2.0 + x,
    //             padding + app_font.linespace / 2.0 - 2.0,
    //         ),
    //         Size::new(1.0, app_font.ascent + app_font.descent + 4.0),
    //     ),
    //     &Color::rgba8(fg.r, fg.g, fg.b, 255),
    // );
    // }

    // fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    // fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    // fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
    //     false
    // }
}

impl<T: Data> PaletteWrapper<T> {
    pub fn new(data: T) -> PaletteWrapper<T> {
        let palette = WidgetPod::new(
            Palette::new()
                .border(theme::BORDER_LIGHT, 1.0)
                .background(CraneTheme::PALETTE_BACKGROUND),
        )
        .boxed();
        PaletteWrapper {
            palette,
            hidden: true,
        }
    }
}

impl PaletteContent {
    pub fn new() -> PaletteContent {
        PaletteContent {}
    }

    // fn layout(&self) {}

    // fn paint(&self, paint_ctx: &mut PaintCtx) {
    // let size = self.get_rect().size();

    // let app_font = self.app.config.font.lock().unwrap().clone();
    // let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
    // let index = self.state.lock().unwrap().index;

    // paint_ctx.fill(
    //     Rect::from_origin_size(
    //         Point::new(0.0, app_font.lineheight() * index as f64),
    //         Size::new(size.width, app_font.lineheight()),
    //     ),
    //     &Color::rgba8(fg.r, fg.g, fg.b, 20),
    // );

    // let input = &self.state.lock().unwrap().input.clone();

    // let font = paint_ctx
    //     .text()
    //     .new_font_by_name("", 13.0)
    //     .unwrap()
    //     .build()
    //     .unwrap();

    // let start =
    //     (self.vertical_scroll() / app_font.lineheight()).floor() as usize;
    // let height = self.widget_state.lock().unwrap().size().height;
    // let lineheight = app_font.lineheight();
    // let line_count = (height / lineheight) as usize + 2;

    // let items = if input == "" {
    //     self.state.lock().unwrap().items.clone()
    // } else {
    //     self.state.lock().unwrap().filtered_items.clone()
    // };

    // let end = if start + line_count > items.len() {
    //     items.len()
    // } else {
    //     start + line_count
    // };

    // for i in start..end {
    //     if let Some(item) = items.get(i) {
    //         let layout = paint_ctx
    //             .text()
    //             .new_text_layout(&font, &item.text)
    //             .unwrap()
    //             .build()
    //             .unwrap();
    //         paint_ctx.draw_text(
    //             &layout,
    //             Point::new(
    //                 10.0,
    //                 app_font.lineheight() * i as f64
    //                     + app_font.ascent
    //                     + app_font.linespace / 2.0,
    //             ),
    //             &Color::rgba8(fg.r, fg.g, fg.b, 255),
    //         );
    //     }
    // }
    // }

    // fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    // fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    // fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
    //     false
    // }
}

impl<T: Data> Widget<T> for Palette<T> {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
        self.content.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        self.content.lifecycle(ctx, event, data, env);
        self.input.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
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
        let size = Size::new(
            content_size.width,
            content_size.height + input_size.height,
        );
        println!("palette size {:?} {:?}", size, bc);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);
    }
}

impl<T> Widget<T> for PaletteContent {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        let line_height = env.get(CraneTheme::EDITOR_LINE_HEIGHT);
        let state = CRANE_STATE.palette.lock().unwrap();
        let items_len = if state.input != "" {
            state.filtered_items.len()
        } else {
            state.items.len()
        };
        let height = { line_height * items_len as f64 };
        println!("content layout size {:?}", bc.max());
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let line_height = env.get(CraneTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        let state = CRANE_STATE.palette.lock().unwrap();
        for rect in rects {
            let start = (rect.y0 / line_height).floor() as usize;
            let items = {
                let items = if state.input != "" {
                    &state.filtered_items
                } else {
                    &state.items
                };
                let items_len = items.len();
                &items[start
                    ..((rect.y1 / line_height).floor() as usize + 1)
                        .min(items_len)]
                    .to_vec()
            };

            for (i, item) in items.iter().enumerate() {
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

impl<T: Data> Widget<T> for PaletteWrapper<T> {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
        println!("event {:?}", event);
        match event {
            Event::Command(cmd) => match cmd {
                _ if cmd.is(CRANE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(CRANE_UI_COMMAND);
                    match command {
                        CraneUICommand::Show => {
                            self.hidden = false;
                            ctx.request_layout();
                        }
                        CraneUICommand::Hide => {
                            self.hidden = true;
                            ctx.request_paint();
                        }
                    }
                }
                _ => (),
            },
            _ => (),
        }
        self.palette.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        self.palette.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        let size = self.palette.layout(ctx, bc, data, env);
        self.palette.set_layout_rect(
            ctx,
            data,
            env,
            Rect::ZERO.with_size(size),
        );
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        if self.hidden {
            return;
        }
        self.palette.paint(ctx, data, env);
    }
}

impl<T> Widget<T> for PaletteInput {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        Size::new(bc.max().width, env.get(CraneTheme::EDITOR_LINE_HEIGHT))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let line_height = env.get(CraneTheme::EDITOR_LINE_HEIGHT);
        let text = CRANE_STATE.palette.lock().unwrap().input.clone();
        let mut text_layout = TextLayout::new(text.as_ref());
        text_layout.set_text_color(CraneTheme::PALETTE_INPUT_FOREROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        text_layout.draw(ctx, Point::new(0.0, 0.0));
        // println!("input region {:?}", ctx.region());
        // let rects = ctx.region().rects().to_vec();
        // for rect in rects {
        //     ctx.fill(rect, &env.get(theme::BORDER_LIGHT))
        // }
    }
}
