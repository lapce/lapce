use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::shell::{kurbo, piet, runloop, WindowBuilder};
use druid::{BoxConstraints, PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, RoundedRect, Size, Vec2};
use piet::{Color, FontBuilder, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::marker::{Send, Sync};
use std::sync::{Arc, Mutex};

pub trait Widget {
    fn id(&self) -> String;
    fn paint(&mut self, paint_ctx: &mut PaintCtx);
    fn layout(&mut self, bc: &BoxConstraints) -> Size;
    fn size(&mut self, width: f64, height: f64);
    fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool;
    fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx);
    fn mouse_move(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx);
    fn wheel(&mut self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx);
    fn ensure_visble(&mut self, rect: Rect, margin_x: f64, margin_y: f64);
    fn add_child(&mut self, widget: Arc<Mutex<Box<Widget + Send + Sync>>>);
    fn set_parent(&mut self, widget: Arc<Mutex<Box<Widget + Send + Sync>>>);
}

pub struct WidgetPod {
    state: BaseState,
    inner: Arc<Mutex<Box<Widget + Send + Sync>>>,
}

impl WidgetPod {
    pub fn new(widget: Arc<Mutex<Box<Widget + Send + Sync>>>) -> WidgetPod {
        WidgetPod {
            state: Default::default(),
            inner: widget,
        }
    }

    pub fn id(&self) -> String {
        self.inner.lock().unwrap().id().clone()
    }

    fn paint_raw(&mut self, paint_ctx: &mut PaintCtx) {
        self.inner.lock().unwrap().paint(paint_ctx);
    }

    pub fn paint(&mut self, paint_ctx: &mut PaintCtx) {
        if let Err(e) = paint_ctx.save() {
            eprintln!("error saving render context: {:?}", e);
            return;
        }
        paint_ctx.transform(Affine::translate(self.state.layout_rect.origin().to_vec2()));
        self.paint_raw(paint_ctx);
        if let Err(e) = paint_ctx.restore() {
            eprintln!("error restoring render context: {:?}", e);
        }
    }

    pub fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        let rect = self.get_layout_rect();
        if !rect.contains(event.pos) {
            self.state.is_active = false;
            return;
        }
        let mut event = event.clone();
        event.pos = event.pos - rect.origin().to_vec2();
        self.state.is_active = true;
        self.inner.lock().unwrap().mouse_down(&event, ctx);
    }

    pub fn mouse_move(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        let rect = self.get_layout_rect();
        if rect.contains(event.pos) {
            self.state.is_focus = true;
        } else {
            self.state.is_focus = false;
        }
    }

    pub fn wheel(&mut self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
        if !self.state.is_focus {
            return;
        }
        self.inner.lock().unwrap().wheel(delta, mods, ctx);
    }

    pub fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        if !self.state.is_active {
            return false;
        }
        self.inner.lock().unwrap().key_down(event, ctx)
    }

    pub fn layout(&mut self, bc: &BoxConstraints) {
        self.inner.lock().unwrap().layout(bc);
    }

    pub fn size(&mut self, width: f64, height: f64) {
        self.inner.lock().unwrap().size(width, height);
    }

    pub fn set_layout_rect(&mut self, layout_rect: Rect) {
        self.state.layout_rect = layout_rect;
    }

    pub fn get_layout_rect(&self) -> &Rect {
        &self.state.layout_rect
    }
}

#[derive(Default)]
pub struct BaseState {
    layout_rect: Rect,

    // TODO: consider using bitflags for the booleans.

    // This should become an invalidation rect.
    needs_inval: bool,

    is_hot: bool,

    is_active: bool,

    /// Any descendant is active.
    has_active: bool,

    /// Any descendant has requested an animation frame.
    request_anim: bool,

    /// Any descendant has requested a timer.
    ///
    /// Note: we don't have any way of clearing this request, as it's
    /// likely not worth the complexity.
    request_timer: bool,

    is_focus: bool,

    /// This widget or a descendant has focus.
    has_focus: bool,

    /// This widget or a descendant has requested focus.
    request_focus: bool,
}
