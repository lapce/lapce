use druid::widget::prelude::*;
use lapce_data::config::{GetConfig, LapceTheme};

use crate::svg::get_svg;

pub struct Svg(String);

impl Svg {
    pub fn new(path: String) -> Self {
        Self(path)
    }

    pub fn set_svg_path(&mut self, path: String) {
        self.0 = path;
    }
}

impl<T: Data + GetConfig> Widget<T> for Svg {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut T,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &T,
        _env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            ctx.request_layout();
            ctx.request_paint();
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &T,
        _data: &T,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        _data: &T,
        _env: &Env,
    ) -> Size {
        if get_svg(&self.0).is_some() {
            // TODO this isn't very flexible
            Size::new(14.0, 14.0)
        } else {
            Size::new(0.0, 0.0)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, _env: &Env) {
        if let Some(svg) = get_svg(&self.0) {
            let rect = ctx.size().to_rect();

            ctx.draw_svg(
                &svg,
                rect,
                Some(
                    data.get_config()
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );
        }
    }
}
