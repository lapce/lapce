use druid::{
    kurbo::Line,
    piet::{PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, FontFamily, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};

use crate::{config::LapceTheme, data::LapceTabData, keymap::LapceKeymap};

#[derive(Clone)]
pub struct LapceSettingsData {
    pub shown: bool,

    pub keymap_widget_id: WidgetId,
    pub keymap_view_id: WidgetId,
    pub keymap_split_id: WidgetId,
}

impl LapceSettingsData {
    pub fn new() -> Self {
        Self {
            shown: true,
            keymap_widget_id: WidgetId::next(),
            keymap_view_id: WidgetId::next(),
            keymap_split_id: WidgetId::next(),
        }
    }
}

pub struct LapceSettings {
    active: usize,
    content_rect: Rect,
    children: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceSettings {
    pub fn new(data: &LapceTabData) -> Self {
        let mut children = Vec::new();
        children.push(WidgetPod::new(LapceKeymap::new(data)));
        Self {
            active: 0,
            content_rect: Rect::ZERO,
            children,
        }
    }
}

impl Widget<LapceTabData> for LapceSettings {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        self.children[self.active].event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        for child in self.children.iter_mut() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.children[self.active].update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let tab_size = bc.max();

        let self_size = Size::new(
            (tab_size.width * 0.8).min(900.0),
            (tab_size.height * 0.8).min(700.0),
        );
        let origin = Point::new(
            tab_size.width / 2.0 - self_size.width / 2.0,
            (tab_size.height / 2.0 - self_size.height / 2.0) / 2.0,
        );
        self.content_rect = self_size.to_rect().with_origin(origin);

        let content_size = Size::new(self_size.width - 100.0, self_size.height);
        let content_origin = origin + (self_size.width - content_size.width, 0.0);
        let content_bc = BoxConstraints::tight(content_size);
        for child in self.children.iter_mut() {
            child.layout(ctx, &content_bc, data, env);
            child.set_origin(ctx, data, env, content_origin);
        }

        tab_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if data.settings.shown {
            let rect = ctx.size().to_rect();
            ctx.fill(
                rect,
                &data
                    .config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW)
                    .clone()
                    .with_alpha(0.5),
            );

            let shadow_width = 5.0;
            ctx.blurred_rect(
                self.content_rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
            ctx.fill(
                self.content_rect,
                data.config
                    .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
            );

            self.children[self.active].paint(ctx, data, env);
        }
    }
}
