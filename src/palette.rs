use crate::app::{App, CommandRunner};
use crate::editor::{Editor, View};
use crate::input::{Cmd, Command, Input, InputState, KeyInput};
use crane_ui::{Flex, Row, Widget, WidgetState, WidgetTrait};
use crane_ui_macros::WidgetBase;
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet};
use fzyr::{has_match, locate, Score};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{
    Color, FontBuilder, LinearGradient, RenderContext, Text, TextLayout, TextLayoutBuilder,
    UnitPoint,
};
use serde_json::{self, json, Value};
use std::cmp::Ordering;
use std::fs::{self, DirEntry};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

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
    input: String,
    cursor: usize,
    items: Vec<PaletteItem>,
    filtered_items: Vec<PaletteItem>,
    index: usize,
}

impl PaletteState {
    pub fn new() -> PaletteState {
        PaletteState {
            items: Vec::new(),
            filtered_items: Vec::new(),
            input: "".to_string(),
            cursor: 0,
            index: 0,
        }
    }
}

#[derive(Clone, WidgetBase)]
pub struct Palette {
    widget_state: Arc<Mutex<WidgetState>>,
    state: Arc<Mutex<PaletteState>>,
    row: Flex,
    input: PaletteInput,
    content: PaletteContent,
    app: Box<App>,
}

#[derive(Clone, WidgetBase)]
pub struct PaletteInput {
    widget_state: Arc<Mutex<WidgetState>>,
    state: Arc<Mutex<PaletteState>>,
    app: App,
}

#[derive(Clone, WidgetBase)]
pub struct PaletteContent {
    widget_state: Arc<Mutex<WidgetState>>,
    state: Arc<Mutex<PaletteState>>,
    app: App,
}

impl Palette {
    pub fn new(app: App) -> Palette {
        let state = Arc::new(Mutex::new(PaletteState::new()));
        let palette = Palette {
            widget_state: Arc::new(Mutex::new(WidgetState::new())),
            state: state.clone(),
            app: Box::new(app.clone()),
            input: PaletteInput::new(app.clone(), state.clone()),
            content: PaletteContent::new(app.clone(), state.clone()),
            row: Row::new(),
        };
        palette.input.set_size(
            0.0,
            5.0 * 2.0 + app.config.font.lock().unwrap().lineheight(),
        );
        palette.row.add_child(Box::new(palette.input.clone()));
        palette.row.add_child(Box::new(palette.content.clone()));
        palette.add_child(Box::new(palette.row.clone()));
        palette
    }

    pub fn run(&self) {
        self.state.lock().unwrap().items = self.get_files();
        self.update_height();
        self.show();
    }

    fn change_index(&self, n: i64) {
        let input = self.state.lock().unwrap().input.clone();
        let app_font = self.app.config.font.lock().unwrap().clone();
        let size = self.get_rect().size();
        let items = if input == "" {
            self.state.lock().unwrap().items.clone()
        } else {
            self.state.lock().unwrap().filtered_items.clone()
        };
        let index = self.state.lock().unwrap().index;

        let new_index = if index as i64 + n < 0 {
            items.len() as i64 + index as i64 + n
        } else if index as i64 + n > items.len() as i64 - 1 {
            index as i64 + n - items.len() as i64
        } else {
            index as i64 + n
        } as usize;

        self.state.lock().unwrap().index = new_index;
        self.content.ensure_visble(
            Rect::from_origin_size(
                Point::new(0.0, new_index as f64 * app_font.lineheight()),
                Size::new(size.width, app_font.lineheight()),
            ),
            0.0,
            0.0,
        );
        self.invalidate();
    }

    fn update_height(&self) {
        let height = {
            let input = &self.state.lock().unwrap().input.clone();
            let items = if input == "" {
                self.state.lock().unwrap().items.clone()
            } else {
                self.state.lock().unwrap().filtered_items.clone()
            };
            let size = self.get_rect().size();
            let padding = 5.0;
            let lines = if items.len() > 10 { 10 } else { items.len() };
            let app_font = self.app.config.font.lock().unwrap().clone();
            self.content
                .set_content_size(size.width, items.len() as f64 * app_font.lineheight());
            padding * 2.0 + app_font.lineheight() + lines as f64 * app_font.lineheight()
        };
        let size = self.get_rect().size();
        if height != size.height {
            self.invalidate();
            self.set_size(size.width, height);
        }
    }

