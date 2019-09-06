use super::Widget;
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::{kurbo, piet, BoxConstraints, PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, RoundedRect, Size, Vec2};
use piet::Piet;
use std::any::Any;
use std::sync::{Arc, Mutex};

pub struct UiHandler {
    root: Arc<Widget>,
    handle: WindowHandle,
}

impl UiHandler {
    pub fn new(root: Arc<Widget>) -> UiHandler {
        UiHandler {
            root,
            handle: Default::default(),
        }
    }
}

impl WinHandler for UiHandler {
    fn connect(&mut self, handle: &WindowHandle) {
        self.root.set_window_handle(handle.clone());
    }

    fn size(&mut self, width: u32, height: u32, _ctx: &mut dyn WinCtx) {
        let dpi = self.handle.get_dpi() as f64;
        let scale = 96.0 / dpi;
        self.root.size(width as f64 * scale, height as f64 * scale);
    }

    fn paint(&mut self, piet: &mut Piet, rect: Rect, ctx: &mut dyn WinCtx) -> bool {
        let mut paint_ctx = PaintCtx { render_ctx: piet };
        self.root.paint_raw(&mut paint_ctx, rect);
        false
    }

    fn command(&mut self, id: u32, ctx: &mut dyn WinCtx) {}

    fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        self.root.mouse_down_raw(event, ctx);
    }

    fn mouse_move(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        self.root.mouse_move_raw(event, ctx);
    }

    fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        self.root.key_down_raw(event, ctx);
        false
    }

    fn key_up(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&mut self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
        self.root.wheel_raw(delta, mods, ctx);
    }

    fn timer(&mut self, token: TimerToken, ctx: &mut dyn WinCtx) {}

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
