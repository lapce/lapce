use std::sync::Arc;

use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontFamily,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetId,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND},
    config::LapceTheme,
    data::LapceWindowData,
    keypress::Alignment,
    menu::MenuData,
};

pub struct Menu {
    widget_id: WidgetId,
    line_height: f64,
}

impl Menu {
    pub fn new(data: &MenuData) -> Self {
        Self {
            widget_id: data.widget_id,
            line_height: 30.0,
        }
    }

    pub fn request_focus(&self, ctx: &mut EventCtx) {
        ctx.request_focus();
    }

    fn mouse_move(
        &self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &mut LapceWindowData,
    ) {
        ctx.set_handled();
        ctx.set_cursor(&Cursor::Pointer);
        let n = (mouse_event.pos.y / self.line_height).floor() as usize;
        if n < data.menu.items.len() {
            Arc::make_mut(&mut data.menu).active = n;
        }
    }

    fn mouse_down(
        &self,
        ctx: &mut EventCtx,
        mouse_event: &MouseEvent,
        data: &LapceWindowData,
    ) {
        ctx.set_handled();
        let n = (mouse_event.pos.y / self.line_height).floor() as usize;
        if let Some(item) = data.menu.items.get(n) {
            ctx.submit_command(Command::new(
                LAPCE_NEW_COMMAND,
                item.command.clone(),
                Target::Widget(data.active_id),
            ));
        }
    }
}

impl Widget<LapceWindowData> for Menu {
    fn id(&self) -> Option<WidgetId> {
        Some(self.widget_id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceWindowData,
        env: &Env,
    ) {
        match event {
            Event::KeyDown(key_event) => {
                if data.menu.shown {
                    let keypress = data.keypress.clone();
                    let mut_keypress = Arc::make_mut(&mut data.keypress);
                    mut_keypress.key_down(
                        ctx,
                        key_event,
                        Arc::make_mut(&mut data.menu),
                        env,
                    );
                    data.keypress = keypress;
                    ctx.set_handled();
                }
            }
            Event::MouseMove(mouse_event) => {
                if data.menu.shown {
                    self.mouse_move(ctx, mouse_event, data);
                }
            }
            Event::MouseDown(mouse_event) => {
                if data.menu.shown {
                    self.mouse_down(ctx, mouse_event, data);
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                if let LapceUICommand::Focus = command {
                    self.request_focus(ctx);
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceWindowData,
        _env: &Env,
    ) {
        if let LifeCycle::FocusChanged(is_focused) = event {
            if !is_focused {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::HideMenu,
                    Target::Auto,
                ));
            }
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceWindowData,
        data: &LapceWindowData,
        _env: &Env,
    ) {
        if !old_data.menu.items.same(&data.menu.items) {
            ctx.request_layout();
        }

        if !old_data.menu.shown != data.menu.shown {
            ctx.request_paint();
        }

        if !old_data.menu.active != data.menu.active {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &LapceWindowData,
        _env: &Env,
    ) -> Size {
        let height = self.line_height * data.menu.items.len() as f64;

        Size::new(300.0, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceWindowData, _env: &Env) {
        if !data.menu.shown {
            return;
        }

        if data.menu.items.len() == 0 {
            return;
        }

        let rect = ctx.size().to_rect();
        let shadow_width = 5.0;
        ctx.blurred_rect(
            rect,
            shadow_width,
            data.config
                .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
        );
        ctx.fill(
            rect,
            data.config
                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        );

        if ctx.is_hot() {
            let line_rect = Rect::ZERO
                .with_origin(Point::new(
                    0.0,
                    data.menu.active as f64 * self.line_height,
                ))
                .with_size(Size::new(ctx.size().width, self.line_height));
            ctx.fill(
                line_rect,
                data.config.get_color_unchecked(LapceTheme::PANEL_CURRENT),
            );
        }

        for (i, item) in data.menu.items.iter().enumerate() {
            let text_layout = ctx
                .text()
                .new_text_layout(item.text.clone())
                .font(FontFamily::SYSTEM_UI, 13.0)
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
                    10.0,
                    self.line_height * i as f64
                        + (self.line_height - text_layout.size().height) / 2.0,
                ),
            );

            if let Some(keymaps) =
                data.keypress.command_keymaps.get(&item.command.cmd)
            {
                if !keymaps.is_empty() {
                    let origin = Point::new(
                        rect.x1,
                        self.line_height * i as f64 + self.line_height / 2.0,
                    );
                    keymaps[0].paint(ctx, origin, Alignment::Right, &data.config);
                }
            }
        }
    }
}
