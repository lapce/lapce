use crate::ui::widget::{Widget, WidgetPod};
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::shell::{kurbo, piet, runloop, WindowBuilder};
use druid::{BoxConstraints, PaintCtx, TimerToken};
use kurbo::{Affine, Point, Rect, RoundedRect, Size, Vec2};
use piet::{Color, FontBuilder, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

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
    children: Vec<ChildWidget>,
    direction: Axis,
}

impl Flex {
    pub fn new(direction: Axis) -> Flex {
        Flex {
            children: Vec::new(),
            direction,
        }
    }

    pub fn add_child(&mut self, widget: Arc<Mutex<Box<Widget + Send + Sync>>>) {
        let mut child = ChildWidget {
            widget: WidgetPod::new(widget),
        };
        self.children.push(child);
    }
}

impl Widget for Flex {
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

    fn mouse_down(&mut self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
        for child in &mut self.children {
            child.widget.mouse_down(event, ctx);
        }
    }

    fn key_down(&mut self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        for child in &mut self.children {
            child.widget.key_down(event, ctx);
        }
        false
    }
}
