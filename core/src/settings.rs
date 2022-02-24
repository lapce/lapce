use std::sync::Arc;

use druid::{
    kurbo::Line,
    piet::{PietTextLayout, Svg, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Data, Env, Event, EventCtx, FontFamily, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size,
    Target, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod,
};

use crate::{
    config::LapceTheme, data::LapceTabData, keymap::LapceKeymap, svg::get_svg,
};

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
            shown: false,
            keymap_widget_id: WidgetId::next(),
            keymap_view_id: WidgetId::next(),
            keymap_split_id: WidgetId::next(),
        }
    }
}

pub struct LapceSettings {
    active: usize,
    content_rect: Rect,
    header_rect: Rect,
    close_rect: Rect,
    children: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceSettings {
    pub fn new(data: &LapceTabData) -> Self {
        let mut children = Vec::new();
        children.push(WidgetPod::new(LapceKeymap::new(data)));
        Self {
            active: 0,
            header_rect: Rect::ZERO,
            content_rect: Rect::ZERO,
            close_rect: Rect::ZERO,
            children,
        }
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &mut LapceTabData,
    ) {
        if self.close_rect.contains(mouse_event.pos) {
            let settings = Arc::make_mut(&mut data.settings);
            settings.shown = false;
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        self.close_rect.contains(mouse_event.pos)
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
        match event {
            Event::MouseMove(mouse_event) => {
                if !data.settings.shown {
                    return;
                }
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                    ctx.set_handled();
                    ctx.request_paint();
                } else {
                    ctx.clear_cursor();
                    ctx.request_paint();
                }
            }
            Event::MouseDown(mouse_event) => {
                if !data.settings.shown {
                    return;
                }
                self.mouse_down(ctx, mouse_event, data);
            }
            _ => {}
        }
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
        self.header_rect = Size::new(self_size.width, 50.0)
            .to_rect()
            .with_origin(origin);

        let close_size = 26.0;
        self.close_rect = Size::new(close_size, close_size).to_rect().with_origin(
            origin
                + (
                    self.header_rect.width()
                        - (self.header_rect.height() / 2.0 - close_size / 2.0)
                        - close_size,
                    self.header_rect.height() / 2.0 - close_size / 2.0,
                ),
        );

        let content_size = Size::new(
            self_size.width - 100.0,
            self_size.height - self.header_rect.height(),
        );
        let content_origin = origin
            + (
                self_size.width - content_size.width,
                self_size.height - content_size.height,
            );
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

            ctx.blurred_rect(
                self.header_rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
            let text_layout = ctx
                .text()
                .new_text_layout("Settings".to_string())
                .font(FontFamily::SYSTEM_UI, 16.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            let text_size = text_layout.size();
            ctx.draw_text(
                &text_layout,
                self.header_rect.origin()
                    + (
                        self.header_rect.height() / 2.0 - text_size.height / 2.0,
                        self.header_rect.height() / 2.0 - text_size.height / 2.0,
                    ),
            );

            let svg = get_svg("close.svg").unwrap();
            let icon_padding = 4.0;
            ctx.draw_svg(
                &svg,
                self.close_rect.inflate(-icon_padding, -icon_padding),
                Some(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
                ),
            );

            self.children[self.active].paint(ctx, data, env);
        }
    }
}
