use crate::config::AppFont;
use crate::config::Config;
use crate::input::{Command, Input, InputState, KeyInput};
use crate::line_cache::{Line, LineCache, Style};
use crate::rpc::Core;
use crate::ui::widget::Widget;
use cairo::{FontFace, FontOptions, FontSlant, FontWeight, Matrix, ScaledFont};
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::shell::{kurbo, piet, runloop, WindowBuilder};
use druid::{BoxConstraints, PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, RoundedRect, Size, Vec2};
use piet::{Color, FontBuilder, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::thread;

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

pub struct EditorView {
    core: Arc<Mutex<Core>>,
    view: Arc<Mutex<View>>,
    config: Config,
    input: Input,
}

pub struct View {
    id: String,
    pub line_cache: LineCache,
}

impl View {
    pub fn new(view_id: String) -> View {
        View {
            id: view_id,
            line_cache: LineCache::new(),
        }
    }

    pub fn apply_update(&mut self, update: &Value) {
        self.line_cache.apply_update(update);
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
    pub fn new(core: Arc<Mutex<Core>>, view: Arc<Mutex<View>>, config: Config) -> EditorView {
        EditorView {
            core,
            view,
            config,
            input: Input::new(),
        }
    }

    fn paint_line(&self, i: usize, paint_ctx: &mut PaintCtx, line: &Line) {
        let mut text = line.text().to_string();
        // text.pop();

        let app_font = self.config.font.lock().unwrap();

        let font = paint_ctx
            .text()
            .new_font_by_name("Consolas", 13.0)
            .unwrap()
            .build()
            .unwrap();
        for style in line.styles() {
            let range = &style.range;
            let layout = paint_ctx
                .text()
                .new_text_layout(&font, &text[range.start..range.end])
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
                Point::new(x, app_font.lineheight() * (i + 1) as f64),
                &Color::from_rgba32_u32(fg_color),
            );
        }
    }

    fn run(&mut self, cmd: &Command, key_input: KeyInput) {
        match *cmd {
            Command::Insert => self.input.state = InputState::Insert,
            Command::Escape => self.input.state = InputState::Nomral,
            Command::Unknown => {
                if self.input.state == InputState::Insert && key_input.text != "" {
                    self.core.lock().unwrap().send_notification(
                        "edit",
                        &json!({
                            "view_id": self.view.lock().unwrap().id.clone(),
                            "method": "insert",
                            "params": {"chars": key_input.text},
                        }),
                    );
                }
            }
        }
    }
}

impl Widget for EditorView {
    fn paint(&mut self, paint_ctx: &mut PaintCtx) {
        let theme = self.config.theme.lock().unwrap();
        let bg = theme.background.unwrap();
        paint_ctx.clear(Color::rgba8(bg.r, bg.g, bg.b, bg.a));
        let font = paint_ctx
            .text()
            .new_font_by_name("Consolas", 13.0)
            .unwrap()
            .build()
            .unwrap();
        let line_cache = &self.view.lock().unwrap().line_cache;

        for i in 0..line_cache.height() {
            if let Some(line) = line_cache.get_line(i) {
                self.paint_line(i, paint_ctx, line);
            }
        }
    }

    fn layout(&mut self, bc: &BoxConstraints) -> Size {
        bc.max()
    }

    fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        println!("mouse down {:?}", event.pos);
        let font = self.config.font.lock().unwrap();

        let line = (event.pos.y as f64 / font.lineheight()) as u32;
        let column = (event.pos.x as f64 / font.width) as u32;
        let core = self.core.clone();
        let view_id = self.view.lock().unwrap().id.clone();
        core.lock().unwrap().send_notification(
            "edit",
            &json!({
                "view_id": view_id,
                "method": "gesture",
                "params": {
                    "line":line,
                    "col":column,
                    "ty": "point_select"
                },
            }),
        );
    }

    fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        println!("got key event {:?}", event);
        let config = self.config.clone();
        let keymaps = config.keymaps.lock().unwrap();
        let key_input = KeyInput::from_keyevent(&event, self.input.state.clone());
        let cmd = keymaps
            .get(&key_input.get_key())
            .unwrap_or(&Command::Unknown);
        println!("get key input {}", key_input.clone());
        self.run(cmd, key_input);

        false
    }
}
