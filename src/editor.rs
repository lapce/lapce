use crate::app::App;
use crate::config::AppFont;
use crate::config::Config;
use crate::input::{Cmd, Command, Input, InputState, KeyInput};
use crate::line_cache::{count_utf16, Annotation, Line, LineCache, Style};
use crate::rpc::Core;
use cairo::{FontFace, FontOptions, FontSlant, FontWeight, Matrix, ScaledFont};
use crane_ui::{Widget, WidgetState};
use crane_ui_macros::WidgetBase;
use druid::shell::keyboard::{KeyCode, KeyEvent, KeyModifiers};
use druid::shell::platform::IdleHandle;
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::shell::{kurbo, piet, runloop, WindowBuilder};
use druid::{BoxConstraints, PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, RoundedRect, Size, Vec2};
use piet::{Color, FontBuilder, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use uuid::Uuid;

pub enum EditViewCommands {
    ViewId(String),
    ApplyUpdate(Value),
    ScrollTo(usize),
    Core(Weak<Mutex<Core>>),
    Undo,
    Redo,
    UpperCase,
    LowerCase,
    Transpose,
    AddCursorAbove,
    AddCursorBelow,
    SingleSelection,
    SelectAll,
}

struct EditorState {
    view: Option<View>,
    input: Input,
}

#[derive(WidgetBase, Clone)]
pub struct Editor {
    state: Arc<Mutex<WidgetState>>,
    local_state: Arc<Mutex<EditorState>>,
    app: App,
}

impl Editor {
    pub fn new(app: App) -> Editor {
        Editor {
            state: Arc::new(Mutex::new(WidgetState::new())),
            local_state: Arc::new(Mutex::new(EditorState {
                view: None,
                input: Input::new(),
            })),
            app,
        }
    }

    pub fn load_view(&self, view: View) {
        self.local_state.lock().unwrap().view = Some(view);
    }

    fn get_view_width(&self) -> f64 {
        match &self.local_state.lock().unwrap().view {
            Some(view) => view.state.lock().unwrap().width,
            None => 0.0,
        }
    }

    fn get_view_height(&self) -> f64 {
        match &self.local_state.lock().unwrap().view {
            Some(view) => view.state.lock().unwrap().height,
            None => 0.0,
        }
    }

    fn layout(&self) {}

    fn paint(&self, paint_ctx: &mut PaintCtx) {
        if self.local_state.lock().unwrap().view.is_none() {
            return;
        }
        let bg = self.app.config.theme.lock().unwrap().background.unwrap();
        let rect = Rect::from_origin_size(Point::ORIGIN, self.state.lock().unwrap().size());
        paint_ctx.fill(rect, &Color::rgba8(bg.r, bg.g, bg.b, bg.a));

        self.paint_scroll(paint_ctx);

        let horizontal_scroll = self.state.lock().unwrap().horizontal_scroll();
        let vertical_scroll = self.state.lock().unwrap().vertical_scroll();
        let height = self.state.lock().unwrap().size().height;
        paint_ctx.save();
        paint_ctx.transform(Affine::translate(Vec2::new(
            -horizontal_scroll,
            -vertical_scroll,
        )));

        let line_cache = self
            .local_state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .line_cache
            .clone();
        let annotations = line_cache.lock().unwrap().annotations();
        let lineheight = self.app.config.font.lock().unwrap().lineheight();
        let start = (vertical_scroll / lineheight) as usize;
        let line_count = (height / lineheight) as usize + 2;
        for i in start..start + line_count {
            if let Some(line) = line_cache.lock().unwrap().get_line(i) {
                self.paint_line(i, paint_ctx, line, &annotations);
            }
        }
        paint_ctx.restore();
    }

    fn paint_line(
        &self,
        i: usize,
        paint_ctx: &mut PaintCtx,
        line: &Line,
        annotations: &Vec<Annotation>,
    ) {
        let mut text = line.text().to_string();
        // text.pop();
        let text_len = count_utf16(&text);

        let app_font = self.app.config.font.lock().unwrap();

        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();

        let input_state = self.local_state.lock().unwrap().input.state.clone();

        let visual_line = self.local_state.lock().unwrap().input.visual_line.clone();

        for annotation in annotations {
            let (start, end) = annotation.check_line(i, line);
            if input_state == InputState::Visual && (start != 0 || end != 0) {
                let point = if visual_line {
                    Point::new(0.0, app_font.lineheight() * i as f64)
                } else {
                    Point::new(
                        app_font.width * start as f64,
                        app_font.lineheight() * i as f64,
                    )
                };
                let size = if visual_line {
                    Size::new(app_font.width * text_len as f64, app_font.lineheight())
                } else {
                    Size::new(
                        app_font.width * (end - start + 1) as f64,
                        app_font.lineheight(),
                    )
                };
                let rect = Rect::from_origin_size(point, size);
                let selection_color = Color::rgba8(fg.r, fg.g, fg.b, 50);
                paint_ctx.fill(rect, &selection_color);
            }
        }

        let cursor_color = Color::rgba8(fg.r, fg.g, fg.b, 160);
        for cursor in line.cursor() {
            let point = Point::new(
                app_font.width * *cursor as f64,
                app_font.lineheight() * i as f64,
            );
            let rect = Rect::from_origin_size(
                point,
                Size::new(
                    match input_state {
                        InputState::Insert => 1.0,
                        InputState::Visual => app_font.width,
                        InputState::Normal => app_font.width,
                    },
                    app_font.lineheight(),
                ),
            );
            paint_ctx.fill(rect, &cursor_color);
        }

        let font = paint_ctx
            .text()
            .new_font_by_name("Consolas", 13.0)
            .unwrap()
            .build()
            .unwrap();
        for style in line.styles() {
            let range = &style.range;
            let mut end = range.end;
            if end == text_len {
                if &text[end - 1..end] == "\n" {
                    end -= 1;
                }
            }
            let layout = paint_ctx
                .text()
                .new_text_layout(&font, &text[range.start..end])
                .unwrap()
                .build()
                .unwrap();
            let x = paint_ctx
                .text()
                .new_text_layout(&font, &text[..range.start])
                .unwrap()
                .build()
                .unwrap()
                .width();
            if let Some(style) = self.app.config.styles.lock().unwrap().get(&style.style_id) {
                if let Some(fg_color) = style.fg_color {
                    paint_ctx.draw_text(
                        &layout,
                        Point::new(
                            x,
                            app_font.lineheight() * i as f64
                                + app_font.ascent
                                + app_font.linespace / 2.0,
                        ),
                        &Color::from_rgba32_u32(fg_color),
                    );
                }
            }
        }
    }

    fn paint_scroll(&self, paint_ctx: &mut PaintCtx) {
        let scroll_thickness = 9.0;
        let view_width = self.get_view_width();
        let view_height = self.get_view_height();
        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
        let color = Color::rgba8(fg.r, fg.g, fg.b, 40);
        let size = self.state.lock().unwrap().size();
        let horizontal_scroll = self.state.lock().unwrap().horizontal_scroll();
        let vertical_scroll = self.state.lock().unwrap().vertical_scroll();
        if view_width > size.width {
            let width = size.width / view_width * size.width;
            let point = Point::new(
                horizontal_scroll / view_width * size.width,
                size.height - scroll_thickness,
            );
            let rect = Rect::from_origin_size(point, Size::new(width, scroll_thickness));
            paint_ctx.fill(rect, &color);
        }
        if view_height > size.height {
            let height = size.height / view_height * size.height;
            let point = Point::new(
                size.width - scroll_thickness,
                vertical_scroll / view_height * size.height,
            );
            let rect = Rect::from_origin_size(point, Size::new(scroll_thickness, height));
            paint_ctx.fill(rect, &color);
        }
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        self.app.set_active_editor(&self);
        if self.local_state.lock().unwrap().view.is_none() {
            return;
        }

        let font = self.app.config.font.lock().unwrap();
        let view_id = self
            .local_state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .id()
            .clone();

        let horizontal_scroll = self.state.lock().unwrap().horizontal_scroll();
        let vertical_scroll = self.state.lock().unwrap().vertical_scroll();
        let line = ((event.pos.y as f64 + vertical_scroll) / font.lineheight()) as u32;
        let col = ((event.pos.x as f64 + horizontal_scroll) / font.width) as u32;
        self.app.core.send_notification(
            "edit",
            &json!({
                "view_id": view_id,
                "method": "gesture",
                "params": {
                    "col":col,
                    "line":line,
                    "ty": "point_select"
                },
            }),
        );
    }

    fn ensure_visble(&self, rect: Rect, margin_x: f64, margin_y: f64) {
        let mut scroll_x = 0.0;
        let mut scroll_y = 0.0;
        let size = self.state.lock().unwrap().size();
        let horizontal_scroll = self.state.lock().unwrap().horizontal_scroll();
        let vertical_scroll = self.state.lock().unwrap().vertical_scroll();
        let right_limit = size.width + horizontal_scroll - margin_x;
        let left_limit = horizontal_scroll + margin_x;
        if rect.x1 > right_limit {
            scroll_x = rect.x1 - right_limit;
        } else if rect.x0 < left_limit {
            scroll_x = rect.x0 - left_limit;
        }

        let bottom_limit = size.height + vertical_scroll - margin_y;
        let top_limit = vertical_scroll + margin_y;
        if rect.y1 > bottom_limit {
            scroll_y = rect.y1 - bottom_limit;
        } else if rect.y0 < top_limit {
            scroll_y = rect.y0 - top_limit;
        }

        self.scroll(Vec2::new(scroll_x, scroll_y));
    }

    fn scroll(&self, delta: Vec2) {
        if delta.x == 0.0 && delta.y == 0.0 {
            return;
        }
        self.send_scroll();

        let view_width = self.get_view_width();
        let view_height = self.get_view_height();
        let size = self.state.lock().unwrap().size();

        let mut horizontal_scroll = self.state.lock().unwrap().horizontal_scroll();
        let mut vertical_scroll = self.state.lock().unwrap().vertical_scroll();
        horizontal_scroll += delta.x;
        if horizontal_scroll > view_width - size.width {
            horizontal_scroll = view_width - size.width;
        }
        if horizontal_scroll < 0.0 {
            horizontal_scroll = 0.0;
        }

        vertical_scroll += delta.y;
        if vertical_scroll > view_height - size.height {
            vertical_scroll = view_height - size.height;
        }
        if vertical_scroll < 0.0 {
            vertical_scroll = 0.0;
        }
        self.state
            .lock()
            .unwrap()
            .set_scroll(horizontal_scroll, vertical_scroll);
        self.invalidate();
    }

    fn send_scroll(&self) {
        if self.local_state.lock().unwrap().view.is_none() {
            return;
        }
        let view_id = self
            .local_state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .id()
            .clone();
        let lineheight = self.app.config.font.lock().unwrap().lineheight();
        let horizontal_scroll = self.state.lock().unwrap().horizontal_scroll();
        let vertical_scroll = self.state.lock().unwrap().vertical_scroll();
        let size = self.state.lock().unwrap().size();
        let start = (vertical_scroll / lineheight) as usize;
        let line_count = (size.height / lineheight) as usize + 2;
        let core = self.app.core.clone();
        thread::spawn(move || {
            core.send_notification(
                "edit",
                &json!({
                    "view_id": view_id,
                    "method": "scroll",
                    "params": [start, start+line_count],
                }),
            );
        });
    }

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
        self.scroll(delta);
    }

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) {
        let config = self.app.config.clone();
        let keymaps = config.keymaps.lock().unwrap();
        let input_state = self.local_state.lock().unwrap().input.state.clone();
        let key_input = KeyInput::from_keyevent(&event);
        if key_input.text == "" {
            return;
        }
        let mut pending_keys = self.local_state.lock().unwrap().input.pending_keys.clone();
        pending_keys.push(key_input.clone());
        for key in &pending_keys {
            println!("current key is {}", key);
        }
        let cmd = keymaps.get(input_state, pending_keys.clone());
        if cmd.more_input {
            self.local_state.lock().unwrap().input.pending_keys = pending_keys;
            return;
        } else {
            self.local_state.lock().unwrap().input.pending_keys = Vec::new();
            if cmd.clone().cmd.unwrap() == Command::Unknown {
                for key in pending_keys {
                    self.run(
                        Cmd {
                            cmd: Some(Command::Unknown),
                            more_input: false,
                        },
                        key,
                    );
                }
                return;
            }
        }
        self.run(cmd, key_input);
    }

    fn run(&self, cmd: Cmd, key_input: KeyInput) {
        if self.local_state.lock().unwrap().view.is_none() {
            return;
        }
        let view_id = self
            .local_state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .id()
            .clone();

        let mut count = self.local_state.lock().unwrap().input.count;
        if count == 0 {
            count = 1;
        }

        let input_state = self.local_state.lock().unwrap().input.state.clone();
        let line_selection = self.local_state.lock().unwrap().input.visual_line.clone();

        let caret = match input_state {
            InputState::Insert => true,
            _ => false,
        };
        let is_selection = match input_state {
            InputState::Visual => true,
            _ => false,
        };

        match cmd.clone().cmd.unwrap() {
            Command::Insert => {
                self.local_state.lock().unwrap().input.state = InputState::Insert;
                self.app.get_active_editor().invalidate();
            }
            Command::Visual => {
                self.local_state.lock().unwrap().input.state = InputState::Visual;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "no_move",
                        "params": {"is_selection": true, "line_selection": false},
                    }),
                );
                self.app.get_active_editor().invalidate();
            }
            Command::VisualLine => {
                self.local_state.lock().unwrap().input.state = InputState::Visual;
                self.local_state.lock().unwrap().input.visual_line = true;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "no_move",
                        "params": {"is_selection": true, "line_selection": true},
                    }),
                );
                self.app.get_active_editor().invalidate();
            }
            Command::Escape => {
                self.local_state.lock().unwrap().input.state = InputState::Normal;
                self.local_state.lock().unwrap().input.count = 0;
                self.local_state.lock().unwrap().input.visual_line = false;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "no_move",
                        "params": {"is_selection": false, "line_selection": false},
                    }),
                );
                self.app.get_active_editor().invalidate();
            }
            Command::Undo => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "undo",
                    }),
                );
                self.app.get_active_editor().invalidate();
            }
            Command::Redo => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "redo",
                    }),
                );
                self.app.get_active_editor().invalidate();
            }
            Command::NewLineBelow => {
                self.local_state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "insert_newline_below",
                    }),
                );
            }
            Command::NewLineAbove => {
                self.local_state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "insert_newline_above",
                    }),
                );
            }
            Command::InsertStartOfLine => {
                self.local_state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_left_end_of_line",
                    }),
                );
            }
            Command::AppendEndOfLine => {
                self.local_state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_right_end_of_line",
                    }),
                );
            }
            Command::DeleteForwardInsert => {
                self.local_state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "delete_forward",
                    }),
                );
            }
            Command::DeleteForward => {
                if input_state == InputState::Visual {
                    self.local_state.lock().unwrap().input.state = InputState::Normal;
                }
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "delete_forward",
                    }),
                );
            }
            Command::DeleteBackward => {
                if input_state == InputState::Visual {
                    self.local_state.lock().unwrap().input.state = InputState::Normal;
                }
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "delete_backward",
                    }),
                );
            }
            Command::ScrollPageUp => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "scroll_page_up",
                    }),
                );
            }
            Command::ScrollPageDown => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "scroll_page_down",
                    }),
                );
            }
            Command::MoveUp => {
                self.app.core.send_notification(
                        "edit",
                        &json!({
                            "view_id": view_id,
                            "method": "move_up",
                        "params": {"count": count, "is_selection": is_selection, "line_selection": line_selection, "caret": caret},
                        }),
                    );
            }
            Command::MoveDown => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_down",
                        "params": {"count": count, "is_selection": is_selection, "line_selection": line_selection, "caret": caret},
                    }),
                );
            }
            Command::MoveLeft => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_left",
                        "params": {"count": count, "is_selection": is_selection, "line_selection": line_selection, "caret": caret},
                    }),
                );
            }
            Command::MoveRight => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_right",
                        "params": {"count": count, "is_selection": is_selection, "line_selection": line_selection, "caret": caret},
                    }),
                );
            }
            Command::MoveWordLeft => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_word_left",
                        "params": {"count": count, "is_selection": is_selection, "line_selection": line_selection, "caret": caret},
                    }),
                );
            }
            Command::MoveWordRight => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_word_right",
                        "params": {"count": count, "is_selection": is_selection, "line_selection": line_selection, "caret": caret},
                    }),
                );
            }
            Command::SplitHorizontal => {}
            Command::SplitVertical => {
                let app = self.app.clone();
                let view = self
                    .local_state
                    .lock()
                    .unwrap()
                    .view
                    .as_ref()
                    .unwrap()
                    .clone();
                thread::spawn(move || {
                    let editor = Editor::new(app.clone());
                    app.editors
                        .lock()
                        .unwrap()
                        .insert(editor.id().clone(), editor.clone());
                    editor.load_view(view);
                    app.main_flex.add_child(Box::new(editor));
                });
                // let new_editor_view = Arc::new(Mutex::new(Box::new(EditorView::new(
                //     self.idle_handle.clone(),
                //     self.window_handle.clone(),
                //     self.core.clone(),
                //     self.view.clone(),
                //     self.config.clone(),
                // ))
                //     as Box<Widget + Send + Sync>));

                // let new_editor_view_id = new_editor_view.lock().unwrap().id();

                // self.view
                //     .lock()
                //     .unwrap()
                //     .editor_views
                //     .lock()
                //     .unwrap()
                //     .insert(new_editor_view_id, new_editor_view.clone());

                // let parent = self.parent.clone().unwrap().clone();

                // thread::spawn(move || {
                //     parent.lock().unwrap().add_child(new_editor_view.clone());
                //     new_editor_view.lock().unwrap().set_parent(parent);
                // });
            }
            Command::Unknown => {
                if input_state == InputState::Insert
                    && key_input.text != ""
                    && !key_input.mods.ctrl
                    && !key_input.mods.alt
                    && !key_input.mods.meta
                {
                    let chars = match key_input.key_code {
                        KeyCode::Return => "\n".to_string(),
                        _ => key_input.text.clone(),
                    };
                    self.app.core.send_notification(
                        "edit",
                        &json!({
                            "view_id": view_id,
                            "method": "insert",
                            "params": {"chars": chars},
                        }),
                    );
                }
            }
        }

        if input_state == InputState::Normal {
            match cmd.cmd.unwrap() {
                Command::Unknown => {
                    if let Ok(n) = key_input.text.parse::<u64>() {
                        let count = self.local_state.lock().unwrap().input.count;
                        self.local_state.lock().unwrap().input.count = count * 10 + n;
                    } else {
                        self.local_state.lock().unwrap().input.count = 0;
                    }
                }
                _ => self.local_state.lock().unwrap().input.count = 0,
            };
        }
    }
}

