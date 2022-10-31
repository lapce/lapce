use std::sync::Arc;

use druid::{
    piet::{Text, TextAttribute, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontWeight, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    TextAlignment, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_core::{command::FocusCommand, meta};
use lapce_data::{
    about::AboutFocusData,
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::LapceTabData,
};

struct AboutUri {}

impl AboutUri {
    const LAPCE: &str = "https://lapce.dev";
    const GITHUB: &str = "https://github.com/lapce/lapce";
    const MATRIX: &str = "https://matrix.to/#/#lapce-editor:matrix.org";
    const DISCORD: &str = "https://discord.gg/n8tGJ6Rn6D";
    const CODICONS: &str = "https://github.com/microsoft/vscode-codicons";
}

pub struct AboutBox {
    content: WidgetPod<LapceTabData, AboutBoxContent>,
}

impl AboutBox {
    pub fn new(data: &LapceTabData) -> Self {
        let content = AboutBoxContent::new(data);
        Self {
            content: WidgetPod::new(content),
        }
    }
}

impl Widget<LapceTabData> for AboutBox {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        if !data.about.active {
            return;
        }
        self.content.event(ctx, event, data, env);
        if !event.should_propagate_to_hidden() {
            ctx.set_handled();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.content.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        self.content.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let self_size = bc.max();
        let size = self.content.layout(ctx, bc, data, env);
        let origin = Point::new(
            (self_size.width - size.width) / 2.0,
            (self_size.height - size.height) / 2.0,
        );
        self.content.set_origin(ctx, data, env, origin);

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        if !data.about.active {
            return;
        }
        let rect = ctx.size().to_rect();
        ctx.fill(
            rect,
            &data
                .config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW)
                .clone()
                .with_alpha(0.5),
        );

        self.content.paint(ctx, data, env);
    }
}

pub struct AboutBoxContent {
    mouse_pos: Option<Point>,
    widget_id: WidgetId,

    width: f64,
    height: f64,
    padding: f64,
    svg_size: f64,

    close_rect: Option<Rect>,

    commands: Vec<(Rect, Command)>,
    mouse_down_point: Option<Point>,
}

impl AboutBoxContent {
    pub fn new(data: &LapceTabData) -> Self {
        Self {
            mouse_pos: None,
            widget_id: data.about.widget_id,
            width: 320.0,
            height: 384.0,
            padding: 40.0,
            svg_size: 80.0,
            close_rect: None,
            commands: vec![],
            mouse_down_point: None,
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for (rect, _) in self.commands.iter() {
            if rect.contains(mouse_event.pos) {
                return true;
            }
        }
        if self
            .close_rect
            .map(|c| c.contains(mouse_event.pos))
            .unwrap_or(false)
        {
            return true;
        }
        false
    }

    fn mouse_down(&self, ctx: &mut EventCtx, mouse_event: &MouseEvent) {
        for (rect, command) in self.commands.iter() {
            if rect.contains(mouse_event.pos) {
                ctx.submit_command(command.clone());
                ctx.set_handled();
                return;
            }
        }
    }
}

impl Widget<LapceTabData> for AboutBoxContent {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::KeyDown(key_event) => {
                let mut focus = AboutFocusData::new(data);
                Arc::make_mut(&mut data.keypress)
                    .key_down(ctx, key_event, &mut focus, env);
            }
            Event::MouseMove(mouse_event) => {
                self.mouse_pos = Some(mouse_event.pos);
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
                ctx.set_handled();
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down_point = Some(mouse_event.pos);
                if mouse_event.button.is_left() {
                    self.mouse_down(ctx, mouse_event);
                }
                ctx.request_paint();
            }
            Event::MouseUp(mouse_event) => {
                if self
                    .close_rect
                    .zip(self.mouse_down_point)
                    .map(|(c, m)| c.contains(m) && c.contains(mouse_event.pos))
                    .unwrap_or(false)
                {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ModalClose),
                            data: None,
                        },
                        Target::Widget(self.widget_id),
                    ));
                }
                self.mouse_down_point = None;
                ctx.request_paint();
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_COMMAND);
                if let CommandKind::Focus(FocusCommand::ModalClose) = &command.kind {
                    let about = Arc::make_mut(&mut data.about);
                    about.active = false;
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::Focus,
                        Target::Widget(*data.focus),
                    ));
                    ctx.set_handled();
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = &command {
                    ctx.request_focus();
                    ctx.set_handled();
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
        _ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        _data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        Size::new(self.width, self.height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let rect = ctx.size().to_rect();
        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        if shadow_width > 0.0 {
            ctx.blurred_rect(
                rect,
                shadow_width,
                data.config
                    .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
            );
        }
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );

        ctx.draw_svg(
            &data.config.logo_svg(),
            Rect::ZERO
                .with_origin(Point::new(
                    self.width / 2.0,
                    self.padding + self.svg_size / 2.0,
                ))
                .inflate(self.svg_size / 2.0, self.svg_size / 2.0),
            Some(data.config.get_color_unchecked(LapceTheme::EDITOR_DIM)),
        );

        let title_padding = 20.0;
        let line_gap = 12.0;
        let mut y = self.padding + self.svg_size + title_padding;

        let title_layout = ctx
            .text()
            .new_text_layout("Lapce")
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .alignment(TextAlignment::Center)
            .max_width(self.width - self.padding * 2.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();

        ctx.draw_text(&title_layout, Point::new(self.padding, y));
        y += title_layout.layout.height() as f64 + line_gap;

        let version_layout = ctx
            .text()
            .new_text_layout(format!("Version: {}", *meta::VERSION))
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .alignment(TextAlignment::Center)
            .max_width(self.width - self.padding * 2.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                    .clone(),
            )
            .build()
            .unwrap();

        ctx.draw_text(&version_layout, Point::new(self.padding, y));
        y += version_layout.size().height + title_padding;

        self.commands.clear();

        for (msg, link) in [
            ("Website", AboutUri::LAPCE),
            ("GitHub", AboutUri::GITHUB),
            ("Discord", AboutUri::DISCORD),
            ("Matrix", AboutUri::MATRIX),
        ] {
            let row_item = ctx
                .text()
                .new_text_layout(msg)
                .font(
                    data.config.ui.font_family(),
                    (data.config.ui.font_size()) as f64,
                )
                .alignment(TextAlignment::Center)
                .max_width(self.width - self.padding * 2.0)
                .text_color(
                    data.config
                        .get_color_unchecked(LapceTheme::EDITOR_LINK)
                        .clone(),
                )
                .build()
                .unwrap();

            let site_rect = Size::new(
                row_item.layout.width() as f64,
                row_item.layout.height() as f64,
            )
            .to_rect()
            .with_origin(Point::new(
                self.width / 2.0 + (row_item.layout.width() as f64 / 2.0)
                    - row_item.layout.width() as f64,
                y,
            ));

            self.commands.push((
                site_rect,
                Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::OpenURI(link.to_string()),
                    Target::Auto,
                ),
            ));

            ctx.draw_text(&row_item, Point::new(self.padding, y));

            y += row_item.size().height + line_gap;
        }

        let row_item = ctx
            .text()
            .new_text_layout("Attributes")
            .font(
                data.config.ui.font_family(),
                (data.config.ui.font_size()) as f64,
            )
            .alignment(TextAlignment::Center)
            .max_width(self.width - self.padding * 2.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_DIM)
                    .clone(),
            )
            .build()
            .unwrap();

        ctx.draw_text(
            &row_item,
            Point::new(
                self.padding,
                rect.y1 - (row_item.layout.height() as f64 + line_gap) * 2.0,
            ),
        );

        let row_item = ctx
            .text()
            .new_text_layout("Codicons (CC-BY-4.0)")
            .font(
                data.config.ui.font_family(),
                (data.config.ui.font_size()) as f64,
            )
            .alignment(TextAlignment::Center)
            .max_width(self.width - self.padding * 2.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_LINK)
                    .clone(),
            )
            .build()
            .unwrap();
        ctx.draw_text(
            &row_item,
            Point::new(
                self.padding,
                rect.y1 - row_item.layout.height() as f64 - line_gap,
            ),
        );

        let site_rect = Size::new(
            row_item.layout.width() as f64,
            row_item.layout.height() as f64,
        )
        .to_rect()
        .with_origin(Point::new(
            self.width / 2.0 + (row_item.layout.width() as f64 / 2.0)
                - row_item.layout.width() as f64,
            rect.y1 - row_item.layout.height() as f64 - line_gap,
        ));

        self.commands.push((
            site_rect,
            Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::OpenURI(AboutUri::CODICONS.to_string()),
                Target::Auto,
            ),
        ));

        let close_rect = Size::new(20.0, 20.0)
            .to_rect()
            .with_origin(Point::new(self.width - 20.0, 0.0));

        if self
            .mouse_pos
            .map(|m| close_rect.contains(m))
            .unwrap_or(false)
        {
            ctx.fill(
                close_rect,
                &data.config.get_hover_color(
                    data.config
                        .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
                ),
            );
        }

        ctx.draw_svg(
            &data.config.ui_svg(LapceIcons::WINDOW_CLOSE),
            close_rect.inflate(-2.5, -2.5),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            ),
        );

        self.close_rect = Some(close_rect);
    }
}
