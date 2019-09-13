use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::platform::IdleHandle;
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{Color, FontBuilder, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::thread;
use uuid::Uuid;

#[derive(Default)]
pub struct WidgetState {
    id: String,
    window_handle: WindowHandle,
    rect: Rect,
    vertical_scroll: f64,
    horizontal_scroll: f64,
    parent: Option<Box<Widget>>,
    children: Vec<Box<Widget>>,
    is_active: bool,
    is_focus: bool,
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

    pub fn add_child(&mut self, child: Box<Widget>) {
        self.children.push(child);
    }

    pub fn replace_child(&mut self, index: usize, child: Box<Widget>) {
        self.children[index] = child;
    }

    pub fn parent(&self) -> Option<Box<Widget>> {
        self.parent.clone()
    }

    pub fn set_parent(&mut self, parent: Box<Widget>) {
        self.parent = Some(parent);
    }

    pub fn num_children(&self) -> usize {
        self.children.len()
    }

    pub fn child(&self, i: usize) -> Box<Widget> {
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

    pub fn top_parent(&self) -> Option<Box<Widget>> {
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
    fn clone_box(&self) -> Box<Widget>;
}

impl<T> WidgetClone for T
where
    T: 'static + Widget + Clone,
{
    fn clone_box(&self) -> Box<Widget> {
        Box::new(self.clone())
    }
}

impl Clone for Box<Widget> {
    fn clone(&self) -> Box<Widget> {
        self.clone_box()
    }
}

pub trait Widget: Send + Sync + WidgetClone {
    fn id(&self) -> String;
    fn set_window_handle(&self, handle: WindowHandle);
    fn size(&self, width: f64, height: f64);
    fn get_rect(&self) -> Rect;
    fn set_rect(&self, rect: Rect);
    fn set_active(&self);
    fn set_inactive(&self, propagate: bool);
    fn paint_raw(&self, paint_ctx: &mut PaintCtx, rect: Rect);
    fn add_child(&self, child: Box<Widget>);
    fn replace_child(&self, index: usize, child: Box<Widget>);
    fn set_parent(&self, parent: Box<Widget>);
    fn contains(&self, pos: Point) -> bool;
    fn mouse_down_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool;
    fn mouse_move_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool;
    fn wheel_raw(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx);
    fn key_down_raw(&self, event: KeyEvent, ctx: &mut dyn WinCtx);
    fn invalidate(&self);
    fn invalidate_rect(&self, rect: Rect);
    fn child_ids(&self) -> Vec<String>;
    fn parent(&self) -> Option<Box<Widget>>;
}