pub struct EditorView {
    id: String,
    idle_handle: IdleHandle,
    window_handle: WindowHandle,
    core: Arc<Mutex<Core>>,
    view: Arc<Mutex<View>>,
    config: Config,
    input: Input,
    width: f64,
    height: f64,
    vertical_scroll: f64,
    horizontal_scroll: f64,
    parent: Option<Arc<Mutex<Box<Widget + Send + Sync>>>>,
    needs_update: bool,
}

struct ViewState {
    width: f64,
    height: f64,
}

#[derive(Clone)]
pub struct View {
    id: String,
    app: App,
    state: Arc<Mutex<ViewState>>,
    pub line_cache: Arc<Mutex<LineCache>>,
}

impl View {
    pub fn new(view_id: String, app: App) -> View {
        View {
            id: view_id,
            app,
            line_cache: Arc::new(Mutex::new(LineCache::new())),
            state: Arc::new(Mutex::new(ViewState {
                width: 0.0,
                height: 0.0,
            })),
        }
    }

    pub fn id(&self) -> String {
        self.id.clone()
    }

    pub fn apply_update(&self, update: &Value) {
        self.line_cache.lock().unwrap().apply_update(update);
        let lineheight = self.app.config.font.lock().unwrap().lineheight();
        let font_width = self.app.config.font.lock().unwrap().width;
        self.state.lock().unwrap().height =
            self.line_cache.lock().unwrap().height() as f64 * lineheight;
        let mut width = 0.0;
        let line_count = self.line_cache.lock().unwrap().height();
        for i in 0..line_count {
            if let Some(line) = self.line_cache.lock().unwrap().get_line(i) {
                let current_width = count_utf16(line.text()) as f64 * font_width;
                if current_width > width {
                    width = current_width;
                }
            }
        }
        self.state.lock().unwrap().width = width;
        println!("finish apply update");

        if let Some(editor) = self.get_active_editor() {
            editor.invalidate();
        }
    }

