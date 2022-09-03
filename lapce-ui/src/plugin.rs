use crate::{panel::PanelSizing, scroll::LapceScroll};
use druid::{
    piet::{Text, TextAttribute, TextLayout as PietTextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Cursor, Env, Event, EventCtx, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    data::{LapceData, LapceTabData},
    panel::PanelKind,
    plugin::{PluginData, PluginLoadStatus, PluginStatus},
};

use crate::panel::{LapcePanel, PanelHeaderKind};

pub struct Plugin {
    line_height: f64,
    width: f64,
    installed: bool,
    rects: Vec<(usize, Rect, PluginStatus)>,
}

impl Plugin {
    pub fn new(installed: bool) -> Self {
        Self {
            line_height: 25.0,
            width: 0.0,
            installed,
            rects: Vec::new(),
        }
    }

    pub fn new_panel(data: &LapceTabData) -> LapcePanel {
        let split_id = WidgetId::next();
        LapcePanel::new(
            PanelKind::Plugin,
            data.plugin.widget_id,
            split_id,
            vec![
                (
                    data.plugin.installed_id,
                    PanelHeaderKind::Simple("Installed".into()),
                    LapceScroll::new(Self::new(true)).boxed(),
                    PanelSizing::Flex(true),
                ),
                (
                    data.plugin.uninstalled_id,
                    PanelHeaderKind::Simple("Available".into()),
                    LapceScroll::new(Self::new(false)).boxed(),
                    PanelSizing::Flex(true),
                ),
            ],
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_plugin(
        &mut self,
        ctx: &mut PaintCtx,
        i: usize,
        display_name: &str,
        description: &str,
        author: &str,
        status: PluginStatus,
        config: &Config,
    ) -> Rect {
        let y = 3.0 * self.line_height * i as f64;
        let x = 3.0 * self.line_height;
        let text_layout = ctx
            .text()
            .new_text_layout(display_name.to_string())
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .text_color(config.get_color_unchecked(LapceTheme::EDITOR_FOCUS).clone())
            .build()
            .unwrap();
        ctx.draw_text(
            &text_layout,
            Point::new(x, y + text_layout.y_offset(self.line_height)),
        );
        let text_layout = ctx
            .text()
            .new_text_layout(description.to_string())
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        // check if text is longer than plugin panel. If so, add ellipsis after description.
        if text_layout.layout.width() > (self.width - x - 15.0) as f32 {
            let hit_point =
                text_layout.hit_test_point(Point::new(self.width - x - 15.0, 0.0));
            let end = description
                .char_indices()
                .filter(|(i, _)| hit_point.idx.overflowing_sub(*i).0 < 4)
                .collect::<Vec<(usize, char)>>();
            let end = if end.is_empty() {
                description.len()
            } else {
                end[0].0
            };
            let description = format!("{}...", (&description[0..end]));
            let text_layout = ctx
                .text()
                .new_text_layout(description)
                .font(config.ui.font_family(), config.ui.font_size() as f64)
                .text_color(
                    config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                )
                .build()
                .unwrap();
            ctx.draw_text(
                &text_layout,
                Point::new(
                    x,
                    y + self.line_height + text_layout.y_offset(self.line_height),
                ),
            );
        } else {
            ctx.draw_text(
                &text_layout,
                Point::new(
                    x,
                    y + self.line_height + text_layout.y_offset(self.line_height),
                ),
            );
        }

        let text_layout = ctx
            .text()
            .new_text_layout(author.to_string())
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(
            &text_layout,
            Point::new(
                x,
                y + self.line_height * 2.0 + text_layout.y_offset(self.line_height),
            ),
        );

        let size = ctx.size();
        let padding = 10.0;

        let text = if status == PluginStatus::Install {
            status.to_string()
        } else {
            format!("{} ▼", status)
        };
        let text_layout = ctx
            .text()
            .new_text_layout(text)
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let text_size = text_layout.size();
        let text_padding = 5.0;
        let x = size.width - text_size.width - text_padding * 2.0 - padding;
        let y = y + self.line_height * 2.0;
        let color = Color::rgb8(80, 161, 79);
        let rect = Size::new(text_size.width + text_padding * 2.0, self.line_height)
            .to_rect()
            .with_origin(Point::new(x, y));
        ctx.fill(rect, &color);
        ctx.draw_text(
            &text_layout,
            Point::new(x + text_padding, y + text_layout.y_offset(self.line_height)),
        );
        rect
    }

    fn paint_installed(&mut self, ctx: &mut PaintCtx, data: &LapceTabData) {
        self.rects.clear();
        for (i, (id, volt)) in data.plugin.installed.iter().enumerate() {
            let status = data.plugin.plugin_status(id);
            let rect = self.paint_plugin(
                ctx,
                i,
                &volt.display_name,
                &volt.description,
                &volt.author,
                status.clone(),
                &data.config,
            );
            self.rects.push((i, rect, status));
        }
    }

    fn paint_available(&mut self, ctx: &mut PaintCtx, data: &LapceTabData) {
        self.rects.clear();
        match data.plugin.volts.status {
            PluginLoadStatus::Loading => {
                let y = self.line_height;
                let x = self.line_height;
                let layout = ctx
                    .text()
                    .new_text_layout("Loading plugin information...")
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
            }
            PluginLoadStatus::Failed => {
                let y = self.line_height;
                let x = self.line_height;
                let layout = ctx
                    .text()
                    .new_text_layout("Failed to load plugin information.")
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
            }
            PluginLoadStatus::Success => {
                let mut i = 0;
                for (index, (id, volt)) in data.plugin.volts.volts.iter().enumerate()
                {
                    if data.plugin.installed.contains_key(id) {
                        continue;
                    }
                    let status = data.plugin.plugin_status(id);
                    let rect = self.paint_plugin(
                        ctx,
                        i,
                        &volt.display_name,
                        &volt.description,
                        &volt.author,
                        status.clone(),
                        &data.config,
                    );
                    self.rects.push((index, rect, status));
                    i += 1;
                }
            }
        }
    }

    fn hit_test<'a>(
        &'a self,
        mouse_event: &MouseEvent,
    ) -> Option<(usize, &'a PluginStatus)> {
        let index = (mouse_event.pos.y / (self.line_height * 3.0)) as usize;
        let (i, rect, status) = self.rects.get(index)?;
        if rect.contains(mouse_event.pos) {
            Some((*i, status))
        } else {
            None
        }
    }
}

impl Default for Plugin {
    fn default() -> Self {
        Self::new(true)
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
                if self.hit_test(mouse_event).is_some() {
                    ctx.set_cursor(&Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
            }
            Event::MouseDown(mouse_event) => {
                if mouse_event.button.is_left() {
                    if let Some((index, status)) = self.hit_test(mouse_event) {
                        if !self.installed {
                            if let Some((_, volt)) =
                                data.plugin.volts.volts.get_index(index)
                            {
                                let _ = PluginData::install_volt(
                                    data.proxy.clone(),
                                    volt.clone(),
                                );
                            }
                        } else if let Some((id, meta)) =
                            data.plugin.installed.get_index(index)
                        {
                            let mut menu = druid::Menu::<LapceData>::new("Plugin");

                            if let PluginStatus::Upgrade(meta_link) = status {
                                let mut info = meta.info();
                                info.meta = meta_link.clone();
                                let proxy = data.proxy.clone();
                                let item = druid::MenuItem::new("Upgrade Plugin")
                                    .on_activate(move |_ctx, _data, _env| {
                                        let _ = PluginData::install_volt(
                                            proxy.clone(),
                                            info.clone(),
                                        );
                                    });
                                menu = menu.entry(item);
                                menu = menu.separator();
                            }

                            let local_volt = meta.info();
                            let tab_id = data.id;
                            let item = druid::MenuItem::new("Enable")
                                .on_activate(move |ctx, _data, _env| {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::EnableVolt(
                                            local_volt.clone(),
                                        ),
                                        Target::Widget(tab_id),
                                    ));
                                })
                                .enabled(data.plugin.disabled.contains(id));
                            menu = menu.entry(item);

                            let local_volt = meta.info();
                            let tab_id = data.id;
                            let item = druid::MenuItem::new("Disable")
                                .on_activate(move |ctx, _data, _env| {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::DisableVolt(
                                            local_volt.clone(),
                                        ),
                                        Target::Widget(tab_id),
                                    ));
                                })
                                .enabled(!data.plugin.disabled.contains(id));
                            menu = menu.entry(item);

                            menu = menu.separator();

                            let local_volt = meta.info();
                            let tab_id = data.id;
                            let item = druid::MenuItem::new("Enable For Workspace")
                                .on_activate(move |ctx, _data, _env| {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::EnableVoltWorkspace(
                                            local_volt.clone(),
                                        ),
                                        Target::Widget(tab_id),
                                    ));
                                })
                                .enabled(
                                    data.plugin.workspace_disabled.contains(id),
                                );
                            menu = menu.entry(item);

                            let local_volt = meta.info();
                            let tab_id = data.id;
                            let item = druid::MenuItem::new("Disable For Workspace")
                                .on_activate(move |ctx, _data, _env| {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::DisableVoltWorkspace(
                                            local_volt.clone(),
                                        ),
                                        Target::Widget(tab_id),
                                    ));
                                })
                                .enabled(
                                    !data.plugin.workspace_disabled.contains(id),
                                );
                            menu = menu.entry(item);

                            let local_meta = meta.clone();
                            let proxy = data.proxy.clone();
                            let item = druid::MenuItem::new("Uninstall")
                                .on_activate(
                                    move |_ctx, _data: &mut LapceData, _env| {
                                        let _ = PluginData::remove_volt(
                                            proxy.clone(),
                                            local_meta.clone(),
                                        );
                                    },
                                );
                            menu = menu.separator().entry(item);
                            ctx.show_context_menu::<LapceData>(
                                menu,
                                ctx.to_window(mouse_event.pos),
                            )
                        }
                    }
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
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let len = if self.installed {
            data.plugin.installed.len()
        } else {
            data.plugin
                .volts
                .volts
                .iter()
                .filter(|(id, _)| !data.plugin.installed.contains_key(*id))
                .count()
        };

        let height = 3.0 * self.line_height * len as f64;
        let height = height.max(bc.max().height);
        self.width = bc.max().width;
        Size::new(bc.max().width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if self.installed {
            self.paint_installed(ctx, data);
        } else {
            self.paint_available(ctx, data);
        }
    }
}
