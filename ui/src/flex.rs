use super::{Widget, WidgetState};
use crane_ui_macros::WidgetBase;
use druid::shell::keyboard::{KeyCode, KeyEvent, KeyModifiers};
use druid::shell::platform::IdleHandle;
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet, TimerToken};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{Color, FontBuilder, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct Column {}

impl Column {
    pub fn new() -> Flex {
        Flex::new(Axis::Vertical)
    }
}

#[derive(Clone)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone)]
pub struct FlexState {
    direction: Axis,
}

#[derive(Clone, WidgetBase)]
pub struct Flex {
    state: Arc<Mutex<WidgetState>>,
    local_state: Arc<Mutex<FlexState>>,
}

impl Flex {
    pub fn new(direction: Axis) -> Flex {
        let state = WidgetState::new();
        let local_state = FlexState { direction };
        Flex {
            state: Arc::new(Mutex::new(WidgetState::new())),
            local_state: Arc::new(Mutex::new(local_state)),
        }
    }

    fn layout(&self) {
        let size = self.state.lock().unwrap().size();
        let direction = self.local_state.lock().unwrap().direction.clone();
        let num_children = self.state.lock().unwrap().num_children();
        // if num_children == 0 {
        //     return;
        // }

        let child_size = match direction {
            Axis::Horizontal => Size::new(size.width, size.height / num_children as f64),
            Axis::Vertical => Size::new(
                (size.width / num_children as f64).floor() - 1.0,
                size.height,
            ),
        };

        let mut width_cum = 0.0;
        for i in 0..num_children {
            let child = self.state.lock().unwrap().child(i);
            let origin = match direction {
                Axis::Horizontal => Point::new(0.0, i as f64 * child_size.height),
                Axis::Vertical => Point::new(
                    if i == 0 {
                        i as f64 * child_size.width
                    } else {
                        i as f64 * (child_size.width + 1.0) + (i / 2) as f64
                    },
                    0.0,
                ),
            };
            let size = match direction {
                Axis::Horizontal => child_size,
                Axis::Vertical => {
                    let width = if i == num_children - 1 {
                        size.width - width_cum
                    } else {
                        child_size.width + (i % 2) as f64
                    };
                    width_cum += width;
                    Size::new(width, child_size.height)
                }
            };
            let rect = Rect::from_origin_size(origin, size);
            child.set_rect(rect);
        }
    }

    fn paint(&self, paint_ctx: &mut PaintCtx) {
        let num_children = self.state.lock().unwrap().num_children();
        for i in 0..num_children {
            let child = self.state.lock().unwrap().child(i);
            let rect = child.get_rect();
            paint_ctx.fill(
                Rect::from_origin_size(Point::new(rect.x1, 0.0), Size::new(1.0, rect.height())),
                &Color::rgba8(0, 0, 0, 255),
            );
        }
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) {}
}