    // pub fn set_editor_view_by_id(&mut self, editor_view_id: &str) {
    //     let editor_view = self
    //         .editor_views
    //         .lock()
    //         .unwrap()
    //         .get(editor_view_id)
    //         .unwrap()
    //         .clone();
    //     self.set_editor_view(editor_view);
    // }

    // pub fn set_editor_view(&mut self, editor_view: Arc<Mutex<Box<Widget + Send + Sync>>>) {
    //     self.editor_view = Some(editor_view);
    // }

    fn get_active_editor(&self) -> Option<Editor> {
        let editor = self.app.get_active_editor();
        if editor.local_state.lock().unwrap().view.clone().unwrap().id == self.id {
            return Some(editor);
        }
        None
    }

    pub fn scroll_to(&self, col: u64, line: u64) {
        if let Some(editor) = self.get_active_editor() {
            let (font_width, line_height) = {
                let font = self.app.config.font.lock().unwrap();
                let font_width = font.width;
                let line_height = font.lineheight();
                (font_width, line_height)
            };
            let rect = Rect::from_origin_size(
                Point::new(col as f64 * font_width, line as f64 * line_height),
                Size::new(font_width, line_height),
            );
            let editor = editor.clone();
            thread::spawn(move || {
                editor.ensure_visble(rect, font_width, line_height);
            });
        }
    }
}

