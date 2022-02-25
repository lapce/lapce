use std::{collections::HashMap, sync::Arc};

use druid::{
    kurbo::Line,
    piet::{
        PietTextLayout, Svg, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    BoxConstraints, Command, Data, Env, Event, EventCtx, FontFamily, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Vec2, Widget, WidgetExt, WidgetId,
    WidgetPod,
};

use crate::{
    config::{Config, LapceConfig, LapceTheme},
    data::LapceTabData,
    editor::LapceEditorView,
    keymap::LapceKeymap,
    split::LapceSplitNew,
    svg::get_svg,
};

#[derive(Clone)]
pub struct LapceSettingsPanelData {
    pub shown: bool,

    pub keymap_widget_id: WidgetId,
    pub keymap_view_id: WidgetId,
    pub keymap_split_id: WidgetId,

    pub settings_widget_id: WidgetId,
    pub settings_view_id: WidgetId,
    pub settings_split_id: WidgetId,
}

impl LapceSettingsPanelData {
    pub fn new() -> Self {
        Self {
            shown: false,
            keymap_widget_id: WidgetId::next(),
            keymap_view_id: WidgetId::next(),
            keymap_split_id: WidgetId::next(),
            settings_widget_id: WidgetId::next(),
            settings_view_id: WidgetId::next(),
            settings_split_id: WidgetId::next(),
        }
    }
}

pub struct LapceSettingsPanel {
    active: usize,
    content_rect: Rect,
    header_rect: Rect,
    switcher_rect: Rect,
    switcher_line_height: f64,
    close_rect: Rect,
    children: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceSettingsPanel {
    pub fn new(data: &LapceTabData) -> Self {
        let mut children = Vec::new();
        children.push(WidgetPod::new(LapceSettings::new(data)));
        children.push(WidgetPod::new(LapceKeymap::new(data)));
        Self {
            active: 0,
            header_rect: Rect::ZERO,
            content_rect: Rect::ZERO,
            close_rect: Rect::ZERO,
            switcher_rect: Rect::ZERO,
            switcher_line_height: 40.0,
            children,
        }
    }

    fn mouse_down(
        &mut self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &mut LapceTabData,
    ) {
        if self.close_rect.contains(mouse_event.pos)
            || !self.content_rect.contains(mouse_event.pos)
        {
            let settings = Arc::make_mut(&mut data.settings);
            settings.shown = false;
            return;
        }

        if self.switcher_rect.contains(mouse_event.pos) {
            let index = ((mouse_event.pos.y - self.switcher_rect.y0)
                / self.switcher_line_height)
                .floor() as usize;
            if index < self.children.len() {
                self.active = index;
                ctx.request_paint();
            }
            return;
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        self.close_rect.contains(mouse_event.pos)
    }
}

impl Widget<LapceTabData> for LapceSettingsPanel {
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
            Event::Wheel(_) => {
                if !data.settings.shown {
                    return;
                }
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

        self.switcher_rect =
            Size::new(150.0, self_size.height - self.header_rect.height())
                .to_rect()
                .with_origin(origin + (0.0, self.header_rect.height()));

        let content_size = Size::new(
            self_size.width - self.switcher_rect.width() - 40.0,
            self_size.height - self.header_rect.height(),
        );
        let content_origin = origin
            + (
                self_size.width - content_size.width - 20.0,
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

            ctx.with_save(|ctx| {
                ctx.clip(
                    self.switcher_rect.inflate(50.0, 0.0) + Vec2::new(50.0, 0.0),
                );
                ctx.blurred_rect(
                    self.switcher_rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            });

            ctx.fill(
                Size::new(self.switcher_rect.width(), self.switcher_line_height)
                    .to_rect()
                    .with_origin(
                        self.switcher_rect.origin()
                            + (0.0, self.active as f64 * self.switcher_line_height),
                    ),
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
            );

            for (i, text) in ["Settings", "Keybindings"].iter().enumerate() {
                let text_layout = ctx
                    .text()
                    .new_text_layout(text.to_string())
                    .font(FontFamily::SYSTEM_UI, 14.0)
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
                    self.switcher_rect.origin()
                        + (
                            20.0,
                            i as f64 * self.switcher_line_height
                                + (self.switcher_line_height / 2.0
                                    - text_size.height / 2.0),
                        ),
                );
            }

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

pub enum SettingsValue {
    Bool(bool),
}

pub struct LapceSettings {
    children: Vec<WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
}

impl LapceSettings {
    pub fn new(data: &LapceTabData) -> Box<dyn Widget<LapceTabData>> {
        let settings = Self {
            children: Vec::new(),
        };

        let input = LapceEditorView::new(data.settings.settings_view_id)
            .hide_header()
            .hide_gutter()
            .padding((15.0, 15.0));

        let split = LapceSplitNew::new(data.settings.settings_split_id)
            .horizontal()
            .with_child(input.boxed(), None, 55.0)
            .with_flex_child(settings.boxed(), None, 1.0);

        split.boxed()
    }

    fn update_children(&mut self) {
        self.children.clear();
    }

    fn paint_setting(
        &self,
        ctx: &mut PaintCtx,
        name: &str,
        description: &str,
        value: &serde_json::Value,
        width: f64,
        y: f64,
        config: &Config,
    ) -> f64 {
        let mut y = y;
        y += 20.0 + 10.0;

        let text_layout = ctx
            .text()
            .new_text_layout(name.to_string())
            .font(FontFamily::SYSTEM_UI, 14.0)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .max_width(width)
            .build()
            .unwrap();
        let text_size = text_layout.size();
        ctx.draw_text(&text_layout, Point::new(10.0, y));
        y += text_size.height + 10.0;

        let text_layout = ctx
            .text()
            .new_text_layout(description.to_string())
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .max_width(width)
            .build()
            .unwrap();
        let text_size = text_layout.size();
        y += 10.0;
        ctx.draw_text(&text_layout, Point::new(10.0, y));
        y += text_size.height + 10.0;

        let text_layout = ctx
            .text()
            .new_text_layout(serde_json::to_string(value).unwrap())
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .max_width(width)
            .build()
            .unwrap();
        let text_size = text_layout.size();
        y += 10.0;
        ctx.draw_text(&text_layout, Point::new(10.0, y));
        y += text_size.height + 10.0;
        y
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
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        let mut y = 0.0;

        let fileds = LapceConfig::FIELDS;
        let descs = LapceConfig::DESCS;
        let lapce_config: HashMap<String, serde_json::Value> =
            serde_json::from_value(
                serde_json::to_value(&data.config.lapce).unwrap(),
            )
            .unwrap();
        for (i, field) in fileds.into_iter().enumerate() {
            y = self.paint_setting(
                ctx,
                field,
                descs[i],
                lapce_config.get(&field.replace("_", "-")).unwrap(),
                size.width,
                y,
                &data.config,
            );
        }
    }
}

pub struct LapceSettingsItem {}

impl Widget<LapceTabData> for LapceSettingsItem {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {}
}
