use druid::{
    kurbo::{Line, Rect},
    widget::Container,
    widget::IdentityWrapper,
    KeyEvent, Target, WidgetId,
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
    command::LAPCE_UI_COMMAND, scroll::LapceScroll, state::LapceWidget,
    state::LAPCE_STATE, theme::LapceTheme,
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
    scroll_widget_id: Option<WidgetId>,
    line_height: f64,
    width: f64,
    input: String,
    cursor: usize,
    items: Vec<PaletteItem>,
    filtered_items: Vec<PaletteItem>,
    index: usize,
    pub hidden: bool,
}

impl PaletteState {
    pub fn new() -> PaletteState {
        PaletteState {
            widget_id: None,
            scroll_widget_id: None,
            line_height: 0.0,
            width: 0.0,
            items: Vec::new(),
            filtered_items: Vec::new(),
            input: "".to_string(),
            cursor: 0,
            index: 0,
            hidden: true,
        }
    }
}

impl PaletteState {
    pub fn set_widget_id(&mut self, id: WidgetId) {
        self.widget_id = Some(id);
    }

    pub fn set_scroll_widget_id(&mut self, id: WidgetId) {
        self.scroll_widget_id = Some(id);
    }

    pub fn set_line_height(&mut self, line_height: f64) {
        self.line_height = line_height;
    }

    pub fn set_width(&mut self, width: f64) {
        self.width = width;
    }

    pub fn run(&mut self) {
        self.items = self.get_files();
        self.hidden = false;
        *LAPCE_STATE.focus.lock().unwrap() = LapceWidget::Palette;
        self.request_layout();
    }

    pub fn cancel(&mut self) {
        self.input = "".to_string();
        self.cursor = 0;
        self.index = 0;
        self.hidden = true;
        *LAPCE_STATE.focus.lock().unwrap() = LapceWidget::Editor;
        self.request_paint();
    }

    fn request_layout(&self) {
        LAPCE_STATE.submit_ui_command(
            LapceUICommand::RequestLayout,
            self.widget_id.unwrap(),
        );
    }

    fn request_paint(&self) {
        LAPCE_STATE.submit_ui_command(
            LapceUICommand::RequestPaint,
            self.widget_id.unwrap(),
        );
    }

    fn ensure_visible(&self) {
        let rect = Rect::ZERO
            .with_origin(Point::new(0.0, self.index as f64 * self.line_height))
            .with_size(Size::new(self.width, self.line_height));
        let margin = (0.0, 0.0);

        LAPCE_STATE.submit_ui_command(
            LapceUICommand::EnsureVisible((rect, margin)),
            self.widget_id.unwrap(),
        );
    }

    pub fn key_event(&mut self, key: &KeyEvent) {}

    pub fn insert(&mut self, content: &str) {
        self.input.insert_str(self.cursor, content);
        self.cursor += content.len();
        self.index = 0;
        self.filter_items();
        self.ensure_visible();
        self.request_layout();
    }

    pub fn move_cursor(&mut self, n: i64) {
        let cursor = (self.cursor as i64 + n)
            .max(0i64)
            .min(self.input.len() as i64) as usize;
        if self.cursor == cursor {
            return;
        }
        self.cursor = cursor;
        self.request_paint();
    }

    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }

        self.input.remove(self.cursor - 1);
        self.cursor = self.cursor - 1;
        self.index = 0;
        self.filter_items();
        self.ensure_visible();
        self.request_layout();
    }

    pub fn delete_to_beginning_of_line(&mut self) {
        if self.cursor == 0 {
            return;
        }

        self.input.replace_range(..self.cursor, "");
        self.cursor = 0;
        self.index = 0;
        self.filter_items();
        self.ensure_visible();
        self.request_layout();
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

    pub fn select(&mut self) {
        let items = if self.input != "" {
            &self.filtered_items
        } else {
            &self.items
        };
        if items.is_empty() {
            return;
        }
        LAPCE_STATE.open_file(&items[self.index].text);
        self.cancel();
    }

    pub fn change_index(&mut self, n: i64) {
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

        self.ensure_visible();
        self.request_paint();
    }
}

