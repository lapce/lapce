use crate::scroll::LapceScroll;
use druid::{
    piet::{Text, TextAttribute, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Cursor, Env, Event, EventCtx, FontWeight, LayoutCtx,
    LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, RenderContext, Size,
    UpdateCtx, Widget, WidgetExt, WidgetId,
};
use lapce_data::{config::LapceTheme, data::LapceTabData, panel::PanelKind};
use lapce_rpc::plugin::PluginDescription;
use strum_macros::Display;

use crate::panel::{LapcePanel, PanelHeaderKind};

#[derive(Display, PartialEq)]
enum PluginStatus {
    Installed,
    Install,
    Upgrade,
}

pub struct Plugin {
    line_height: f64,
}

impl Plugin {
    pub fn new() -> Self {
        Self { line_height: 25.0 }
    }

    pub fn new_panel(data: &LapceTabData) -> LapcePanel {
        let split_id = WidgetId::next();
        LapcePanel::new(
            PanelKind::Plugin,
            data.plugin.widget_id,
            split_id,
            vec![(
                split_id,
                PanelHeaderKind::None,
                LapceScroll::new(Self::new()).boxed(),
                None,
            )],
        )
    }

    fn hit_test<'a>(
        &self,
        ctx: &mut EventCtx,
        data: &'a LapceTabData,
        mouse_event: &MouseEvent,
    ) -> Option<(&'a PluginDescription, PluginStatus)> {
        let index = (mouse_event.pos.y / (self.line_height * 3.0)) as usize;
        let plugin = data.plugins.get(index)?;
        let status = match data
            .installed_plugins
            .get(&plugin.name)
            .map(|installed| plugin.version.clone() == installed.version.clone())
        {
            Some(true) => PluginStatus::Installed,
            Some(false) => PluginStatus::Upgrade,
            None => PluginStatus::Install,
        };

        if status == PluginStatus::Installed {
            return None;
        }

        let padding = 10.0;
        let text_padding = 5.0;

        let text_layout = ctx
            .text()
            .new_text_layout(status.to_string())
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .build()
            .unwrap();

        let text_size = text_layout.size();
        let x = ctx.size().width - text_size.width - text_padding * 2.0 - padding;
        let y = 3.0 * self.line_height * index as f64 + self.line_height * 2.0;
        let rect = Size::new(text_size.width + text_padding * 2.0, self.line_height)
            .to_rect()
            .with_origin(Point::new(x, y));
        if rect.contains(mouse_event.pos) {
            Some((plugin, status))
        } else {
            None
        }
    }
}

impl Default for Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget<LapceTabData> for Plugin {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                if self.hit_test(ctx, data, mouse_event).is_some() {
                    ctx.set_cursor(&Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
            }
            Event::MouseDown(mouse_event) => {
                if let Some((plugin, _)) = self.hit_test(ctx, data, mouse_event) {
                    data.proxy.install_plugin(plugin);
                }
            }
            _ => (),
        }
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
        if _data.plugins.is_empty() {
            return bc.max();
        }
        let height = 3.0 * self.line_height * _data.plugins.len() as f64;
        let height = height.max(bc.max().height);

        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let size = ctx.size();
        let padding = 10.0;

        ctx.with_save(|ctx| {
            let viewport = ctx.size().to_rect().inflate(-padding, 0.0);
            ctx.clip(viewport);

            if data.plugins.is_empty() {
                let y = self.line_height;
                let x = self.line_height;
                let layout = ctx
                    .text()
                    .new_text_layout("Failed to load plugin information")
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .default_attribute(TextAttribute::Weight(FontWeight::SEMI_BOLD))
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_WARN)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(&layout, Point::new(x, y));
                return;
            }

            for (i, plugin) in data.plugins.iter().enumerate() {
                let y = 3.0 * self.line_height * i as f64;
                let x = 3.0 * self.line_height;
                let text_layout = ctx
                    .text()
                    .new_text_layout(plugin.display_name.clone())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOCUS)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        x,
                        y + (self.line_height - text_layout.size().height) / 2.0,
                    ),
                );

                let text_layout = ctx
                    .text()
                    .new_text_layout(plugin.description.clone())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        x,
                        y + self.line_height
                            + (self.line_height - text_layout.size().height) / 2.0,
                    ),
                );

                let text_layout = ctx
                    .text()
                    .new_text_layout(plugin.author.clone())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        x,
                        y + self.line_height * 2.0
                            + (self.line_height - text_layout.size().height) / 2.0,
                    ),
                );

                let status = match data
                    .installed_plugins
                    .get(&plugin.name)
                    .map(|installed| installed.version == plugin.version)
                {
                    Some(true) => PluginStatus::Installed,
                    Some(false) => PluginStatus::Upgrade,
                    None => PluginStatus::Install,
                };

                let text_layout = ctx
                    .text()
                    .new_text_layout(status.to_string())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64,
                    )
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                            .clone(),
                    )
                    .build()
                    .unwrap();
                let text_size = text_layout.size();
                let text_padding = 5.0;
                let x = size.width - text_size.width - text_padding * 2.0 - padding;
                let y = y + self.line_height * 2.0;
                let color = if status == PluginStatus::Installed {
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_DIM)
                        .clone()
                } else {
                    Color::rgb8(80, 161, 79)
                };
                ctx.fill(
                    Size::new(
                        text_size.width + text_padding * 2.0,
                        self.line_height,
                    )
                    .to_rect()
                    .with_origin(Point::new(x, y)),
                    &color,
                );
                ctx.draw_text(
                    &text_layout,
                    Point::new(
                        x + text_padding,
                        y + (self.line_height - text_layout.size().height) / 2.0,
                    ),
                );
            }
        });
    }
}
