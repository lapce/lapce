use druid::widget::{prelude::*, Controller, Label};
use lapce_data::config::GetConfig;

pub struct TextColorWatcher(&'static str);

impl TextColorWatcher {
    pub fn new(key: &'static str) -> Self {
        Self(key)
    }
}

impl<T: Data + GetConfig> Controller<T, Label<T>> for TextColorWatcher {
    fn lifecycle(
        &mut self,
        child: &mut Label<T>,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        child.lifecycle(ctx, event, data, env);
        if let LifeCycle::WidgetAdded = event {
            child.set_text_color(
                data.get_config().get_color_unchecked(self.0).clone(),
            );
        }
    }

    fn update(
        &mut self,
        child: &mut Label<T>,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
        child.update(ctx, old_data, data, env);
        if !data.same(old_data) {
            child.set_text_color(
                data.get_config().get_color_unchecked(self.0).clone(),
            );
        }
    }
}