pub struct Palette<T> {
    content: WidgetPod<T, Box<dyn Widget<T>>>,
    input: WidgetPod<T, Box<dyn Widget<T>>>,
    rect: Rect,
}

pub struct PaletteWrapper<T> {
    palette: WidgetPod<T, Box<dyn Widget<T>>>,
}

pub struct PaletteInput {}

pub struct PaletteContent {}

impl<T: Data> Palette<T> {
    pub fn new() -> Palette<T> {
        let palette_input = PaletteInput::new()
            .padding((5.0, 5.0, 5.0, 5.0))
            .border(LapceTheme::PALETTE_INPUT_BORDER, 1.0)
            .background(LapceTheme::PALETTE_INPUT_BACKGROUND)
            .padding((5.0, 5.0, 5.0, 5.0));
        let palette_scroll_id = WidgetId::next();
        LAPCE_STATE
            .palette
            .lock()
            .unwrap()
            .set_scroll_widget_id(palette_scroll_id);
        let palette_content = IdentityWrapper::wrap(
            LapceScroll::new(PaletteContent::new()).vertical(),
            palette_scroll_id,
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
        LAPCE_STATE.palette.lock().unwrap().items = self.get_files();
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
        LAPCE_STATE.palette.lock().unwrap().input = "".to_string();
        LAPCE_STATE.palette.lock().unwrap().cursor = 0;
        LAPCE_STATE.palette.lock().unwrap().index = 0;
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
    pub fn new() -> PaletteWrapper<T> {
        let palette = WidgetPod::new(
            Palette::new()
                .border(theme::BORDER_LIGHT, 1.0)
                .background(LapceTheme::PALETTE_BACKGROUND),
        )
        .boxed();
        PaletteWrapper { palette }
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
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.input.paint(ctx, data, env);
        self.content.paint(ctx, data, env);
    }
}

impl<T: Data> Widget<T> for PaletteContent {
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
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        {
            let mut state = LAPCE_STATE.palette.lock().unwrap();
            state.set_line_height(line_height);
            state.set_width(bc.max().width);
        }
        let state = LAPCE_STATE.palette.lock().unwrap();
        let items_len = if state.input != "" {
            state.filtered_items.len()
        } else {
            state.items.len()
        };
        let height = { line_height * items_len as f64 };
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let rects = ctx.region().rects().to_vec();
        let state = LAPCE_STATE.palette.lock().unwrap();
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
                if state.index == start + i {
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

impl<T: Data> Widget<T> for PaletteWrapper<T> {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
        match event {
            Event::Internal(_) => self.palette.event(ctx, event, data, env),
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
                        _ => println!(
                            "palette unprocessed ui command {:?}",
                            command
                        ),
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
        if LAPCE_STATE.palette.lock().unwrap().hidden {
            return;
        }
        self.palette.paint(ctx, data, env);
    }
}

impl<T: Data> Widget<T> for PaletteInput {
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
        Size::new(bc.max().width, env.get(LapceTheme::EDITOR_LINE_HEIGHT))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let text = LAPCE_STATE.palette.lock().unwrap().input.clone();
        let cursor = LAPCE_STATE.palette.lock().unwrap().cursor;
        let mut text_layout = TextLayout::new(text.as_ref());
        text_layout.set_text_color(LapceTheme::PALETTE_INPUT_FOREROUND);
        text_layout.rebuild_if_needed(ctx.text(), env);
        let line = text_layout.cursor_line_for_text_position(cursor);
        ctx.stroke(line, &env.get(LapceTheme::PALETTE_INPUT_FOREROUND), 1.0);
        text_layout.draw(ctx, Point::new(0.0, 0.0));
        // println!("input region {:?}", ctx.region());
        // let rects = ctx.region().rects().to_vec();
        // for rect in rects {
        //     ctx.fill(rect, &env.get(theme::BORDER_LIGHT))
        // }
    }
}