fn scale_matrix(scale: f64) -> Matrix {
    Matrix {
        xx: scale,
        yx: 0.0,
        xy: 0.0,
        yy: scale,
        x0: 0.0,
        y0: 0.0,
    }
}

impl EditorView {
    pub fn new(
        idle_handle: IdleHandle,
        window_handle: WindowHandle,
        core: Arc<Mutex<Core>>,
        view: Arc<Mutex<View>>,
        config: Config,
    ) -> EditorView {
        let id = Uuid::new_v4().to_string();
        EditorView {
            id,
            idle_handle,
            window_handle,
            core,
            view,
            config,
            input: Input::new(),
            width: 0.0,
            height: 0.0,
            vertical_scroll: 0.0,
            horizontal_scroll: 0.0,
            parent: None,
            needs_update: false,
        }
    }

    // fn set_active(&self) {
    //     self.view.lock().unwrap().set_editor_view_by_id(&self.id);
    // }

    // fn paint_scroll(&self, paint_ctx: &mut PaintCtx) {
    //     let scroll_thickness = 9.0;
    //     let view_width = self.view.lock().unwrap().width;
    //     let view_height = self.view.lock().unwrap().height;
    //     let fg = self.config.theme.lock().unwrap().foreground.unwrap();
    //     let color = Color::rgba8(fg.r, fg.g, fg.b, 40);
    //     if view_width > self.width {
    //         let width = self.width / view_width * self.width;
    //         let point = Point::new(
    //             self.horizontal_scroll / view_width * self.width,
    //             self.height - scroll_thickness,
    //         );
    //         let rect = Rect::from_origin_size(point, Size::new(width, scroll_thickness));
    //         paint_ctx.fill(rect, &color);
    //     }
    //     if view_height > self.height {
    //         let height = self.height / view_height * self.height;
    //         let point = Point::new(
    //             self.width - scroll_thickness,
    //             self.vertical_scroll / view_height * self.height,
    //         );
    //         let rect = Rect::from_origin_size(point, Size::new(scroll_thickness, height));
    //         paint_ctx.fill(rect, &color);
    //     }
    // }

