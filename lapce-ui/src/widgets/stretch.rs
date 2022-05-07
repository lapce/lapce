use druid::widget::prelude::*;

/// Gives infinite vertical space to child widget.
pub struct StretchVertical<W> {
    inner: W,
}

impl<W> StretchVertical<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W, T: Data> Widget<T> for StretchVertical<W>
where
    W: Widget<T>,
{
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        self.inner.event(ctx, event, data, env)
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        self.inner.lifecycle(ctx, event, data, env)
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, env: &Env) {
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &T,
        env: &Env,
    ) -> Size {
        let bc =
            BoxConstraints::new(bc.min(), Size::new(bc.max().width, f64::INFINITY));
        self.inner.layout(ctx, &bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.inner.paint(ctx, data, env)
    }
}
