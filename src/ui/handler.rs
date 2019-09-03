use super::widget::Widget;
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::shell::{kurbo, piet, runloop, WindowBuilder};
use druid::{BoxConstraints, PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, RoundedRect, Size, Vec2};
use piet::{Color, FontBuilder, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::any::Any;
use std::sync::{Arc, Mutex};

pub struct UiHandler {
    root: Arc<Mutex<Widget>>,
    handle: WindowHandle,
    size: Size,
}

impl UiHandler {
    pub fn new(root: Arc<Mutex<Widget>>) -> UiHandler {
        UiHandler {
            root,
            handle: Default::default(),
            size: Default::default(),
        }
    }
}

impl WinHandler for UiHandler {
    fn connect(&mut self, handle: &WindowHandle) {
        self.handle = handle.clone();
    }

    fn paint(&mut self, piet: &mut Piet, ctx: &mut dyn WinCtx) -> bool {
        let bc = BoxConstraints::tight(self.size);
        let text = piet.text();
        self.root.lock().unwrap().layout(&bc);

        // piet.clear(Color::rgb8(0x27, 0x28, 0x22));
        let mut paint_ctx = PaintCtx { render_ctx: piet };
        self.root.lock().unwrap().paint(&mut paint_ctx);
        false
    }

    fn command(&mut self, id: u32, ctx: &mut dyn WinCtx) {
        eprintln!("got command {}", id);
    }

    fn size(&mut self, width: u32, height: u32, _ctx: &mut dyn WinCtx) {
        let dpi = self.handle.get_dpi() as f64;
        let scale = 96.0 / dpi;
        self.size = Size::new(width as f64 * scale, height as f64 * scale);
    }

    fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        self.root.lock().unwrap().mouse_down(event, ctx);
    }

    fn mouse_up(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn mouse_move(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        self.root.lock().unwrap().key_down(event, ctx)
    }

    fn key_up(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&mut self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn timer(&mut self, token: TimerToken, ctx: &mut dyn WinCtx) {}

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