    fn paint_line(&self, i: usize, paint_ctx: &mut PaintCtx, line: &Line) {
        let mut text = line.text().to_string();
        // text.pop();
        let text_len = count_utf16(&text);

        let app_font = self.config.font.lock().unwrap();

        let fg = self.config.theme.lock().unwrap().foreground.unwrap();
        let cursor_color = Color::rgba8(fg.r, fg.g, fg.b, 160);

        for cursor in line.cursor() {
            let point = Point::new(
                app_font.width * *cursor as f64,
                app_font.lineheight() * i as f64,
            );
            let rect = Rect::from_origin_size(
                point,
                Size::new(
                    match self.input.state {
                        InputState::Insert => 1.0,
                        InputState::Visual => 1.0,
                        InputState::Normal => app_font.width,
                    },
                    app_font.lineheight(),
                ),
            );
            paint_ctx.fill(rect, &cursor_color);
        }

        let font = paint_ctx
            .text()
            .new_font_by_name("Consolas", 13.0)
            .unwrap()
            .build()
            .unwrap();
        for style in line.styles() {
            let range = &style.range;
            let mut end = range.end;
            if end == text_len {
                if &text[end - 1..end] == "\n" {
                    end -= 1;
                }
            }
            let layout = paint_ctx
                .text()
                .new_text_layout(&font, &text[range.start..end])
                .unwrap()
                .build()
                .unwrap();
            let x = paint_ctx
                .text()
                .new_text_layout(&font, &text[..range.start])
                .unwrap()
                .build()
                .unwrap()
                .width();
            let fg_color = self
                .config
                .styles
                .lock()
                .unwrap()
                .get(&style.style_id)
                .unwrap()
                .fg_color
                .unwrap();
            paint_ctx.draw_text(
                &layout,
                Point::new(
                    x,
                    app_font.lineheight() * i as f64 + app_font.ascent + app_font.linespace / 2.0,
                ),
                &Color::from_rgba32_u32(fg_color),
            );
        }
    }

