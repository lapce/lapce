use std::cmp::Ordering;

use druid::kurbo::{Line, Rect};
use druid::{
    theme, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, RenderContext, Size, UpdateCtx,
    Widget, WidgetPod,
};

pub struct CraneSplit<T> {
    vertical: bool,
    children: Vec<WidgetPod<T, Box<dyn Widget<T>>>>,
    children_sizes: Vec<f64>,
    current_bar_hover: usize,
}

impl<T> CraneSplit<T> {
    pub fn new(vertical: bool) -> Self {
        CraneSplit {
            vertical,
            children: Vec::new(),
            children_sizes: Vec::new(),
            current_bar_hover: 0,
        }
    }

    pub fn with_child(mut self, child: impl Widget<T> + 'static) -> Self {
        self.children.push(WidgetPod::new(child).boxed());
        let children_len = self.children.len();
        self.children_sizes = (1..children_len)
            .into_iter()
            .map(|i| i as f64 * (1.0 / children_len as f64))
            .collect();
        self
    }

    fn update_split_point(&mut self, size: Size, mouse_pos: Point) {
        let limit = 50.0;
        let i = self.current_bar_hover - 1;
        if i == 0 {
            if mouse_pos.x < limit {
                return;
            }
        }

        let left = if i == 0 {
            0.0
        } else {
            self.children_sizes[i - 1] * size.width
        };

        let right = if i == self.children_sizes.len() - 1 {
            size.width
        } else {
            self.children_sizes[i + 1] * size.width
        };

        if mouse_pos.x < left + limit || mouse_pos.x > right - limit {
            return;
        }

        self.children_sizes[self.current_bar_hover - 1] =
            mouse_pos.x / size.width;
    }

    fn bar_hit_test(&self, size: Size, mouse_pos: Point) -> usize {
        let children_len = self.children.len();
        if children_len <= 1 {
            return 0;
        }
        for i in 1..children_len {
            let x = self.children_sizes[i - 1] * size.width;
            if mouse_pos.x >= x - 3.0 && mouse_pos.x <= x + 3.0 {
                return i;
            }
        }
        0
    }

    fn paint_bar(&mut self, ctx: &mut PaintCtx, env: &Env) {
        let children_len = self.children.len();
        if children_len <= 1 {
            return;
        }

        let size = ctx.size();
        for i in 1..children_len {
            let x = self.children_sizes[i - 1] * size.width;
            let line =
                Line::new(Point::new(x, 0.0), Point::new(x, size.height));
            let color = env.get(theme::BORDER_LIGHT);
            ctx.stroke(line, &color, 1.0);
        }
    }
}

impl<T: Data> Widget<T> for CraneSplit<T> {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            if child.is_active() {
                child.event(ctx, event, data, env);
                if ctx.is_handled() {
                    return;
                }
            }
        }

        match event {
            Event::MouseDown(mouse) => {
                if mouse.button.is_left() {
                    let bar_number = self.bar_hit_test(ctx.size(), mouse.pos);
                    if bar_number > 0 {
                        self.current_bar_hover = bar_number;
                        ctx.set_active(true);
                        ctx.set_handled();
                    }
                }
            }
            Event::MouseUp(mouse) => {
                if mouse.button.is_left() && ctx.is_active() {
                    ctx.set_active(false);
                    self.update_split_point(ctx.size(), mouse.pos);
                    ctx.request_paint();
                }
            }
            Event::MouseMove(mouse) => {
                if ctx.is_active() {
                    self.update_split_point(ctx.size(), mouse.pos);
                    ctx.request_layout();
                }

                if ctx.is_hot() && self.bar_hit_test(ctx.size(), mouse.pos) > 0
                    || ctx.is_active()
                {
                    match self.vertical {
                        true => ctx.set_cursor(&Cursor::ResizeLeftRight),
                        false => ctx.set_cursor(&Cursor::ResizeUpDown),
                    }
                }
            }
            _ => (),
        }

        for child in self.children.as_mut_slice() {
            if !child.is_active() {
                child.event(ctx, event, data, env);
            }
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &T,
        data: &T,
        env: &Env,
    ) {
        for child in self.children.as_mut_slice() {
            child.update(ctx, &data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        let mut my_size = bc.max();

        let children_len = self.children.len();
        if children_len == 0 {
            return my_size;
        }
        let children_sizes = self.children_sizes.clone();
        let sizes: Vec<Size> = self
            .children
            .iter_mut()
            .enumerate()
            .map(|(i, c)| {
                let width = if i < children_sizes.len() {
                    children_sizes[i] * my_size.width
                } else {
                    my_size.width * (1.0 - children_sizes[i - 1])
                };

                let child_bc = BoxConstraints::new(
                    Size::new(width, bc.min().height),
                    Size::new(width, bc.max().height),
                );
                c.layout(ctx, &child_bc, data, env)
            })
            .collect();
        my_size.height = sizes
            .iter()
            .max_by(|x, y| {
                x.height.partial_cmp(&y.height).unwrap_or(Ordering::Equal)
            })
            .unwrap()
            .height;

        for (i, child) in self.children.iter_mut().enumerate() {
            let x = if i == 0 {
                0.0
            } else {
                self.children_sizes[i - 1] * my_size.width
            };
            let child_rect =
                Rect::from_origin_size(Point::new(x, 0.), sizes[i]);
            child.set_layout_rect(ctx, data, env, child_rect);
        }

        my_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.paint_bar(ctx, env);
        for child in self.children.as_mut_slice() {
            child.paint(ctx, &data, env);
        }
    }
}
