use crate::app::App;
use crane_ui::{WidgetState, WidgetTrait};
use crane_ui_macros::WidgetBase;
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{Color, FontBuilder, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::fs::{self, DirEntry};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
enum PaletteKind {
    File,
}

#[derive(Clone)]
pub struct PaletteItem {
    kind: PaletteKind,
    text: String,
}

pub struct PaletteState {
    input: String,
    items: Vec<PaletteItem>,
}

impl PaletteState {
    pub fn new() -> PaletteState {
        PaletteState {
            items: Vec::new(),
            input: "".to_string(),
        }
    }
}

#[derive(Clone, WidgetBase)]
pub struct Palette {
    state: Arc<Mutex<WidgetState>>,
    local_state: Arc<Mutex<PaletteState>>,
    app: Box<App>,
}

impl Palette {
    pub fn new(app: App) -> Palette {
        Palette {
            state: Arc::new(Mutex::new(WidgetState::new())),
            local_state: Arc::new(Mutex::new(PaletteState::new())),
            app: Box::new(app),
        }
    }

    pub fn run(&self) {
        self.local_state.lock().unwrap().items = self.get_files();
        self.show();
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
                    dirs.push(path);
                } else {
                    let file = path.as_path().to_str().unwrap().to_string();
                    items.push(PaletteItem {
                        kind: PaletteKind::File,
                        text: file,
                    });
                }
            }
        }
        items
    }

    fn layout(&self) {}

    fn paint(&self, paint_ctx: &mut PaintCtx) {
        let color = Color::rgb8(33, 37, 43);
        let size = self.get_rect().size();
        // println!("rect is {:?}", rect);
        paint_ctx.fill(Rect::from_origin_size(Point::ZERO, size), &color);

        let app_font = self.app.config.font.lock().unwrap();
        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();

        let mut i = 0;
        let start = 0;
        let height = self.state.lock().unwrap().size().height;
        let lineheight = app_font.lineheight();
        let line_count = (height / lineheight) as usize + 2;
        for item in self
            .local_state
            .lock()
            .unwrap()
            .items
            .get(start..start + line_count)
            .unwrap()
        {
            let font = paint_ctx
                .text()
                .new_font_by_name("", 13.0)
                .unwrap()
                .build()
                .unwrap();
            let layout = paint_ctx
                .text()
                .new_text_layout(&font, &item.text)
                .unwrap()
                .build()
                .unwrap();
            paint_ctx.draw_text(
                &layout,
                Point::new(
                    0.0,
                    app_font.lineheight() * i as f64 + app_font.ascent + app_font.linespace / 2.0,
                ),
                &Color::rgba8(fg.r, fg.g, fg.b, 255),
            );
            i += 1;
        }
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) {}
}
