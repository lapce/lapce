use druid::{widget::prelude::*, Color};

pub struct Background<W> {
    background: Option<Color>,
    inner: W,
}

impl<W> Background<W> {
    pub fn new(inner: W) -> Self {
        Self {
            background: None,
            inner,
        }
    }

    pub fn set_background(&mut self, background: Color) {
        self.background = Some(background);
    }

    pub fn clear_background(&mut self) {
        self.background = None;
    }
}

impl<W: Widget<T>, T: Data> Widget<T> for Background<W> {
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
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        if let Some(background) = self.background.as_ref() {
            let rect = ctx.size().to_rect();
            ctx.fill(rect, background);
        }
        self.inner.paint(ctx, data, env)
    }
}
