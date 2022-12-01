use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Rect, Size, UpdateCtx, Widget,
};
use lapce_data::{config::LapceTheme, data::LapceTabData};

pub struct LapceIconSvg {
    icon: &'static str,
    rect: Rect,
}

impl LapceIconSvg {
    pub fn new(icon: &'static str) -> Self {
        Self {
            icon,
            rect: Rect::ZERO,
        }
    }
}

impl Widget<LapceTabData> for LapceIconSvg {
    fn event(
        &mut self,
        _ctx: &mut EventCtx,
        _event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let size = bc.max();
        self.rect = size.to_rect();
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let svg = data.config.ui_svg(self.icon);
        ctx.draw_svg(
            &svg,
            self.rect,
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE),
            ),
        );
    }
}
