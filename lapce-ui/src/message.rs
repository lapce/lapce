use druid::{
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    BoxConstraints, Command, Env, Event, EventCtx, FontWeight, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target, UpdateCtx,
    Widget, WidgetId, WidgetPod,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceConfig, LapceIcons, LapceTheme},
    data::LapceTabData,
};
use lsp_types::MessageType;

pub struct LapceMessage {
    widget_id: WidgetId,
    items: Vec<WidgetPod<LapceTabData, LapceMessageItem>>,
}

impl LapceMessage {
    pub fn new(widget_id: WidgetId) -> Self {
        Self {
            widget_id,
            items: Vec::new(),
        }
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }
}

impl Widget<LapceTabData> for LapceMessage {
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
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::NewMessage {
                        kind,
                        title,
                        message,
                    } => {
                        ctx.set_handled();
                        let item = LapceMessageItem::new(
                            self.widget_id,
                            ctx.text(),
                            *kind,
                            title.clone(),
                            message.clone(),
                            &data.config,
                        );
                        self.items.push(WidgetPod::new(item));
                        ctx.children_changed();
                        return;
                    }
                    LapceUICommand::CloseMessage(item_widget_id) => {
                        ctx.set_handled();
                        let mut index = None;
                        for (i, item) in self.items.iter().enumerate() {
                            if &item.id() == item_widget_id {
                                index = Some(i);
                            }
                        }
                        if let Some(index) = index {
                            self.items.remove(index);
                            ctx.children_changed();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        for item in self.items.iter_mut() {
            item.event(ctx, event, data, env);
        }

        if ctx.is_handled() {
            return;
        }

        match event {
            Event::MouseMove(_) | Event::MouseDown(_) | Event::MouseUp(_) => {
                ctx.set_handled();
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        for item in self.items.iter_mut() {
            item.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        for item in self.items.iter_mut() {
            item.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let mut width = 0.0;
        let mut height = 0.0;

        let num_items = self.items.len();
        for (i, item) in self.items.iter_mut().enumerate() {
            let size = item.layout(ctx, bc, data, env);
            item.set_origin(ctx, data, env, Point::new(0.0, height));

            if size.width > width {
                width = size.width;
            }
            height += size.height;
            if i + 1 < num_items {
                height += 10.0;
            }
        }
        ctx.set_paint_insets(100.0);
        Size::new(width, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let rect = ctx.region().bounding_box();
        for item in self.items.iter_mut() {
            if !item.layout_rect().intersect(rect).is_empty() {
                item.paint(ctx, data, env);
            }
        }
    }
}

pub struct LapceMessageItem {
    message_widget_id: WidgetId,
    item_widget_id: WidgetId,
    kind: MessageType,
    title: String,
    message: String,
    icon_rect: Rect,
    close_rect: Rect,
    close_clicked: Option<()>,
    text_layout: PietTextLayout,
    text_width: f64,
    text_padding: f64,
    text_line_height: f64,
}

impl LapceMessageItem {
    pub fn new(
        message_widget_id: WidgetId,
        piet_text: &mut PietText,
        kind: MessageType,
        title: String,
        message: String,
        config: &LapceConfig,
    ) -> Self {
        let text_width = 300.0;
        let text_line_height = 2.0;
        let text_layout = piet_text
            .new_text_layout("".to_string())
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .max_width(text_width)
            .set_line_height(text_line_height)
            .build()
            .unwrap();
        Self {
            message_widget_id,
            item_widget_id: WidgetId::next(),
            kind,
            icon_rect: Rect::ZERO,
            close_rect: Rect::ZERO,
            close_clicked: None,
            title,
            message,
            text_layout,
            text_width,
            text_padding: 20.0,
            text_line_height,
        }
    }

    fn new_text_layout(
        &self,
        piet_text: &mut PietText,
        config: &LapceConfig,
    ) -> PietTextLayout {
        let text = format!("{}\n\n{}", self.title, self.message);
        let text_layout = piet_text
            .new_text_layout(text)
            .font(config.ui.font_family(), config.ui.font_size() as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .range_attribute(
                0..self.title.len(),
                TextAttribute::Weight(FontWeight::BOLD),
            )
            .max_width(self.text_width)
            .set_line_height(self.text_line_height)
            .build()
            .unwrap();
        text_layout
    }
}

impl Widget<LapceTabData> for LapceMessageItem {
    fn id(&self) -> Option<WidgetId> {
        Some(self.item_widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseUp(mouse_event) => {
                ctx.set_handled();
                if self.close_clicked.take().is_some()
                    && self.close_rect.contains(mouse_event.pos)
                {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::CloseMessage(self.item_widget_id),
                        Target::Widget(self.message_widget_id),
                    ));
                }
            }
            Event::MouseDown(mouse_event) => {
                ctx.set_handled();
                self.close_clicked = None;
                if self.close_rect.contains(mouse_event.pos) {
                    self.close_clicked = Some(());
                }
            }
            Event::MouseMove(mouse_event) => {
                ctx.set_handled();
                if self.close_rect.contains(mouse_event.pos) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.clear_cursor();
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
        self.text_layout = self.new_text_layout(ctx.text(), &data.config);

        let mut height = self.text_layout.size().height;

        if let Some(metric) = self.text_layout.line_metric(0) {
            height =
                height - metric.y_offset + metric.y_offset / self.text_line_height;
        }

        if let Some(metric) = self
            .text_layout
            .line_metric(self.text_layout.line_count().saturating_sub(1))
        {
            let descend = metric.height - metric.y_offset;
            height = height - descend + descend / self.text_line_height;
        }

        self.icon_rect = Rect::ZERO
            .with_origin(Point::new(
                self.text_padding,
                self.text_padding + data.config.ui.font_size() as f64 / 2.0,
            ))
            .inflate(self.text_padding / 2.0, self.text_padding / 2.0);

        let width = self.text_width + self.text_padding * 4.0;

        self.close_rect = Rect::ZERO
            .with_origin(Point::new(
                width - self.text_padding,
                self.text_padding + data.config.ui.font_size() as f64 / 2.0,
            ))
            .inflate(self.text_padding / 2.0, self.text_padding / 2.0);

        Size::new(width, height + self.text_padding * 2.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let mut y = 0.0;
        if let Some(metric) = self.text_layout.line_metric(0) {
            y = -metric.y_offset + metric.y_offset / self.text_line_height;
        }
        let rect = ctx.size().to_rect().inflate(-0.5, -0.5);
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );
        ctx.draw_text(
            &self.text_layout,
            Point::new(self.text_padding * 2.0, y + self.text_padding),
        );

        let shadow_width = data.config.ui.drop_shadow_width() as f64;
        if shadow_width > 0.0 {
            ctx.with_save(|ctx| {
                ctx.blurred_rect(
                    rect,
                    shadow_width,
                    data.config
                        .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                );
            });
        } else {
            ctx.stroke(
                rect,
                data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
                1.0,
            );
        }

        let inflate =
            (self.text_padding - data.config.ui.font_size() as f64) / 2.0 - 1.0;
        let svg = match self.kind {
            MessageType::ERROR => LapceIcons::ERROR,
            MessageType::WARNING => LapceIcons::WARNING,
            _ => LapceIcons::WARNING,
        };
        ctx.draw_svg(
            &data.config.ui_svg(svg),
            self.icon_rect.inflate(-inflate, -inflate),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            ),
        );

        ctx.draw_svg(
            &data.config.ui_svg(LapceIcons::CLOSE),
            self.close_rect.inflate(-inflate, -inflate),
            Some(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND),
            ),
        );
    }
}