    fn send_scroll(&self) {
        let view_id = self.view.lock().unwrap().id.clone();
        let lineheight = self.config.font.lock().unwrap().lineheight();
        let start = (self.vertical_scroll / lineheight) as usize;
        let line_count = (self.height / lineheight) as usize + 2;
        let core = self.core.clone();
        thread::spawn(move || {
            core.lock().unwrap().send_notification(
                "edit",
                &json!({
                    "view_id": view_id,
                    "method": "scroll",
                    "params": [start, start+line_count],
                }),
            );
        });
    }

    // fn scroll(&mut self, delta: Vec2) {
    //     if delta.x == 0.0 && delta.y == 0.0 {
    //         return;
    //     }
    //     self.needs_update = true;

    //     self.send_scroll();

    //     let view_width = self.view.lock().unwrap().width;
    //     let view_height = self.view.lock().unwrap().height;

    //     self.horizontal_scroll += delta.x;
    //     if self.horizontal_scroll > view_width - self.width {
    //         self.horizontal_scroll = view_width - self.width;
    //     }
    //     if self.horizontal_scroll < 0.0 {
    //         self.horizontal_scroll = 0.0;
    //     }

    //     self.vertical_scroll += delta.y;
    //     if self.vertical_scroll > view_height - self.height {
    //         self.vertical_scroll = view_height - self.height;
    //     }
    //     if self.vertical_scroll < 0.0 {
    //         self.vertical_scroll = 0.0;
    //     }