    fn filter_items(&self) {
        let mut filtered_items: Vec<PaletteItem> = Vec::new();
        let items = self.state.lock().unwrap().items.clone();
        let input = self.state.lock().unwrap().input.clone();
        for item in items {
            if has_match(&input, &item.text) {
                let result = locate(&input, &item.text);
                let filtered_item = PaletteItem {
                    kind: item.kind.clone(),
                    text: item.text.clone(),
                    score: result.score,
                };
                let index = match filtered_items.binary_search_by(|other_item| {
                    filtered_item.score.partial_cmp(&other_item.score).unwrap()
                }) {
                    Ok(index) => index,
                    Err(index) => index,
                };
                filtered_items.insert(index, filtered_item);
            }
        }
        self.state.lock().unwrap().filtered_items = filtered_items;
        self.update_height();
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
                    if path.as_path().to_str().unwrap().to_string() != "./target" {
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
        self.state.lock().unwrap().input = "".to_string();
        self.state.lock().unwrap().cursor = 0;
        self.state.lock().unwrap().index = 0;
        self.content.set_scroll(0.0, 0.0);
        self.hide();
    }

    fn layout(&self) {
        self.row.set_rect(self.get_rect().with_origin(Point::ZERO));
        let parent = self.parent();
        if parent.is_none() {
            return;
        }
        let parent_width = parent.unwrap().get_rect().size().width;
        let width = self.get_rect().size().width;
        let old_x = self.get_rect().origin().x;

        let x = if parent_width >= width {
            ((parent_width - width) / 2.0).floor()
        } else {
            0.0
        };
        if old_x != x {
            self.set_pos(x, 0.0)
        }
    }

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

    fn paint(&self, paint_ctx: &mut PaintCtx) {}
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

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        false
    }
}

impl CommandRunner for Palette {
    fn run(&self, cmd: Cmd, key_input: KeyInput) {
        match cmd.clone().cmd.unwrap() {
            Command::Escape => {
                self.cancel();
                self.invalidate();
            }
            Command::Execute => {
                let input = self.state.lock().unwrap().input.clone();
                let items = if input == "" {
                    self.state.lock().unwrap().items.clone()
                } else {
                    self.state.lock().unwrap().filtered_items.clone()
                };
                let index = self.state.lock().unwrap().index;
                let file_path = items[index].text.clone();
                self.cancel();
                self.app.get_active_editor().load_file(file_path);
            }
            Command::DeleteToBeginningOfLine => {
                let cursor = self.state.lock().unwrap().cursor;
                self.state.lock().unwrap().input.replace_range(..cursor, "");
                self.state.lock().unwrap().cursor = 0;
                self.state.lock().unwrap().index = 0;
                self.content.set_scroll(0.0, 0.0);
                self.invalidate();
                self.filter_items();
                self.invalidate();
            }
            Command::MoveDown => {
                self.change_index(1);
            }
            Command::MoveUp => {
                self.change_index(-1);
            }
            Command::DeleteBackward => {
                let cursor = self.state.lock().unwrap().cursor;
                if cursor == 0 {
                    return;
                }
                self.state.lock().unwrap().input.remove(cursor - 1);
                self.state.lock().unwrap().cursor = cursor - 1;
                self.state.lock().unwrap().index = 0;
                self.content.set_scroll(0.0, 0.0);
                self.invalidate();
                self.filter_items();
                self.invalidate();
            }
            Command::Unknown => {
                if key_input.text != ""
                    && !key_input.mods.ctrl
                    && !key_input.mods.alt
                    && !key_input.mods.meta
                {
                    let cursor = self.state.lock().unwrap().cursor;
                    self.state
                        .lock()
                        .unwrap()
                        .input
                        .insert_str(cursor, &key_input.text);
                    self.state.lock().unwrap().cursor = cursor + key_input.text.len();
                    self.state.lock().unwrap().index = 0;
                    self.content.set_scroll(0.0, 0.0);
                    self.invalidate();
                    self.filter_items();
                    self.invalidate();
                }
            }
            _ => (),
        }
    }
}

