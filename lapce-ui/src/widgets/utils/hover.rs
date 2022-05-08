use std::marker::PhantomData;

use druid::widget::{prelude::*, Controller};

pub struct Hover<W, T, F> {
    is_hovered: bool,

    /// A closure that will be invoked when the child widget hover state changes.
    hover_changed: F,

    _marker: PhantomData<(W, T)>,
}

impl<W, T, F> Hover<W, T, F>
where
    F: Fn(&mut W, &mut LifeCycleCtx, &T, &Env),
{
    pub fn new(hover_changed: F) -> Self {
        Self {
            is_hovered: false,
            hover_changed,
            _marker: PhantomData,
        }
    }
}

impl<T, W, F> Controller<T, W> for Hover<W, T, F>
where
    F: Fn(&mut W, &mut LifeCycleCtx, &T, &Env),
    T: Data,
    W: Widget<T>,
{
    fn lifecycle(
        &mut self,
        child: &mut W,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        child.lifecycle(ctx, event, data, env);
        match event {
            LifeCycle::HotChanged(_) => {
                (self.hover_changed)(child, ctx, data, env);
                self.is_hovered = ctx.is_hot();
                ctx.request_paint();
            }
            _ => (),
        }
    }
}