    //     let window_handle = self.window_handle.clone();
    //     let width = self.width;
    //     let height = self.height;
    //     self.idle_handle.add_idle(move |_| {
    //         window_handle.invalidate_rect(Rect::from_origin_size(
    //             Point::new(0.0, 0.0),
    //             Size::new(width, height),
    //         ));
    //     });
    // }

    // fn run(&mut self, cmd: &Command, key_input: KeyInput) {
    //     let mut count = self.input.count;
    //     if count == 0 {
    //         count = 1;
    //     }

    //     match *cmd {
    //         Command::Insert => self.input.state = InputState::Insert,
    //         Command::Escape => {
    //             self.input.state = InputState::Nomral;
    //             self.input.count = 0;
    //         }
    //         Command::DeleteBackward => {
    //             self.core.lock().unwrap().send_notification(
    //                 "edit",
    //                 &json!({
    //                     "view_id": self.view.lock().unwrap().id.clone(),
    //                     "method": "delete_backward",
    //                 }),
    //             );
    //         }
    //         Command::MoveUp => {
    //             self.core.lock().unwrap().send_notification(
    //                 "edit",
    //                 &json!({
    //                     "view_id": self.view.lock().unwrap().id.clone(),
    //                     "method": "move_up",
    //                     "params": {"count": count},
    //                 }),
    //             );
    //         }
    //         Command::MoveDown => {
    //             self.core.lock().unwrap().send_notification(
    //                 "edit",
    //                 &json!({
    //                     "view_id": self.view.lock().unwrap().id.clone(),
    //                     "method": "move_down",
    //                     "params": {"count": count},
    //                 }),
    //             );
    //         }
    //         Command::MoveLeft => {
    //             self.core.lock().unwrap().send_notification(
    //                 "edit",
    //                 &json!({
    //                     "view_id": self.view.lock().unwrap().id.clone(),
    //                     "method": "move_left",
    //                     "params": {"count": count},
    //                 }),
    //             );
    //         }
    //         Command::MoveRight => {
    //             self.core.lock().unwrap().send_notification(
    //                 "edit",
    //                 &json!({
    //                     "view_id": self.view.lock().unwrap().id.clone(),
    //                     "method": "move_right",
    //                     "params": {"count": count},
    //                 }),
    //             );
    //         }
    //         Command::SplitHorizontal => {}
    //         Command::SplitVertical => {
    //             // let new_editor_view = Arc::new(Mutex::new(Box::new(EditorView::new(
    //             //     self.idle_handle.clone(),
    //             //     self.window_handle.clone(),
    //             //     self.core.clone(),
    //             //     self.view.clone(),
    //             //     self.config.clone(),
    //             // ))
    //             //     as Box<Widget + Send + Sync>));

    //             // let new_editor_view_id = new_editor_view.lock().unwrap().id();

    //             // self.view
    //             //     .lock()
    //             //     .unwrap()
    //             //     .editor_views
    //             //     .lock()
    //             //     .unwrap()
    //             //     .insert(new_editor_view_id, new_editor_view.clone());

    //             // let parent = self.parent.clone().unwrap().clone();

    //             // thread::spawn(move || {
    //             //     parent.lock().unwrap().add_child(new_editor_view.clone());
    //             //     new_editor_view.lock().unwrap().set_parent(parent);
    //             // });
    //         }
    //         Command::Unknown => {
    //             if self.input.state == InputState::Insert
    //                 && key_input.text != ""
    //                 && !key_input.mods.ctrl
    //                 && !key_input.mods.alt
    //                 && !key_input.mods.meta
    //             {
    //                 let chars = match key_input.key_code {
    //                     KeyCode::Return => "\n".to_string(),
    //                     _ => key_input.text.clone(),
    //                 };
    //                 self.core.lock().unwrap().send_notification(
    //                     "edit",
    //                     &json!({
    //                         "view_id": self.view.lock().unwrap().id.clone(),
    //                         "method": "insert",
    //                         "params": {"chars": chars},
    //                     }),
    //                 );
    //             }
    //         }
    //     }