impl PaletteInput {
    pub fn new(app: App, state: Arc<Mutex<PaletteState>>) -> PaletteInput {
        PaletteInput {
            widget_state: Arc::new(Mutex::new(WidgetState::new())),
            state,
            app,
        }
    }

    fn layout(&self) {}

    fn paint(&self, paint_ctx: &mut PaintCtx) {
        let app_font = self.app.config.font.lock().unwrap().clone();
        let padding = 5.0;
        let size = self.get_rect().size();
        paint_ctx.fill(
            Rect::from_origin_size(
                Point::new(padding, padding),
                Size::new(size.width - padding * 2.0, app_font.lineheight()),
            ),
            &Color::rgb8(0, 0, 0),
        );
        paint_ctx.fill(
            Rect::from_origin_size(
                Point::new(padding + 1.0, padding + 1.0),
                Size::new(
                    size.width - padding * 2.0 - 2.0,
                    app_font.lineheight() - 2.0,
                ),
            ),
            &Color::rgb8(33, 37, 43),
        );

        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
        let input = &self.state.lock().unwrap().input.clone();
        let cursor = self.state.lock().unwrap().cursor;
        let font = paint_ctx
            .text()
            .new_font_by_name("", 13.0)
            .unwrap()
            .build()
            .unwrap();
        let layout = paint_ctx
            .text()
            .new_text_layout(&font, &input)
            .unwrap()
            .build()
            .unwrap();
        paint_ctx.draw_text(
            &layout,
            Point::new(
                padding * 2.0,
                padding + app_font.ascent + app_font.linespace / 2.0,
            ),
            &Color::rgba8(fg.r, fg.g, fg.b, 255),
        );

        let x = paint_ctx
            .text()
            .new_text_layout(&font, &input[..cursor])
            .unwrap()
            .build()
            .unwrap()
            .width();
        paint_ctx.fill(
            Rect::from_origin_size(
                Point::new(padding * 2.0 + x, padding + app_font.linespace / 2.0 - 2.0),
                Size::new(1.0, app_font.ascent + app_font.descent + 4.0),
            ),
            &Color::rgba8(fg.r, fg.g, fg.b, 255),
        );
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        false
    }
}

impl PaletteContent {
    pub fn new(app: App, state: Arc<Mutex<PaletteState>>) -> PaletteContent {
        PaletteContent {
            widget_state: Arc::new(Mutex::new(WidgetState::new())),
            state,
            app,
        }
    }

    fn layout(&self) {}

    fn paint(&self, paint_ctx: &mut PaintCtx) {
        let size = self.get_rect().size();

        let app_font = self.app.config.font.lock().unwrap().clone();
        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
        let index = self.state.lock().unwrap().index;

        paint_ctx.fill(
            Rect::from_origin_size(
                Point::new(0.0, app_font.lineheight() * index as f64),
                Size::new(size.width, app_font.lineheight()),
            ),
            &Color::rgba8(fg.r, fg.g, fg.b, 20),
        );

        let input = &self.state.lock().unwrap().input.clone();

        let font = paint_ctx
            .text()
            .new_font_by_name("", 13.0)
            .unwrap()
            .build()
            .unwrap();

        let start = (self.vertical_scroll() / app_font.lineheight()).floor() as usize;
        let height = self.widget_state.lock().unwrap().size().height;
        let lineheight = app_font.lineheight();
        let line_count = (height / lineheight) as usize + 2;

        let items = if input == "" {
            self.state.lock().unwrap().items.clone()
        } else {
            self.state.lock().unwrap().filtered_items.clone()
        };

        let end = if start + line_count > items.len() {
            items.len()
        } else {
            start + line_count
        };

        for i in start..end {
            if let Some(item) = items.get(i) {
                let layout = paint_ctx
                    .text()
                    .new_text_layout(&font, &item.text)
                    .unwrap()
                    .build()
                    .unwrap();
                paint_ctx.draw_text(
                    &layout,
                    Point::new(
                        10.0,
                        app_font.lineheight() * i as f64
                            + app_font.ascent
                            + app_font.linespace / 2.0,
                    ),
                    &Color::rgba8(fg.r, fg.g, fg.b, 255),
                );
            }
        }
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        false
    }
}
