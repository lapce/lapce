use std::f64::INFINITY;

use druid::kurbo::{Point, Rect, Size, Vec2};
use druid::{
    scroll_component::*, widget::ClipBox, widget::Scroll, BoxConstraints, Data, Env,
    Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, UpdateCtx,
    Widget, WidgetPod,
};

use crate::command::{LapceUICommand, LAPCE_UI_COMMAND};

#[derive(Debug, Clone)]
enum ScrollDirection {
    Bidirectional,
    Vertical,
    Horizontal,
}

/// A container that scrolls its contents.
///
/// This container holds a single child, and uses the wheel to scroll it
/// when the child's bounds are larger than the viewport.
///
/// The child is laid out with completely unconstrained layout bounds by
/// default. Restrict to a specific axis with [`vertical`] or [`horizontal`].
/// When restricted to scrolling on a specific axis the child's size is
/// locked on the opposite axis.
///
/// [`vertical`]: struct.Scroll.html#method.vertical
/// [`horizontal`]: struct.Scroll.html#method.horizontal
pub struct LapceScroll<T, W> {
    clip: ClipBox<T, W>,
    //child: WidgetPod<T, W>,
    scroll_component: ScrollComponent,
    //direction: ScrollDirection,
    //content_size: Size,
    //scroll_offset: Vec2,
}

impl<T: Data, W: Widget<T>> LapceScroll<T, W> {
    /// Create a new scroll container.
    ///
    /// This method will allow scrolling in all directions if child's bounds
    /// are larger than the viewport. Use [vertical](#method.vertical) and
    /// [horizontal](#method.horizontal) methods to limit scrolling to a specific axis.
    pub fn new(child: W) -> LapceScroll<T, W> {
        LapceScroll {
            clip: ClipBox::new(child),
            scroll_component: ScrollComponent::new(),
            //direction: ScrollDirection::Bidirectional,
            //content_size: Size::ZERO,
            //scroll_offset: Vec2::ZERO,
        }
    }

    /// Restrict scrolling to the vertical axis while locking child width.
    pub fn vertical(mut self) -> Self {
        self.clip.set_constrain_vertical(false);
        self.clip.set_constrain_horizontal(true);
        self
    }

    /// Restrict scrolling to the horizontal axis while locking child height.
    pub fn horizontal(mut self) -> Self {
        self.clip.set_constrain_vertical(true);
        self.clip.set_constrain_horizontal(false);
        self
    }

    /// Returns a reference to the child widget.
    pub fn child(&self) -> &W {
        self.clip.child()
    }

    /// Returns a mutable reference to the child widget.
    pub fn child_mut(&mut self) -> &mut W {
        self.clip.child_mut()
    }

    /// Returns the size of the child widget.
    pub fn child_size(&self) -> Size {
        self.clip.content_size()
    }

    /// Returns the current scroll offset.
    pub fn offset(&self) -> Vec2 {
        self.clip.viewport_origin().to_vec2()
    }

    pub fn scroll(&mut self, x: f64, y: f64) {
        self.clip.pan_by(Vec2::new(x, y));
        //let mut offset = self.offset();
        //offset.x = offset.x + x;
        //offset.y = offset.y + y;
        //if offset.y < 0.0 {
        //    offset.y = 0.0;
        //}
        //self.scroll_offset = offset;
        //self.child.set_viewport_offset(offset);
    }

    pub fn scroll_to(&mut self, x: f64, y: f64) {
        self.clip.pan_to(Point::new(x, y));
    }

    pub fn ensure_visible(
        &mut self,
        scroll_size: Size,
        rect: &Rect,
        margin: &(f64, f64),
    ) -> bool {
        let mut new_offset = self.offset();
        let content_size = self.child_size();

        let (x_margin, y_margin) = margin;

        new_offset.x = if new_offset.x < rect.x1 + x_margin - scroll_size.width {
            (rect.x1 + x_margin - scroll_size.width)
                .min(content_size.width - scroll_size.width)
        } else if new_offset.x > rect.x0 - x_margin {
            (rect.x0 - x_margin).max(0.0)
        } else {
            new_offset.x
        };

        new_offset.y = if new_offset.y < rect.y1 + y_margin - scroll_size.height {
            (rect.y1 + y_margin - scroll_size.height)
                .min(content_size.height - scroll_size.height)
        } else if new_offset.y > rect.y0 - y_margin {
            (rect.y0 - y_margin).max(0.0)
        } else {
            new_offset.y
        };

        if new_offset == self.offset() {
            return false;
        }

        self.clip.pan_to(Point::new(new_offset.x, new_offset.y));
        true
    }
}

impl<T: Data, W: Widget<T>> Widget<T> for LapceScroll<T, W> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        match event {
            Event::Internal(_) => {
                self.clip.event(ctx, event, data, env);
            }
            Event::Command(cmd) => match cmd {
                _ if cmd.is(LAPCE_UI_COMMAND) => {
                    let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                    match command {
                        LapceUICommand::RequestLayout => {
                            println!("scroll request layout");
                            ctx.request_layout();
                        }
                        LapceUICommand::RequestPaint => {
                            println!("scroll request paint");
                            ctx.request_paint();
                        }
                        LapceUICommand::EnsureVisible((rect, margin, position)) => {
                            if self.ensure_visible(ctx.size(), rect, margin) {
                                ctx.request_paint();
                            }
                            return;
                        }
                        LapceUICommand::ScrollTo((x, y)) => {
                            self.scroll_to(*x, *y);
                            return;
                        }
                        LapceUICommand::Scroll((x, y)) => {
                            self.scroll(*x, *y);
                            ctx.request_paint();
                            return;
                        }
                        _ => println!("scroll unprocessed ui command {:?}", command),
                    }
                }
                _ => (),
            },
            _ => (),
        };
        // self.scroll_component.event(ctx, event, env);
        if !ctx.is_handled() {
            self.clip.event(ctx, event, data, env);
        }

        // self.scroll_component.handle_scroll(
        //     self.child.viewport_offset(),
        //     ctx,
        //     event,
        //     env,
        // );
        // In order to ensure that invalidation regions are correctly propagated up the tree,
        // we need to set the viewport offset on our child whenever we change our scroll offset.
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        self.clip.lifecycle(ctx, event, data, env);
        self.scroll_component.lifecycle(ctx, event, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, env: &Env) {
        self.clip.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        bc.debug_check("Scroll");

        let old_size = self.clip.viewport().rect.size();
        let child_size = self.clip.layout(ctx, &bc, data, env);

        let self_size = bc.constrain(child_size);
        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.clip.paint(ctx, data, env);
        self.scroll_component
            .draw_bars(ctx, &self.clip.viewport(), env);
    }
}
