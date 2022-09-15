use crate::{panel::PanelSizing, scroll::LapceScroll, svg::logo_svg};
use druid::{
    kurbo::Line,
    piet::{PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Cursor, Env, Event, EventCtx, FontWeight,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetExt, WidgetId,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    command::{CommandKind, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND},
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
    gap: f64,
    height: f64,
}

impl Plugin {
    pub fn new(installed: bool) -> Self {
        Self {
            line_height: 25.0,
            width: 0.0,
            height: 0.0,
            installed,
            rects: Vec::new(),
            gap: 10.0,
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

    /// Return the modifier that should be multiplied by the plugin number
    /// to get its initial y position
    fn plugin_y_mod(&self) -> f64 {
        3.5 * self.line_height
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_plugin(
        &mut self,
        ctx: &mut PaintCtx,
        i: usize,
        display_name: &str,
        description: &str,
        author: &str,
        version: &str,
        status: PluginStatus,
        config: &Config,
    ) -> Rect {
        let y = (3.0 * self.line_height + self.gap) * i as f64 + self.gap / 2.0;
        let x = 3.0 * self.line_height;

        let svg = logo_svg();
        ctx.draw_svg(
            &svg,
            Rect::ZERO
                .with_origin(Point::new(x / 2.0, y + self.line_height))
                .inflate(self.line_height * 0.75, self.line_height * 0.75),
            Some(config.get_color_unchecked(LapceTheme::EDITOR_DIM)),
        );

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

        let version_x = x + text_layout.size().width + 8.0;
        let text_layout = ctx
            .text()
            .new_text_layout(format!("v{version}"))
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .default_attribute(TextAttribute::Weight(FontWeight::NORMAL))
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(
            &text_layout,
            Point::new(version_x, y + text_layout.y_offset(self.line_height)),
        );

        // display description
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

        if status == PluginStatus::Install {
            let text_layout = ctx
                .text()
                .new_text_layout(status.to_string())
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
            let rect =
                Size::new(text_size.width + text_padding * 2.0, self.line_height)
                    .to_rect()
                    .with_origin(Point::new(x, y));
            ctx.fill(rect, &color);
            ctx.draw_text(
                &text_layout,
                Point::new(
                    x + text_padding,
                    y + text_layout.y_offset(self.line_height),
                ),
            );
            rect
        } else {
            let color = match status {
                PluginStatus::Installed => LapceTheme::EDITOR_FOCUS,
                PluginStatus::Upgrade(_) => LapceTheme::LAPCE_WARN,
                _ => LapceTheme::EDITOR_DIM,
            };

            let status_x = text_layout.size().width + 20.0;
            let text_layout = ctx
                .text()
                .new_text_layout(format!("[ {status} ]"))
                .font(config.ui.font_family(), config.ui.font_size() as f64)
                .text_color(config.get_color_unchecked(color).clone())
                .build()
                .unwrap();
            ctx.draw_text(
                &text_layout,
                Point::new(
                    status_x,
                    y + self.line_height * 2.0
                        + text_layout.y_offset(self.line_height),
                ),
            );
            // if status is [installed, disabled, upgrade(x)], display the settings.svg
            let x = self.width - 24.0;
            let y = y + self.line_height * 2.2;
            let rect = Size::new(15.0, 15.0)
                .to_rect()
                .with_origin(Point::new(x, y));
            ctx.draw_svg(
                &get_svg("settings.svg").unwrap(),
                rect,
                Some(
                    &config
                        .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                        .clone(),
                ),
            );
            rect
        }
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
                &volt.version,
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
                        &volt.version,
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
        let index =
            (mouse_event.pos.y / (self.line_height * 3.0 + self.gap)) as usize;
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
                if mouse_event.pos.y <= self.height {
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
                    } else if mouse_event.pos.y <= self.height {
                        let index = (mouse_event.pos.y
                            / (self.line_height * 3.0 + self.gap))
                            as usize;
                        if self.installed {
                            if let Some((_, volt)) =
                                data.plugin.installed.get_index(index)
                            {
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::OpenPluginInfo(volt.info()),
                                    Target::Widget(data.id),
                                ));
                            }
                        } else {
                            let mut i = 0;
                            for (id, volt) in data.plugin.volts.volts.iter() {
                                if data.plugin.installed.contains_key(id) {
                                    continue;
                                }
                                if i == index {
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::OpenPluginInfo(volt.clone()),
                                        Target::Widget(data.id),
                                    ));
                                    break;
                                }
                                i += 1;
                            }
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

        self.height = (3.0 * self.line_height + self.gap) * len as f64;
        self.width = bc.max().width;
        Size::new(bc.max().width, bc.max().height.max(self.height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if self.installed {
            self.paint_installed(ctx, data);
        } else {
            self.paint_available(ctx, data);
        }
    }
}

pub struct PluginInfo {
    widget_id: WidgetId,
    editor_tab_id: WidgetId,
    volt_id: String,

    padding: f64,
    gap: f64,
    name_text_layout: Option<PietTextLayout>,
    desc_text_layout: Option<PietTextLayout>,
    author_text_layout: Option<PietTextLayout>,
    button_text_layout: Option<PietTextLayout>,
    icon_width: f64,
    title_width: f64,
}

impl PluginInfo {
    fn new(widget_id: WidgetId, editor_tab_id: WidgetId, volt_id: String) -> Self {
        Self {
            widget_id,
            editor_tab_id,
            volt_id,
            padding: 50.0,
            gap: 30.0,
            name_text_layout: None,
            desc_text_layout: None,
            author_text_layout: None,
            button_text_layout: None,
            icon_width: 0.0,
            title_width: 0.0,
        }
    }

    pub fn new_scroll(
        widget_id: WidgetId,
        editor_tab_id: WidgetId,
        volt_id: String,
    ) -> LapceScroll<LapceTabData, PluginInfo> {
        LapceScroll::new(PluginInfo::new(widget_id, editor_tab_id, volt_id))
    }
}

impl Widget<LapceTabData> for PluginInfo {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let cmd = cmd.get_unchecked(LAPCE_COMMAND);
                if let CommandKind::Focus(FocusCommand::SplitClose) = &cmd.kind {
                    data.main_split.widget_close(
                        ctx,
                        self.widget_id,
                        self.editor_tab_id,
                    );
                }
            }
            _ => {}
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
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let width = if let Some(volt) = data.plugin.volts.volts.get(&self.volt_id) {
            self.name_text_layout = Some(
                ctx.text()
                    .new_text_layout(volt.display_name.clone())
                    .font(
                        data.config.ui.font_family(),
                        data.config.ui.font_size() as f64 * 1.5,
                    )
                    .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
                    .text_color(
                        data.config
                            .get_color_unchecked(LapceTheme::EDITOR_FOCUS)
                            .clone(),
                    )
                    .build()
                    .unwrap(),
            );
            self.desc_text_layout = Some(
                ctx.text()
                    .new_text_layout(volt.description.clone())
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
                    .unwrap(),
            );
            self.author_text_layout = Some(
                ctx.text()
                    .new_text_layout(volt.author.clone())
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
                    .unwrap(),
            );
            let status = data.plugin.plugin_status(&self.volt_id);
            let text = if status == PluginStatus::Install {
                status.to_string()
            } else {
                format!("{} ▼", status)
            };
            self.button_text_layout = Some(
                ctx.text()
                    .new_text_layout(text)
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
                    .unwrap(),
            );

            self.icon_width = self.name_text_layout.as_ref().unwrap().size().height
                * 2.0
                + self.desc_text_layout.as_ref().unwrap().size().height * 2.0 * 3.0;
            self.title_width = self
                .name_text_layout
                .as_ref()
                .unwrap()
                .size()
                .width
                .max(self.desc_text_layout.as_ref().unwrap().size().width);
            self.padding * 4.0 + self.icon_width + self.title_width
        } else {
            0.0
        };
        Size::new(bc.max().width.max(width), bc.max().height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        if let Some(name_text_layout) = self.name_text_layout.as_ref() {
            let width = self.icon_width + self.title_width + self.padding * 4.0;
            let width = width.max(740.0);
            let width = width.min(ctx.size().width - self.padding * 2.0);
            let padding = (ctx.size().width - width) / 2.0;

            let mut y = 30.0;
            let size = name_text_layout.size();
            let name_y = y;
            y += size.height * 2.0;

            let desc_text_layout = self.desc_text_layout.as_ref().unwrap();
            let size = desc_text_layout.size();
            let desc_y = y;
            y += size.height * 2.0;

            let author_text_layout = self.author_text_layout.as_ref().unwrap();
            let size = author_text_layout.size();
            let author_y = y;
            y += size.height * 2.0;

            let button_text_layout = self.button_text_layout.as_ref().unwrap();
            let size = button_text_layout.size();
            let button_y = y;

            let icon_rect = Rect::ZERO
                .with_origin(Point::new(
                    padding + self.padding + self.icon_width / 2.0,
                    name_y + self.icon_width / 2.0,
                ))
                .inflate(self.icon_width / 2.0 * 0.8, self.icon_width / 2.0 * 0.8);
            ctx.draw_svg(
                &logo_svg(),
                icon_rect,
                Some(data.config.get_color_unchecked(LapceTheme::EDITOR_DIM)),
            );

            ctx.draw_text(
                name_text_layout,
                Point::new(
                    padding + self.padding + self.icon_width,
                    name_y + name_text_layout.y_offset(size.height * 2.0),
                ),
            );
            ctx.draw_text(
                desc_text_layout,
                Point::new(
                    padding + self.padding + self.icon_width,
                    desc_y + desc_text_layout.y_offset(size.height * 2.0),
                ),
            );
            ctx.draw_text(
                author_text_layout,
                Point::new(
                    padding + self.padding + self.icon_width,
                    author_y + author_text_layout.y_offset(size.height * 2.0),
                ),
            );
            let rect = Size::new(size.width + 5.0 * 2.0, 0.0)
                .to_rect()
                .with_origin(Point::new(
                    padding + self.padding + self.icon_width,
                    button_y + size.height,
                ))
                .inflate(0.0, size.height);
            ctx.fill(rect, &Color::rgb8(80, 161, 79));
            ctx.draw_text(
                button_text_layout,
                Point::new(
                    padding + self.padding + 5.0 + self.icon_width,
                    button_y + button_text_layout.y_offset(size.height * 2.0),
                ),
            );

            y += size.height * 2.0;
            y += 30.0;

            let line = Line::new(
                Point::new(padding, y + 0.5),
                Point::new(ctx.size().width - padding, y + 0.5),
            );
            ctx.stroke(
                line,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            )
        }
    }
}
