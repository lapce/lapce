use crate::ui::widget::{Widget, WidgetPod};
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::shell::{kurbo, piet, runloop, WindowBuilder};
use druid::{BoxConstraints, PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, RoundedRect, Size, Vec2};
use piet::{Color, FontBuilder, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

struct ChildWidget {
    widget: WidgetPod,
}
pub enum Axis {
    Horizontal,
    Vertical,
}

pub struct Column {}

pub struct Row {}

impl Column {
    pub fn new() -> Flex {
        Flex::new(Axis::Vertical)
    }
}

impl Row {
    pub fn new() -> Flex {
        Flex::new(Axis::Horizontal)
    }
}

pub struct Flex {
    id: String,
    children: Vec<ChildWidget>,
    direction: Axis,
    width: f64,
    height: f64,
}

impl Flex {
    pub fn new(direction: Axis) -> Flex {
        Flex {
            id: Uuid::new_v4().to_string(),
            children: Vec::new(),
            direction,
            width: 0.0,
            height: 0.0,
        }
    }
}

impl Widget for Flex {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn paint(&mut self, paint_ctx: &mut PaintCtx) {
        for child in &mut self.children {
            child.widget.paint(paint_ctx);
        }
    }

    fn layout(&mut self, bc: &BoxConstraints) -> Size {
        let size = bc.max();
        let child_size = match self.direction {
            Axis::Horizontal => Size::new(size.width, size.height / self.children.len() as f64),
            Axis::Vertical => Size::new(size.width / self.children.len() as f64, size.height),
        };
        let mut i = 0;
        for child in &mut self.children {
            let origin = match self.direction {
                Axis::Horizontal => Point::new(0.0, i as f64 * child_size.height),
                Axis::Vertical => Point::new(i as f64 * child_size.width, 0.0),
            };
            let rect = Rect::from_origin_size(origin, child_size);
            child.widget.set_layout_rect(rect);
            i += 1;
        }
        let child_bc = BoxConstraints::new(child_size.clone(), child_size.clone());
        for child in &mut self.children {
            child.widget.layout(&child_bc);
        }
        size
    }

    fn size(&mut self, width: f64, height: f64) {
        self.width = width;
        self.height = height;
        let child_size = match self.direction {
            Axis::Horizontal => Size::new(width, height / self.children.len() as f64),
            Axis::Vertical => Size::new(width / self.children.len() as f64, height),
        };
        let mut i = 0;
        for child in &mut self.children {
            let origin = match self.direction {
                Axis::Horizontal => Point::new(0.0, i as f64 * child_size.height),
                Axis::Vertical => Point::new(i as f64 * child_size.width, 0.0),
            };
            let rect = Rect::from_origin_size(origin, child_size);
            child.widget.set_layout_rect(rect);
            i += 1;
        }
        let child_bc = BoxConstraints::new(child_size.clone(), child_size.clone());
        for child in &mut self.children {
            child.widget.size(child_size.width, child_size.height);
        }
    }

    fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        for child in &mut self.children {
            child.widget.key_down(event, ctx);
        }
        false
    }

    fn wheel(&mut self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
        for child in &mut self.children {
            child.widget.wheel(delta, mods, ctx);
        }
    }

    fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        for child in &mut self.children {
            child.widget.mouse_down(event, ctx);
        }
    }

    fn mouse_move(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        for child in &mut self.children {
            child.widget.mouse_move(event, ctx);
        }
    }

    fn ensure_visble(&mut self, rect: Rect, margin_x: f64, margin_y: f64) {}

    fn add_child(&mut self, widget: Arc<Mutex<Box<Widget + Send + Sync>>>) {
        let mut child = ChildWidget {
            widget: WidgetPod::new(widget),
        };
        self.children.push(child);
        self.size(self.width, self.height);
    }

    fn set_parent(&mut self, widget: Arc<Mutex<Box<Widget + Send + Sync>>>) {}
}
