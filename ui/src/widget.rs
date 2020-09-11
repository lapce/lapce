use crane_ui_macros::WidgetBase;
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::platform::IdleHandle;
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{Color, LinearGradient, RenderContext, Text, TextLayoutBuilder, UnitPoint};
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
    content_size: Size,
    parent: Option<Box<WidgetTrait>>,
    children: Vec<Box<WidgetTrait>>,
    is_active: bool,
    is_focus: bool,
    is_hidden: bool,
    background: Option<Color>,
    shadow: (f64, f64, f64, f64, Option<Color>),
    padding: (f64, f64, f64, f64),
}

pub trait WidgetClone {
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
    fn set_content_size(&self, width: f64, height: f64);
    fn set_pos(&self, x: f64, y: f64);
    fn set_background(&self, color: Color);
    fn background(&self) -> Option<Color>;
    fn set_shadow(&self, horizontal: f64, vertical: f64, blur: f64, spread: f64, color: Color);
    fn shadow(&self) -> (f64, f64, f64, f64, Option<Color>);
    fn padding(&self) -> (f64, f64, f64, f64);
    fn set_padding(&self, top: f64, right: f64, bottom: f64, left: f64);
    fn layout_raw(&self);
    fn set_active(&self);
    fn set_inactive(&self, propagate: bool);
    fn paint_raw(&self, paint_ctx: &mut PaintCtx, rect: Rect);
    fn add_child(&self, child: Box<WidgetTrait>);
    fn remove_child(&self, child: String);
    fn replace_child(&self, index: usize, child: Box<WidgetTrait>);
    fn set_parent(&self, parent: Box<WidgetTrait>);
    fn set_scroll(&self, horizontal: f64, vertical: f64);
    fn show(&self);
    fn hide(&self);
    fn is_hidden(&self) -> bool;
    fn contains(&self, pos: Point) -> bool;
    fn mouse_down_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool;
    fn mouse_move_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool;
    fn wheel_raw(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx);
    fn key_down_raw(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool;
    fn invalidate(&self);
    fn invalidate_rect(&self, rect: Rect);
    fn child_ids(&self) -> Vec<String>;
    fn parent(&self) -> Option<Box<WidgetTrait>>;
    fn ensure_visble(&self, rect: Rect, margin_x: f64, margin_y: f64);
    fn horizontal_scroll(&self) -> f64;
    fn vertical_scroll(&self) -> f64;
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

    pub fn set_content_size(&mut self, width: f64, height: f64) {
        self.content_size = Size::new(width, height);
    }

    pub fn content_size(&self) -> Size {
        self.content_size.clone()
    }

    pub fn set_pos(&mut self, x: f64, y: f64) {
        let new_rect = self.custom_rect.with_origin(Point::new(x, y));
        self.set_custom_rect(new_rect);
    }

    pub fn scroll(&mut self, delta: Vec2) {
        if delta.x == 0.0 && delta.y == 0.0 {
            return;
        }
        let content_size = self.content_size();
        let size = self.get_rect().size();

        let mut horizontal_scroll = self.horizontal_scroll();
        let mut vertical_scroll = self.vertical_scroll();
        horizontal_scroll += delta.x;
        if horizontal_scroll > content_size.width - size.width {
            horizontal_scroll = content_size.width - size.width;
        }
        if horizontal_scroll < 0.0 {
            horizontal_scroll = 0.0;
        }

        vertical_scroll += delta.y;
        if vertical_scroll > content_size.height - size.height {
            vertical_scroll = content_size.height - size.height;
        }
        if vertical_scroll < 0.0 {
            vertical_scroll = 0.0;
        }
        println!("now scroll {} {}", horizontal_scroll, vertical_scroll);
        self.set_scroll(horizontal_scroll, vertical_scroll);
        self.invalidate();
    }

    pub fn set_padding(&mut self, top: f64, right: f64, bottom: f64, left: f64) {
        self.padding = (top, right, bottom, left);
    }

    pub fn padding(&mut self) -> (f64, f64, f64, f64) {
        self.padding.clone()
    }

    pub fn set_background(&mut self, color: Color) {
        self.background = Some(color);
    }

    pub fn background(&self) -> Option<Color> {
        self.background.clone()
    }

    pub fn set_shadow(
        &mut self,
        horizontal: f64,
        vertical: f64,
        blur: f64,
        spread: f64,
        color: Color,
    ) {
        self.shadow = (horizontal, vertical, blur, spread, Some(color));
    }

    pub fn shadow(&self) -> (f64, f64, f64, f64, Option<Color>) {
        self.shadow.clone()
    }

    pub fn set_custom_rect(&mut self, rect: Rect) {
        self.custom_rect = rect;
        if let Some(parent) = self.parent.clone() {
            thread::spawn(move || {
                parent.layout_raw();
            });
        }
    }

    pub fn custom_rect(&self) -> Rect {
        self.custom_rect.clone()
    }

    pub fn get_rect(&self) -> Rect {
        self.rect.clone()
    }

    pub fn invalidate(&self) {
        let (horizontal, vertical, blur, spread, color) = self.shadow();

        let rect = match color {
            Some(color) => {
                let rect = self.get_rect();
                let size = rect.size();
                let shadow_rect = Rect::from_origin_size(
                    Point::new(rect.x0 - blur + horizontal, rect.y0 - blur + vertical),
                    Size::new(size.width + 2.0 * blur, size.height + 2.0 * blur),
                );
                rect.union(shadow_rect)
            }
            None => self.get_rect(),
        };

        self.invalidate_rect(rect.with_origin(Point::new(horizontal - blur, vertical - blur)));
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
            match self.window_handle.get_idle_handle() {
                Some(handle) => handle.add_idle(move |_| {
                    window_handle.invalidate_rect(rect);
                }),
                None => (),
            };
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

    pub fn remove_child(&mut self, child_id: String) {
        let mut i = 0;
        for child in &self.children {
            if child.id() == child_id {
                self.children.remove(i);
                break;
            }
            i += 1;
        }
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

    pub fn paint(&self, paint_ctx: &mut PaintCtx) {
        let rect = self.get_rect();
        let size = rect.size();
        let width = size.width;
        let height = size.height;
        let (horizontal, vertical, blur, spread, color) = self.shadow();
        if let Some(color) = color {
            if blur > 0.0 {
                let gradient = LinearGradient::new(
                    UnitPoint::RIGHT,
                    UnitPoint::LEFT,
                    (color.clone(), color.clone().with_alpha(0.0)),
                );
                paint_ctx.fill(
                    Rect::from_origin_size(
                        Point::new(rect.x0 + horizontal - blur, rect.y0 + vertical),
                        Size::new(blur, height),
                    ),
                    &gradient,
                );

                let gradient = LinearGradient::new(
                    UnitPoint::TOP,
                    UnitPoint::BOTTOM,
                    (color.clone(), color.clone().with_alpha(0.0)),
                );
                paint_ctx.fill(
                    Rect::from_origin_size(
                        Point::new(rect.x0 + horizontal, rect.y1 + vertical),
                        Size::new(width, blur),
                    ),
                    &gradient,
                );

                let gradient = LinearGradient::new(
                    UnitPoint::LEFT,
                    UnitPoint::RIGHT,
                    (color.clone(), color.clone().with_alpha(0.0)),
                );
                paint_ctx.fill(
                    Rect::from_origin_size(
                        Point::new(rect.x1 + horizontal, rect.y0 + vertical),
                        Size::new(blur, height),
                    ),
                    &gradient,
                );
            }
        }

        if let Some(bg) = self.background() {
            paint_ctx.fill(rect, &bg);
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

    pub fn child_key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        for child in &self.children {
            if child.key_down_raw(event, ctx) {
                return true;
            }
        }
        return false;
    }

    pub fn ensure_visble(&mut self, rect: Rect, margin_x: f64, margin_y: f64) {
        let mut scroll_x = 0.0;
        let mut scroll_y = 0.0;
        let size = self.get_rect().size();
        let horizontal_scroll = self.horizontal_scroll();
        let vertical_scroll = self.vertical_scroll();
        let right_limit = size.width - horizontal_scroll - margin_x;
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
}

#[derive(Clone, WidgetBase)]
pub struct Widget {
    widget_state: Arc<Mutex<WidgetState>>,
}

impl Widget {
    pub fn new() -> Widget {
        Widget {
            widget_state: Arc::new(Mutex::new(WidgetState::new())),
        }
    }

    fn layout(&self) {
        let widget = self.clone();
        let rect = widget.get_rect();
        let num_children = widget.widget_state.lock().unwrap().num_children();
        thread::spawn(move || {
            for i in 0..num_children {
                let widget_state = widget.widget_state.clone();
                let child = widget_state.lock().unwrap().child(i).clone();
                let child_custom_rect = child.custom_rect();
                let current_child_rect = child.get_rect();
                let child_rect = if child_custom_rect.x0 == 0.0
                    && child_custom_rect.x1 == 0.0
                    && child_custom_rect.y0 == 0.0
                    && child_custom_rect.y1 == 0.0
                {
                    rect.clone()
                } else {
                    child_custom_rect
                };
                child.set_rect(child_rect);
            }

            widget.invalidate();
        });
    }

    fn paint(&self, paint_ctx: &mut PaintCtx) {}

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        false
    }
}
