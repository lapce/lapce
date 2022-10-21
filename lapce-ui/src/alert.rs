use std::sync::Arc;

use druid::{
    piet::{PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, FontWeight, LayoutCtx, LifeCycle,
    LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect, RenderContext, Size, Target,
    TextAlignment, UpdateCtx, Widget, WidgetId, WidgetPod,
};
use lapce_core::command::FocusCommand;
use lapce_data::{
    alert::AlertFocusData,
    command::{
        CommandKind, LapceCommand, LapceUICommand, LAPCE_COMMAND, LAPCE_UI_COMMAND,
    },
    config::{LapceIcons, LapceTheme},
    data::LapceTabData,
};

pub struct AlertBox {
    content: WidgetPod<LapceTabData, AlertBoxContent>,
}

impl AlertBox {
    pub fn new(data: &LapceTabData) -> Self {
        let content = AlertBoxContent::new(data);
        Self {
            content: WidgetPod::new(content),
        }
    }
}

impl Widget<LapceTabData> for AlertBox {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        if !data.alert.active {
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
        if !data.alert.active {
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

pub struct AlertBoxContent {
    widget_id: WidgetId,

    width: f64,
    padding: f64,
    svg_size: f64,
    button_height: f64,

    svg_rect: Rect,
    cancel_rect: Rect,
    buttons: Vec<Rect>,
    title_layout: Option<PietTextLayout>,
    title_origin: Point,
    msg_layout: Option<PietTextLayout>,
    msg_origin: Point,

    mouse_down_point: Point,
}

impl AlertBoxContent {
    pub fn new(data: &LapceTabData) -> Self {
        Self {
            widget_id: data.alert.widget_id,
            width: 250.0,
            padding: 20.0,
            svg_size: 50.0,
            button_height: 30.0,
            svg_rect: Rect::ZERO,
            cancel_rect: Rect::ZERO,
            buttons: Vec::new(),
            title_layout: None,
            title_origin: Point::ZERO,
            msg_layout: None,
            msg_origin: Point::ZERO,
            mouse_down_point: Point::ZERO,
        }
    }

    fn icon_hit_test(&self, mouse_event: &MouseEvent) -> bool {
        for rect in self.buttons.iter() {
            if rect.contains(mouse_event.pos) {
                return true;
            }
        }
        if self.cancel_rect.contains(mouse_event.pos) {
            return true;
        }
        false
    }
}

impl Widget<LapceTabData> for AlertBoxContent {
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
                let mut focus = AlertFocusData::new(data);
                Arc::make_mut(&mut data.keypress)
                    .key_down(ctx, key_event, &mut focus, env);
            }
            Event::MouseMove(mouse_event) => {
                if self.icon_hit_test(mouse_event) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
                }
            }
            Event::MouseDown(mouse_event) => {
                self.mouse_down_point = mouse_event.pos;
            }
            Event::MouseUp(mouse_event) => {
                if self.cancel_rect.contains(self.mouse_down_point)
                    && self.cancel_rect.contains(mouse_event.pos)
                {
                    ctx.submit_command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::ModalClose),
                            data: None,
                        },
                        Target::Widget(self.widget_id),
                    ));
                    ctx.set_handled();
                    return;
                }

                for (i, rect) in self.buttons.iter().enumerate() {
                    if rect.contains(self.mouse_down_point)
                        && rect.contains(mouse_event.pos)
                    {
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            data.alert.content.buttons[i].2.clone(),
                            Target::Widget(data.alert.content.buttons[i].1),
                        ));
                        ctx.submit_command(Command::new(
                            LAPCE_COMMAND,
                            LapceCommand {
                                kind: CommandKind::Focus(FocusCommand::ModalClose),
                                data: None,
                            },
                            Target::Widget(self.widget_id),
                        ));
                        ctx.set_handled();
                        return;
                    }
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_COMMAND);
                if let CommandKind::Focus(FocusCommand::ModalClose) = &command.kind {
                    let alert = Arc::make_mut(&mut data.alert);
                    alert.active = false;
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
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        self.svg_rect = Rect::ZERO
            .with_origin(Point::new(
                self.width / 2.0,
                self.padding + self.svg_size / 2.0,
            ))
            .inflate(self.svg_size / 2.0, self.svg_size / 2.0);

        let title_layout = ctx
            .text()
            .new_text_layout(data.alert.content.title.clone())
            .font(
                data.config.ui.font_family(),
                data.config.ui.font_size() as f64,
            )
            .default_attribute(TextAttribute::Weight(FontWeight::BOLD))
            .alignment(TextAlignment::Center)
            .set_line_height(1.2)
            .max_width(self.width - self.padding * 2.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        let title_size = title_layout.size();
        self.title_origin =
            Point::new(self.padding, self.padding * 2.0 + self.svg_size);

        let msg_layout = ctx
            .text()
            .new_text_layout(data.alert.content.msg.clone())
            .font(
                data.config.ui.font_family(),
                (data.config.ui.font_size() - 1) as f64,
            )
            .alignment(TextAlignment::Center)
            .set_line_height(1.2)
            .max_width(self.width - self.padding * 2.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();
        self.msg_origin = Point::new(
            self.padding,
            self.padding * 2.0
                + self.svg_size
                + title_size.height
                + self.padding / 2.0,
        );

        let mut y = self.msg_origin.y + msg_layout.size().height + self.padding;
        self.buttons.clear();
        for _ in data.alert.content.buttons.iter() {
            let rect = Rect::ZERO
                .with_origin(Point::new(
                    self.width / 2.0,
                    y + self.button_height / 2.0,
                ))
                .inflate(self.width / 2.0 - self.padding, self.button_height / 2.0);
            self.buttons.push(rect);
            y += self.button_height + self.padding / 2.0;
        }

        y += self.padding / 2.0;
        self.cancel_rect = Rect::ZERO
            .with_origin(Point::new(self.width / 2.0, y + self.button_height / 2.0))
            .inflate(self.width / 2.0 - self.padding, self.button_height / 2.0);

        self.title_layout = Some(title_layout);
        self.msg_layout = Some(msg_layout);

        Size::new(self.width, y + self.button_height + self.padding)
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

        let svg = data.config.ui_svg(LapceIcons::WARNING);
        ctx.draw_svg(
            &svg,
            self.svg_rect,
            Some(data.config.get_color_unchecked(LapceTheme::LAPCE_WARN)),
        );

        ctx.draw_text(self.title_layout.as_ref().unwrap(), self.title_origin);
        ctx.draw_text(self.msg_layout.as_ref().unwrap(), self.msg_origin);

        for (i, (text, _, _)) in data.alert.content.buttons.iter().enumerate() {
            ctx.stroke(
                self.buttons[i],
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
            let text_layout = ctx
                .text()
                .new_text_layout(text.to_string())
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
            let text_layout_size = text_layout.size();
            let point = self.buttons[i].center()
                - (text_layout_size.width / 2.0, text_layout.cap_center());
            ctx.draw_text(&text_layout, point);
        }

        ctx.stroke(
            self.cancel_rect,
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            1.0,
        );
        let text_layout = ctx
            .text()
            .new_text_layout("Cancel")
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
        let text_layout_size = text_layout.size();
        let cancel_point = self.cancel_rect.center()
            - (text_layout_size.width / 2.0, text_layout.cap_center());
        ctx.draw_text(&text_layout, cancel_point);
    }
}
