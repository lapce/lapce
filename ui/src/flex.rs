use super::{WidgetState, WidgetTrait};
use crane_ui_macros::WidgetBase;
use druid::shell::keyboard::{KeyCode, KeyEvent, KeyModifiers};
use druid::shell::platform::IdleHandle;
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet, TimerToken};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use piet::{
    Color, FontBuilder, LinearGradient, Piet, RenderContext, Text, TextLayout, TextLayoutBuilder,
    UnitPoint,
};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

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
    widget_state: Arc<Mutex<WidgetState>>,
    state: Arc<Mutex<FlexState>>,
}

impl Flex {
    pub fn new(direction: Axis) -> Flex {
        Flex {
            widget_state: Arc::new(Mutex::new(WidgetState::new())),
            state: Arc::new(Mutex::new(FlexState { direction })),
        }
    }

    fn layout(&self) {
        let size = self.widget_state.lock().unwrap().size();
        let direction = self.state.lock().unwrap().direction.clone();
        let num_children = self.widget_state.lock().unwrap().num_children();
        // if num_children == 0 {
        //     return;
        // }
        let mut custom_distance = 0.0;
        let mut custom_n = 0;
        for i in 0..num_children {
            let child = self.widget_state.lock().unwrap().child(i);
            let custom_size = child.custom_rect().size();
            let child_custom_distance = match direction {
                Axis::Horizontal => custom_size.height,
                Axis::Vertical => custom_size.width,
            };
            if child_custom_distance > 0.0 {
                custom_distance += child_custom_distance;
                custom_n += 1;
            }
        }

        let child_size = match direction {
            Axis::Horizontal => Size::new(
                size.width,
                (size.height - custom_distance) / (num_children - custom_n) as f64,
            ),
            Axis::Vertical => Size::new(
                (size.width / num_children as f64).floor() - 1.0,
                size.height,
            ),
        };

        let mut width_cum = 0.0;
        let mut height_cum = 0.0;
        for i in 0..num_children {
            let child = self.widget_state.lock().unwrap().child(i);
            let custom_size = child.custom_rect().size();
            let origin = match direction {
                Axis::Horizontal => Point::new(0.0, height_cum),
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
                Axis::Horizontal => {
                    if custom_size.height > 0.0 {
                        height_cum += custom_size.height;
                        Size::new(size.width, custom_size.height)
                    } else {
                        height_cum += child_size.height;
                        child_size
                    }
                }
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
        let num_children = self.widget_state.lock().unwrap().num_children();
        for i in 0..num_children {
            let child = self.widget_state.lock().unwrap().child(i);
            let rect = child.get_rect();
            paint_ctx.fill(
                Rect::from_origin_size(Point::new(rect.x1, 0.0), Size::new(1.0, rect.height())),
                &Color::rgba8(0, 0, 0, 255),
            );
        }
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        false
    }
}
