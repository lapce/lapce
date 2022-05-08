//! Stops propagating input events (mouse, keyboard)

use druid::widget::{prelude::*, Controller};

pub struct InputGate;

impl<T, W> Controller<T, W> for InputGate
where
    T: Data,
    W: Widget<T>,
{
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
        if event.should_propagate_to_hidden() {
            child.event(ctx, event, data, env);
        }
    }
}
