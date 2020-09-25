use crate::app::{App, CommandRunner};
use crate::config::AppFont;
use crate::config::Config;
use crate::input::{Cmd, Command, Input, InputState, KeyInput};
use crate::line_cache::{count_utf16, Annotation, Line, LineCache, Style};
use crate::popup::Popup;
use crate::rpc::Core;
use cairo::{FontFace, FontOptions, FontSlant, FontWeight, Matrix, ScaledFont};
use druid::shell::keyboard::{KeyCode, KeyEvent, KeyModifiers};
use druid::shell::platform::IdleHandle;
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::shell::{kurbo, piet, runloop, WindowBuilder};
use druid::{PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{
    Color, FontBuilder, LinearGradient, Piet, RenderContext, Text, TextLayout,
    TextLayoutBuilder, UnitPoint,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use uuid::Uuid;
use xi_core_lib::word_boundaries::{get_word_property, WordProperty};

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
    gutter_padding: f64,
    col: usize,
    line: usize,
}

#[derive(WidgetBase, Clone)]
pub struct Editor {
    widget_state: Arc<Mutex<WidgetState>>,
    state: Arc<Mutex<EditorState>>,
    app: App,
}

impl Editor {
    pub fn new(app: App) -> Editor {
        Editor {
            widget_state: Arc::new(Mutex::new(WidgetState::new())),
            state: Arc::new(Mutex::new(EditorState {
                view: None,
                input: Input::new(),
                gutter_padding: 10.0,
                col: 0,
                line: 0,
            })),
            app,
        }
    }

    pub fn load_view(&self, view: View) {
        self.state.lock().unwrap().view = Some(view);
    }

    pub fn view(&self) -> View {
        self.state.lock().unwrap().view.clone().unwrap()
    }

    pub fn line(&self) -> usize {
        self.state.lock().unwrap().line
    }

    pub fn col(&self) -> usize {
        self.state.lock().unwrap().col
    }

    pub fn get_completion_pos(&self) -> (usize, usize, String) {
        let view = self.state.lock().unwrap().view.clone();
        match view {
            Some(view) => {
                match view.line_cache.lock().unwrap().get_line(self.line()) {
                    Some(line) => {
                        let col = self.col();
                        let chars: Vec<char> =
                            line.text()[..col].to_string().chars().collect();
                        if chars.len() == 0 {
                            (0, 0, "".to_string())
                        } else {
                            let mut i = chars.len() - 1;
                            while let Some(ch) = chars.get(i) {
                                if get_word_property(ch.clone())
                                    != WordProperty::Other
                                {
                                    break;
                                }
                                if i == 0 {
                                    break;
                                }
                                i -= 1;
                            }
                            (
                                i + 1,
                                self.line(),
                                line.text()[i + 1..self.col()].to_string(),
                            )
                        }
                    }
                    None => (0, 0, "".to_string()),
                }
            }
            None => (0, 0, "".to_string()),
        }
    }

    fn get_view_width(&self) -> f64 {
        let view = self.state.lock().unwrap().view.clone();
        match view {
            Some(view) => view.state.lock().unwrap().width,
            None => 0.0,
        }
    }

    fn get_view_height(&self) -> f64 {
        let view = self.state.lock().unwrap().view.clone();
        match view {
            Some(view) => view.state.lock().unwrap().height,
            None => 0.0,
        }
    }

    fn gutter_len(&self) -> usize {
        let view = self.state.lock().unwrap().view.clone();
        match view {
            Some(view) => {
                format!("{}", view.line_cache.lock().unwrap().height()).len()
            }
            None => 0,
        }
    }

    fn gutter_width(&self) -> f64 {
        let app_font = self.app.config.font.lock().unwrap().width;
        let gutter_padding = self.state.lock().unwrap().gutter_padding;
        2.0 * gutter_padding + app_font * self.gutter_len() as f64
    }

    pub fn move_popup(&self) {
        let popup = self
            .app
            .state
            .lock()
            .unwrap()
            .popup
            .clone()
            .unwrap()
            .clone();
        let rect = self.get_rect();
        let col = popup.col();
        let line = popup.line();
        let horizontal_scroll = self.horizontal_scroll();
        let vertical_scroll = self.vertical_scroll();
        let gutter_width = self.gutter_width();
        let app_font = self.app.config.font.lock().unwrap().clone();
        let x = rect.x0 + gutter_width + col as f64 * app_font.width
            - horizontal_scroll;
        let y = rect.y0 + (line + 1) as f64 * app_font.lineheight()
            - vertical_scroll;
        popup.set_pos(x, y);
    }

    fn layout(&self) {}

    fn paint(&self, paint_ctx: &mut PaintCtx) {
        let view = self.state.lock().unwrap().view.clone();
        if view.is_none() {
            return;
        }
        let current_line = self.state.lock().unwrap().line;
        let is_active =
            self.app.state.lock().unwrap().active_editor.clone() == self.id();
        let bg = self.app.config.theme.lock().unwrap().background.unwrap();
        let rect = Rect::from_origin_size(
            Point::ORIGIN,
            self.widget_state.lock().unwrap().size(),
        );
        paint_ctx.fill(rect, &Color::rgba8(bg.r, bg.g, bg.b, bg.a));

        let line_cache = self
            .state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .line_cache
            .clone();

        let horizontal_scroll =
            self.widget_state.lock().unwrap().horizontal_scroll();
        let vertical_scroll =
            self.widget_state.lock().unwrap().vertical_scroll();
        let width = self.widget_state.lock().unwrap().size().width;
        let height = self.widget_state.lock().unwrap().size().height;

        let lineheight = self.app.config.font.lock().unwrap().lineheight();
        let start = (vertical_scroll / lineheight) as usize;
        let line_count = (height / lineheight) as usize + 2;
        let total_lines = line_cache.lock().unwrap().height();
        let end = if start + line_count > total_lines {
            total_lines
        } else {
            start + line_count
        };

        let gutter_width = self.paint_gutter(paint_ctx, start, end);

        self.paint_scroll(paint_ctx);
        paint_ctx.save();
        paint_ctx.clip(Rect::from_origin_size(
            Point::new(gutter_width, 0.0),
            Size::new(width - gutter_width, height),
        ));
        paint_ctx.transform(Affine::translate(Vec2::new(
            -horizontal_scroll + gutter_width,
            -vertical_scroll,
        )));

        let annotations = line_cache.lock().unwrap().annotations();
        for i in start..end {
            if let Some(line) = line_cache.lock().unwrap().get_line(i).clone() {
                self.paint_line(
                    i,
                    paint_ctx,
                    line,
                    &annotations,
                    current_line == i,
                    is_active,
                );
            }
        }
        paint_ctx.restore();
    }

    fn paint_gutter(
        &self,
        paint_ctx: &mut PaintCtx,
        start: usize,
        end: usize,
    ) -> f64 {
        let vertical_scroll =
            self.widget_state.lock().unwrap().vertical_scroll();
        let app_font = self.app.config.font.lock().unwrap();
        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
        let fg_color = Color::rgba8(fg.r, fg.g, fg.b, 100);
        let gutter_len = self.gutter_len();
        let gutter_padding = self.state.lock().unwrap().gutter_padding;
        let font = paint_ctx
            .text()
            .new_font_by_name("Cascadia Code", 13.0)
            .unwrap()
            .build()
            .unwrap();
        for i in start..end {
            let text = format!("{}", i);
            let layout = paint_ctx
                .text()
                .new_text_layout(&font, &format!("{}", i))
                .unwrap()
                .build()
                .unwrap();
            let x = gutter_padding
                + app_font.width
                    * (if gutter_len >= text.len() {
                        gutter_len - text.len()
                    } else {
                        0
                    }) as f64;
            paint_ctx.draw_text(
                &layout,
                Point::new(
                    x,
                    app_font.lineheight() * i as f64
                        + app_font.ascent
                        + app_font.linespace / 2.0
                        - vertical_scroll,
                ),
                &fg_color,
            );
        }
        2.0 * gutter_padding + app_font.width * gutter_len as f64
    }

    fn paint_line(
        &self,
        i: usize,
        paint_ctx: &mut PaintCtx,
        line: &Line,
        annotations: &Vec<Annotation>,
        is_current: bool,
        is_active: bool,
    ) {
        let mut text = line.text().to_string();
        // text.pop();
        let text_len = count_utf16(&text);

        let app_font = self.app.config.font.lock().unwrap();

        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();

        let input_state = self.state.lock().unwrap().input.state.clone();

        let visual_line = self.state.lock().unwrap().input.visual_line.clone();

        let width = self.widget_state.lock().unwrap().size().width;

        let view_width = self.get_view_width();

        for annotation in annotations {
            if let Some((start, end)) = annotation.check_line(i, line) {
                if input_state == InputState::Visual {
                    let point = if visual_line {
                        Point::new(0.0, app_font.lineheight() * i as f64)
                    } else {
                        Point::new(
                            app_font.width * start as f64,
                            app_font.lineheight() * i as f64,
                        )
                    };
                    let size = if visual_line {
                        Size::new(
                            app_font.width * text_len as f64,
                            app_font.lineheight(),
                        )
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
        }

        if input_state != InputState::Visual && is_current {
            let point = Point::new(0.0, app_font.lineheight() * i as f64);
            let size = Size::new(
                if view_width > width {
                    view_width
                } else {
                    width
                },
                app_font.lineheight(),
            );
            let rect = Rect::from_origin_size(point, size);
            let current_line_color = Color::rgba8(fg.r, fg.g, fg.b, 20);
            paint_ctx.fill(rect, &current_line_color);
        }

        if is_active {
            let cursor_color = Color::rgba8(fg.r, fg.g, fg.b, 160);
            for cursor in line.cursor() {
                let point = Point::new(
                    match input_state {
                        InputState::Insert => {
                            if *cursor == 0 {
                                0.0
                            } else {
                                app_font.width * *cursor as f64 - 1.0
                            }
                        }
                        _ => app_font.width * *cursor as f64,
                    },
                    app_font.lineheight() * i as f64,
                );
                let rect = Rect::from_origin_size(
                    point,
                    Size::new(
                        match input_state {
                            InputState::Insert => 2.0,
                            _ => app_font.width,
                        },
                        app_font.lineheight(),
                    ),
                );
                paint_ctx.fill(rect, &cursor_color);
            }
        }

        let font = paint_ctx
            .text()
            .new_font_by_name("Cascadia Code", 13.0)
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
            if let Some(style) =
                self.app.config.styles.lock().unwrap().get(&style.style_id)
            {
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
        let gutter_width = self.gutter_width();
        let scroll_thickness = 9.0;
        let view_width = self.get_view_width();
        let view_height = self.get_view_height();
        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();
        let color = Color::rgba8(fg.r, fg.g, fg.b, 40);
        let size = self.widget_state.lock().unwrap().size();
        let width = size.width - gutter_width;
        let height = size.height;
        let horizontal_scroll =
            self.widget_state.lock().unwrap().horizontal_scroll();
        let vertical_scroll =
            self.widget_state.lock().unwrap().vertical_scroll();
        if view_width > width {
            let point = Point::new(
                gutter_width + horizontal_scroll / view_width * width,
                height - scroll_thickness,
            );
            let rect = Rect::from_origin_size(
                point,
                Size::new(width / view_width * width, scroll_thickness),
            );
            paint_ctx.fill(rect, &color);
        }
        if view_height > height {
            let point = Point::new(
                width + gutter_width - scroll_thickness,
                vertical_scroll / view_height * height,
            );
            let rect = Rect::from_origin_size(
                point,
                Size::new(scroll_thickness, height / view_height * height),
            );
            paint_ctx.fill(rect, &color);
        }
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        let gutter_width = self.gutter_width();
        self.app.set_active_editor(&self);
        if self.state.lock().unwrap().view.is_none() {
            return;
        }

        let font = self.app.config.font.lock().unwrap();
        let view_id = self
            .state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .id()
            .clone();

        let horizontal_scroll =
            self.widget_state.lock().unwrap().horizontal_scroll();
        let vertical_scroll =
            self.widget_state.lock().unwrap().vertical_scroll();
        let line =
            ((event.pos.y as f64 + vertical_scroll) / font.lineheight()) as u32;
        let col = ((event.pos.x as f64 + horizontal_scroll - gutter_width)
            / font.width) as u32;
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

    fn set_cursor(&self, col: usize, line: usize) {
        self.state.lock().unwrap().col = col;
        self.state.lock().unwrap().line = line;
    }

    fn ensure_visble(&self, rect: Rect, margin_x: f64, margin_y: f64) {
        let mut scroll_x = 0.0;
        let mut scroll_y = 0.0;
        let gutter_width = self.gutter_width();
        let size = self.widget_state.lock().unwrap().size();
        let horizontal_scroll =
            self.widget_state.lock().unwrap().horizontal_scroll();
        let vertical_scroll =
            self.widget_state.lock().unwrap().vertical_scroll();
        let right_limit =
            size.width - gutter_width + horizontal_scroll - margin_x;
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
        let size = self.widget_state.lock().unwrap().size();
        let width = size.width - self.gutter_width();
        let height = size.height;

        let mut horizontal_scroll =
            self.widget_state.lock().unwrap().horizontal_scroll();
        let mut vertical_scroll =
            self.widget_state.lock().unwrap().vertical_scroll();
        horizontal_scroll += delta.x;
        if horizontal_scroll > view_width - width {
            horizontal_scroll = view_width - width;
        }
        if horizontal_scroll < 0.0 {
            horizontal_scroll = 0.0;
        }

        vertical_scroll += delta.y;
        if vertical_scroll > view_height - height {
            vertical_scroll = view_height - height;
        }
        if vertical_scroll < 0.0 {
            vertical_scroll = 0.0;
        }
        self.widget_state
            .lock()
            .unwrap()
            .set_scroll(horizontal_scroll, vertical_scroll);
        self.invalidate();
    }

    fn exchange(&self) {
        let parent = self.widget_state.lock().unwrap().parent().unwrap();
        let id = self.id();
        let ids = parent.child_ids();
        if ids.len() <= 1 {
            return;
        }
        let index = ids.iter().position(|c| c == &id).unwrap();
        let new_index = match index {
            i if i < ids.len() - 1 => i + 1,
            i => i - 1,
        };
        let new_id = ids.get(new_index).unwrap();
        let editor = self.clone();
        let new_editor = self
            .app
            .editors
            .lock()
            .unwrap()
            .get(new_id.as_str())
            .unwrap()
            .clone();
        parent.replace_child(index, Box::new(new_editor.clone()));
        parent.replace_child(new_index, Box::new(editor.clone()));
        new_editor.set_active();
        self.app.state.lock().unwrap().active_editor = new_editor.id();

        let view_id = new_editor
            .state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .id()
            .clone();
        let col = new_editor.state.lock().unwrap().col;
        let line = new_editor.state.lock().unwrap().line;
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

        self.app.main_flex.invalidate();
    }

    fn move_curosr(&self, vertical: i64) {
        let parent = self.parent().unwrap();
        let id = self.id();
        let ids = parent.child_ids();
        let index = ids.iter().position(|c| c == &id).unwrap();
        let new_index = match index as i64 + vertical {
            i if i < 0 => 0,
            i if i >= ids.len() as i64 => ids.len() - 1,
            i => i as usize,
        };
        if new_index == index {
            return;
        }
        let new_id = ids.get(new_index).unwrap();
        let new_editor = self
            .app
            .editors
            .lock()
            .unwrap()
            .get(new_id.as_str())
            .unwrap()
            .clone();
        new_editor.set_active();
        self.app.state.lock().unwrap().active_editor = new_editor.id();

        let view_id = new_editor
            .state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .id()
            .clone();
        let col = new_editor.state.lock().unwrap().col;
        let line = new_editor.state.lock().unwrap().line;
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

        self.invalidate();
        new_editor.invalidate();
    }

    pub fn load_file(&self, file_path: String) {
        if let Some(view) =
            self.app.path_views.lock().unwrap().get(&file_path).clone()
        {
            self.load_view(view.to_owned());
            self.invalidate();
            return;
        }
        let params = json!({
            "file_path": file_path.clone(),
        });
        let editor = self.clone();
        self.app
            .core
            .send_request("new_view", &params, move |value| {
                let view_id = value.as_str().unwrap().to_string();
                let view = View::new(view_id.clone(), editor.app.clone());
                editor
                    .app
                    .views
                    .lock()
                    .unwrap()
                    .insert(view_id.clone(), view.clone());
                editor
                    .app
                    .path_views
                    .lock()
                    .unwrap()
                    .insert(file_path.clone(), view.clone());
                editor.load_view(view);
                editor.invalidate();
            });
    }

    fn send_scroll(&self) {
        let view = self.state.lock().unwrap().view.clone();
        if view.is_none() {
            return;
        }
        let view_id = view.as_ref().unwrap().id().clone();
        let lineheight = self.app.config.font.lock().unwrap().lineheight();
        let horizontal_scroll =
            self.widget_state.lock().unwrap().horizontal_scroll();
        let vertical_scroll =
            self.widget_state.lock().unwrap().vertical_scroll();
        let size = self.widget_state.lock().unwrap().size();
        let start = match (vertical_scroll / lineheight) as usize {
            s if s > 0 => s - 1,
            0 => 0,
            _ => 0,
        };
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

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        let app = self.app.clone();
        let key_input = KeyInput::from_keyevent(&event);
        thread::spawn(move || {
            app.handle_key_down(key_input);
        });
        true
    }

    pub fn get_state(&self) -> InputState {
        self.state.lock().unwrap().input.state.clone()
    }
}

impl CommandRunner for Editor {
    fn run(&self, cmd: Cmd, key_input: KeyInput) {
        if self.state.lock().unwrap().view.is_none() {
            return;
        }
        let view_id = self
            .state
            .lock()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .id()
            .clone();

        let mut count = self.state.lock().unwrap().input.count;
        if count == 0 {
            count = 1;
        }

        let input_state = self.state.lock().unwrap().input.state.clone();
        let line_selection =
            self.state.lock().unwrap().input.visual_line.clone();

        let caret = match input_state {
            InputState::Insert => true,
            _ => false,
        };
        let is_selection = match input_state {
            InputState::Visual => true,
            _ => false,
        };

        match cmd.clone().cmd.unwrap() {
            Command::CommandPalette => {
                let palette =
                    self.app.state.lock().unwrap().palette.clone().unwrap();
                palette.run();
            }
            Command::Insert => {
                self.state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.no_move(&view_id, true, true, true);
                self.app.get_active_editor().invalidate();
            }
            Command::Visual => {
                self.state.lock().unwrap().input.visual_line = false;
                if input_state == InputState::Visual {
                    if line_selection {
                        self.app.core.no_move(&view_id, true, false, false);
                    } else {
                        self.state.lock().unwrap().input.state =
                            InputState::Normal;
                        self.app.core.no_move(&view_id, false, false, false);
                    }
                } else {
                    self.state.lock().unwrap().input.state = InputState::Visual;
                    self.app.core.no_move(&view_id, true, false, false);
                }
                self.app.get_active_editor().invalidate();
            }
            Command::VisualLine => {
                if input_state == InputState::Visual {
                    if !line_selection {
                        self.state.lock().unwrap().input.visual_line = true;
                        self.app.core.no_move(&view_id, true, true, false);
                    } else {
                        self.state.lock().unwrap().input.visual_line = false;
                        self.state.lock().unwrap().input.state =
                            InputState::Normal;
                        self.app.core.no_move(&view_id, false, false, false);
                    }
                } else {
                    self.state.lock().unwrap().input.state = InputState::Visual;
                    self.state.lock().unwrap().input.visual_line = true;
                    self.app.core.no_move(&view_id, true, true, false);
                }
                self.app.get_active_editor().invalidate();
            }
            Command::Escape => {
                self.state.lock().unwrap().input.state = InputState::Normal;
                self.state.lock().unwrap().input.count = 0;
                self.state.lock().unwrap().input.visual_line = false;
                self.app.core.no_move(&view_id, false, false, false);
                self.app.state.lock().unwrap().popup.clone().unwrap().hide();
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
                self.state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "insert_newline_below",
                    }),
                );
            }
            Command::NewLineAbove => {
                self.state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "insert_newline_above",
                    }),
                );
            }
            Command::InsertNewLine => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "insert_newline",
                    }),
                );
            }
            Command::InsertTab => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "insert_tab",
                    }),
                );
            }
            Command::MoveStartOfLine => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_left_end_of_line",
                    }),
                );
            }
            Command::MoveEndOfLine => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_right_end_of_line",
                    }),
                );
            }
            Command::InsertStartOfLine => {
                self.state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_left_end_of_line",
                    }),
                );
            }
            Command::AppendRight => {
                self.state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_right",
                        "params": {"count": count, "is_selection": false, "line_selection": false, "caret": true},
                    }),
                );
            }
            Command::AppendEndOfLine => {
                self.state.lock().unwrap().input.state = InputState::Insert;
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_right_end_of_line",
                    }),
                );
            }
            Command::DeleteForwardInsert => {
                self.state.lock().unwrap().input.state = InputState::Insert;
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
                    self.state.lock().unwrap().input.state = InputState::Normal;
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
                    self.state.lock().unwrap().input.state = InputState::Normal;
                }
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "delete_backward",
                    }),
                );
                if input_state == InputState::Insert {
                    let popup = self
                        .app
                        .state
                        .lock()
                        .unwrap()
                        .popup
                        .clone()
                        .unwrap()
                        .clone();
                    let (_col, _line, filter) = self.get_completion_pos();
                    if !popup.is_hidden() {
                        if filter == "" {
                            popup.hide();
                        } else {
                            popup.filter_items(
                                filter[..filter.len() - 1].to_string(),
                            );
                        }
                        popup.invalidate();
                    } else {
                        if filter != "" {
                            self.app.core.send_notification(
                                "edit",
                                &json!({
                                    "view_id": view_id,
                                    "method": "request_completion",
                                    "params": {"request_id": 123},
                                }),
                            );
                        }
                    }
                }
            }
            Command::DeleteWordBackward => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "delete_word_backward",
                    }),
                );
            }
            Command::DeleteToBeginningOfLine => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "delete_to_beginning_of_line",
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
            Command::MoveToTop => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_beginning_of_document",
                    }),
                );
            }
            Command::MoveToBottom => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "move_to_end_of_document",
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
            Command::Hover => {
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view_id,
                        "method": "request_hover",
                        "params": {"request_id": 123},
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
            Command::MoveCursorToWindowAbove => {}
            Command::MoveCursorToWindowBelow => {}
            Command::MoveCursorToWindowLeft => {
                let editor = self.clone();
                thread::spawn(move || {
                    editor.move_curosr(-1);
                });
            }
            Command::MoveCursorToWindowRight => {
                let editor = self.clone();
                thread::spawn(move || {
                    editor.move_curosr(1);
                });
            }
            Command::ExchangeWindow => {
                let editor = self.clone();
                thread::spawn(move || {
                    editor.exchange();
                });
            }
            Command::SplitHorizontal => {}
            Command::SplitClose => {
                println!("now remove child {}", self.id());
                let parent = self.parent().unwrap();
                let id = self.id();
                let ids = parent.child_ids();
                if ids.len() == 1 {
                    return;
                }
                let index = ids.iter().position(|c| c == &id).unwrap();
                match index {
                    i if i + 1 < ids.len() => self.move_curosr(1),
                    i => self.move_curosr(-1),
                };
                self.app.main_flex.remove_child(self.id());
                self.app.main_flex.invalidate();
            }
            Command::SplitVertical => {
                let view =
                    self.state.lock().unwrap().view.as_ref().unwrap().clone();
                let editor = Editor::new(self.app.clone());
                self.app
                    .editors
                    .lock()
                    .unwrap()
                    .insert(editor.id().clone(), editor.clone());
                editor.state.lock().unwrap().col = self.col();
                editor.state.lock().unwrap().line = self.line();
                editor.set_scroll(
                    self.horizontal_scroll(),
                    self.vertical_scroll(),
                );
                editor.load_view(view);
                self.app.main_flex.add_child(Box::new(editor));
                self.app.main_flex.invalidate();
            }
            Command::Unknown => {
                if input_state == InputState::Insert
                    && key_input.text != ""
                    && !key_input.mods.ctrl
                    && !key_input.mods.alt
                    && !key_input.mods.meta
                {
                    self.app.core.send_notification(
                        "edit",
                        &json!({
                            "view_id": view_id,
                            "method": "insert",
                            "params": {"chars": key_input.text},
                        }),
                    );
                    let popup = self
                        .app
                        .state
                        .lock()
                        .unwrap()
                        .popup
                        .clone()
                        .unwrap()
                        .clone();
                    if key_input.text.len() == 1 {
                        let ch = key_input.text.chars().next().unwrap();
                        let prop = get_word_property(ch);
                        match prop {
                            WordProperty::Other => {
                                if popup.is_hidden() {
                                    if get_word_property(ch)
                                        == WordProperty::Other
                                    {
                                        self.app.core.send_notification(
                                            "edit",
                                            &json!({
                                                "view_id": view_id,
                                                "method": "request_completion",
                                                "params": {"request_id": 123},
                                            }),
                                        );
                                    }
                                } else {
                                    let (_col, _line, filter) =
                                        self.get_completion_pos();
                                    popup.filter_items(format!(
                                        "{}{}",
                                        filter, key_input.text
                                    ));
                                    popup.invalidate();
                                }
                            }
                            _ => {
                                popup.hide();
                                popup.invalidate();
                            }
                        };
                    }
                }
            }
            _ => {}
        }

        if input_state == InputState::Normal {
            match cmd.cmd.unwrap() {
                Command::Unknown => {
                    if let Ok(n) = key_input.text.parse::<u64>() {
                        let count = self.state.lock().unwrap().input.count;
                        self.state.lock().unwrap().input.count = count * 10 + n;
                    } else {
                        self.state.lock().unwrap().input.count = 0;
                    }
                }
                _ => self.state.lock().unwrap().input.count = 0,
            };
        }
    }
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
        let (start, end) = self.line_cache.lock().unwrap().apply_update(update);
        let lineheight = self.app.config.font.lock().unwrap().lineheight();
        let font_width = self.app.config.font.lock().unwrap().width;
        self.state.lock().unwrap().height =
            self.line_cache.lock().unwrap().height() as f64 * lineheight;
        let mut width = 0.0;
        let line_count = self.line_cache.lock().unwrap().height();
        for i in 0..line_count {
            if let Some(line) = self.line_cache.lock().unwrap().get_line(i) {
                let current_width =
                    count_utf16(line.text()) as f64 * font_width;
                if current_width > width {
                    width = current_width;
                }
            }
        }
        self.state.lock().unwrap().width = width;
        // println!("finish apply update");

        for (_, editor) in self.app.editors.lock().unwrap().iter() {
            let view = editor.state.lock().unwrap().view.clone();
            match view {
                Some(view) => {
                    if view.id() == self.id {
                        let old_ln = editor.state.lock().unwrap().line;
                        match view
                            .line_cache
                            .lock()
                            .unwrap()
                            .get_old_line(old_ln)
                            .clone()
                        {
                            Some(old_line) => {
                                editor.state.lock().unwrap().line =
                                    old_line.new_ln();
                            }
                            None => (),
                        }
                        let gutter_width = editor.gutter_width();
                        let vertical_scroll = editor
                            .widget_state
                            .lock()
                            .unwrap()
                            .vertical_scroll();
                        let x = editor.gutter_width();
                        let y = start as f64 * lineheight - vertical_scroll;

                        let width =
                            editor.widget_state.lock().unwrap().size().width
                                - gutter_width;
                        let height = (end - start + 1) as f64 * lineheight;

                        let rect = Rect::from_origin_size(
                            Point::new(x, y),
                            Size::new(width, height),
                        );
                        editor.invalidate_rect(rect);
                    }
                }
                None => (),
            }
        }

        // if let Some(editor) = self.get_active_editor() {
        //     editor.invalidate();
        // }
    }

    fn get_active_editor(&self) -> Option<Editor> {
        let editor = self.app.get_active_editor();
        if editor.state.lock().unwrap().view.clone().unwrap().id == self.id {
            return Some(editor);
        }
        None
    }

    pub fn scroll_to(&self, col: usize, line: usize) {
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
                editor.set_cursor(col, line);
                editor.ensure_visble(rect, font_width, line_height);
            });
        }
    }
}
