use crane_ui_macros::WidgetBase;
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::platform::IdleHandle;
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{Color, FontBuilder, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::sync::{Arc, Mutex};
use std::thread;
use uuid::Uuid;

#[derive(Default)]
pub struct WidgetState {
    id: String,
    window_handle: WindowHandle,
    rect: Rect,
    custom_rect: Rect,
    vertical_scroll: f64,
    horizontal_scroll: f64,
    parent: Option<Box<WidgetTrait>>,
    children: Vec<Box<WidgetTrait>>,
    is_active: bool,
    is_focus: bool,
    is_hidden: bool,
}

impl WidgetState {
    pub fn new() -> WidgetState {
        WidgetState {
            id: Uuid::new_v4().to_string(),
            ..Default::default()
        }
    }

    pub fn id(&self) -> String {
        self.id.clone()
    }

    pub fn set_window_handle(&mut self, handle: WindowHandle) {
        self.window_handle = handle;
    }

    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    pub fn show(&mut self) {
        self.is_hidden = false;
        self.invalidate();
    }

    pub fn hide(&mut self) {
        self.is_hidden = true;
        self.invalidate();
    }

    pub fn is_hidden(&self) -> bool {
        self.is_hidden
    }

    pub fn set_size(&mut self, width: f64, height: f64) {
        let new_rect = self.custom_rect.with_size(Size::new(width, height));
        self.set_custom_rect(new_rect);
    }

    pub fn set_pos(&mut self, x: f64, y: f64) {
        let new_rect = self.custom_rect.with_origin(Point::new(x, y));
        self.set_custom_rect(new_rect);
    }

    pub fn set_custom_rect(&mut self, rect: Rect) {
        self.custom_rect = rect;
        if let Some(parent) = &self.parent {
            parent.layout_raw();
        }
    }

    pub fn custom_rect(&self) -> Rect {
        self.custom_rect.clone()
    }

    pub fn get_rect(&self) -> Rect {
        self.rect.clone()
    }

    pub fn invalidate(&self) {
        self.invalidate_rect(self.rect.with_origin(Point::ZERO));
    }

    pub fn invalidate_rect(&self, rect: Rect) {
        let self_rect = self.rect.clone();
        if let Some(parent) = self.parent.clone() {
            thread::spawn(move || {
                let parent_rect = parent.get_rect();
                parent.invalidate_rect(
                    rect + self_rect.origin().to_vec2() + parent_rect.origin().to_vec2(),
                );
            });
        } else {
            let window_handle = self.window_handle.clone();
            self.window_handle
                .get_idle_handle()
                .unwrap()
                .add_idle(move |_| {
                    window_handle.invalidate_rect(rect);
                });
        }
    }

    pub fn horizontal_scroll(&self) -> f64 {
        self.horizontal_scroll
    }

    pub fn vertical_scroll(&self) -> f64 {
        self.vertical_scroll
    }

    pub fn set_scroll(&mut self, horizontal: f64, vertical: f64) {
        self.horizontal_scroll = horizontal;
        self.vertical_scroll = vertical;
    }

    pub fn size(&self) -> Size {
        self.rect.size().clone()
    }

    pub fn add_child(&mut self, child: Box<WidgetTrait>) {
        self.children.push(child);
    }

    pub fn replace_child(&mut self, index: usize, child: Box<WidgetTrait>) {
        self.children[index] = child;
    }

    pub fn parent(&self) -> Option<Box<WidgetTrait>> {
        self.parent.clone()
    }

    pub fn set_parent(&mut self, parent: Box<WidgetTrait>) {
        self.parent = Some(parent);
    }

    pub fn num_children(&self) -> usize {
        self.children.len()
    }

    pub fn child(&self, i: usize) -> Box<WidgetTrait> {
        self.children[i].clone()
    }

    pub fn paint_children(&self, paint_ctx: &mut PaintCtx, rect: Rect) {
        for child in &self.children {
            child.paint_raw(paint_ctx, rect);
        }
    }

    pub fn contains(&self, pos: Point) -> bool {
        self.rect.contains(pos)
    }

    pub fn no_focus(&mut self) {
        self.is_focus = false
    }

    pub fn is_focus(&self) -> bool {
        self.is_focus
    }

    pub fn set_focus(&mut self) {
        self.is_focus = true
    }

    pub fn set_active(&mut self) {
        self.is_active = true
    }

    pub fn set_inactive(&mut self, propagate: bool) {
        self.is_active = false;
        if propagate {
            for child in &self.children {
                child.set_inactive(propagate);
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn top_parent(&self) -> Option<Box<WidgetTrait>> {
        if self.parent().is_none() {
            return None;
        }
        let mut parent = self.parent().unwrap();
        loop {
            let new_parent = parent.parent();
            if new_parent.is_none() {
                return Some(parent);
            } else {
                parent = new_parent.unwrap();
            }
        }
    }

    pub fn child_ids(&self) -> Vec<String> {
        self.children.iter().map(|c| c.id()).collect()
    }

    pub fn child_mouse_move(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool {
        let mut in_child = false;
        for child in self.children.iter().rev() {
            if child.mouse_move_raw(event, ctx) {
                in_child = true;
            }
        }
        in_child
    }

    pub fn child_mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool {
        let mut in_child = false;
        for child in self.children.iter().rev() {
            if child.mouse_down_raw(event, ctx) {
                in_child = true;
            }
        }
        in_child
    }

    pub fn child_wheel(&mut self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
        for child in &self.children {
            child.wheel_raw(delta, mods, ctx);
        }
    }

    pub fn child_key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) {
        for child in &self.children {
            child.key_down_raw(event, ctx);
        }
    }
}

trait WidgetClone {
    fn clone_box(&self) -> Box<WidgetTrait>;
}

impl<T> WidgetClone for T
where
    T: 'static + WidgetTrait + Clone,
{
    fn clone_box(&self) -> Box<WidgetTrait> {
        Box::new(self.clone())
    }
}

impl Clone for Box<WidgetTrait> {
    fn clone(&self) -> Box<WidgetTrait> {
        self.clone_box()
    }
}

pub trait WidgetTrait: Send + Sync + WidgetClone {
    fn id(&self) -> String;
    fn set_window_handle(&self, handle: WindowHandle);
    fn size(&self, width: f64, height: f64);
    fn get_rect(&self) -> Rect;
    fn custom_rect(&self) -> Rect;
    fn set_custom_rect(&self, rect: Rect);
    fn set_rect(&self, rect: Rect);
    fn set_size(&self, width: f64, height: f64);
    fn set_pos(&self, x: f64, y: f64);
    fn layout_raw(&self);
    fn set_active(&self);
    fn set_inactive(&self, propagate: bool);
    fn paint_raw(&self, paint_ctx: &mut PaintCtx, rect: Rect);
    fn add_child(&self, child: Box<WidgetTrait>);
    fn replace_child(&self, index: usize, child: Box<WidgetTrait>);
    fn set_parent(&self, parent: Box<WidgetTrait>);
    fn show(&self);
    fn hide(&self);
    fn contains(&self, pos: Point) -> bool;
    fn mouse_down_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool;
    fn mouse_move_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool;
    fn wheel_raw(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx);
    fn key_down_raw(&self, event: KeyEvent, ctx: &mut dyn WinCtx);
    fn invalidate(&self);
    fn invalidate_rect(&self, rect: Rect);
    fn child_ids(&self) -> Vec<String>;
    fn parent(&self) -> Option<Box<WidgetTrait>>;
}

#[derive(Clone, WidgetBase)]
pub struct Widget {
    state: Arc<Mutex<WidgetState>>,
}

impl Widget {
    pub fn new() -> Widget {
        Widget {
            state: Arc::new(Mutex::new(WidgetState::new())),
        }
    }

    fn layout(&self) {
        let num_children = self.state.lock().unwrap().num_children();
        let rect = self.get_rect();

        for i in 0..num_children {
            let child = self.state.lock().unwrap().child(i);
            let child_custom_rect = child.custom_rect();
            if child_custom_rect.x0 == 0.0
                && child_custom_rect.x1 == 0.0
                && child_custom_rect.y0 == 0.0
                && child_custom_rect.y1 == 0.0
            {
                child.set_rect(rect.clone());
            } else {
                child.set_rect(child_custom_rect.clone());
            }
        }
    }

    fn paint(&self, paint_ctx: &mut PaintCtx) {}

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) {}
}