    //     if self.input.state == InputState::Nomral {
    //         match *cmd {
    //             Command::Unknown => {
    //                 if let Ok(n) = key_input.text.parse::<u64>() {
    //                     self.input.count = self.input.count * 10 + n;
    //                 } else {
    //                     self.input.count = 0;
    //                 }
    //             }
    //             _ => self.input.count = 0,
    //         };
    //     }
    // }
}

// impl Widget for EditorView {
//     fn id(&self) -> String {
//         self.id.clone()
//     }

//     fn paint(&mut self, paint_ctx: &mut PaintCtx) {
//         if !self.needs_update {
//             return;
//         }
//         {
//             let theme = self.config.theme.lock().unwrap();
//             let bg = theme.background.unwrap();
//             let rect = Rect::from_origin_size(Point::ORIGIN, Size::new(self.width, self.height));
//             paint_ctx.fill(rect, &Color::rgba8(bg.r, bg.g, bg.b, bg.a));
//         }

//         self.paint_scroll(paint_ctx);

//         paint_ctx.save();
//         paint_ctx.clip(Rect::from_origin_size(
//             Point::ORIGIN,
//             Size::new(self.width, self.height),
//         ));
//         paint_ctx.transform(Affine::translate(Vec2::new(
//             -self.horizontal_scroll,
//             -self.vertical_scroll,
//         )));
//         let font = paint_ctx
//             .text()
//             .new_font_by_name("Consolas", 13.0)
//             .unwrap()
//             .build()
//             .unwrap();
//         let line_cache = &self.view.lock().unwrap().line_cache;
//         let lineheight = self.config.font.lock().unwrap().lineheight();

//         let start = (self.vertical_scroll / lineheight) as usize;
//         let line_count = (self.height / lineheight) as usize + 2;
//         for i in start..start + line_count {
//             if let Some(line) = line_cache.get_line(i) {
//                 self.paint_line(i, paint_ctx, line);
//             }
//         }
//         paint_ctx.restore();
//         self.needs_update = false;
//     }

//     fn layout(&mut self, bc: &BoxConstraints) -> Size {
//         bc.max()
//     }

//     fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
//         self.set_active();
//         println!("mouse down {:?}", event.pos);
//         let font = self.config.font.lock().unwrap();

//         let line = ((event.pos.y as f64 + self.vertical_scroll) / font.lineheight()) as u32;
//         let column = ((event.pos.x as f64 + self.horizontal_scroll) / font.width) as u32;
//         let core = self.core.clone();
//         let view_id = self.view.lock().unwrap().id.clone();
//         core.lock().unwrap().send_notification(
//             "edit",
//             &json!({
//                 "view_id": view_id,
//                 "method": "gesture",
//                 "params": {
//                     "line":line,
//                     "col":column,
//                     "ty": "point_select"
//                 },
//             }),
//         );
//     }

//     fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
//         let config = self.config.clone();
//         let keymaps = config.keymaps.lock().unwrap();
//         let key_input = KeyInput::from_keyevent(&event, self.input.state.clone());
//         let cmd = keymaps
//             .get(&key_input.get_key())
//             .unwrap_or(&Command::Unknown);
//         self.run(cmd, key_input);

//         false
//     }

//     fn size(&mut self, width: f64, height: f64) {
//         println!("set editor size {}, {}", width, height);
//         self.width = width;
//         self.height = height;
//         self.send_scroll();
//     }

//     fn mouse_move(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

//     fn wheel(&mut self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
//         self.scroll(delta);
//     }

//     fn ensure_visble(&mut self, rect: Rect, margin_x: f64, margin_y: f64) {
//         let mut scroll_x = 0.0;
//         let mut scroll_y = 0.0;
//         let right_limit = self.width + self.horizontal_scroll - margin_x;
//         let left_limit = self.horizontal_scroll + margin_x;
//         if rect.x1 > right_limit {
//             scroll_x = rect.x1 - right_limit;
//         } else if rect.x0 < left_limit {
//             scroll_x = rect.x0 - left_limit;
//         }

//         let bottom_limit = self.height + self.vertical_scroll - margin_y;
//         let top_limit = self.vertical_scroll + margin_y;
//         if rect.y1 > bottom_limit {
//             scroll_y = rect.y1 - bottom_limit;
//         } else if rect.y0 < top_limit {
//             scroll_y = rect.y0 - top_limit;
//         }

//         self.scroll(Vec2::new(scroll_x, scroll_y));
//     }

//     fn add_child(&mut self, widget: Arc<Mutex<Box<Widget + Send + Sync>>>) {}

//     fn set_parent(&mut self, widget: Arc<Mutex<Box<Widget + Send + Sync>>>) {
//         self.parent = Some(widget);
//     }
// }
