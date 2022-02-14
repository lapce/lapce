use std::sync::Arc;

use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    Affine, BoxConstraints, Color, Command, Cursor, Data, Env, Event, EventCtx,
    FontFamily, FontWeight, LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent,
    PaintCtx, Point, Rect, RenderContext, Size, Target, UpdateCtx, Widget, WidgetId,
};

use crate::{
    command::{
        CommandExecuted, LapceCommand, LapceCommandNew, LapceUICommand,
        LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::{LapceTabData, LapceWindowData},
    keypress::KeyPressFocus,
    state::Mode,
};

#[derive(Debug)]
pub struct MenuItem {
    pub text: String,
    pub command: LapceCommandNew,
}

#[derive(Clone, Debug)]
pub struct MenuData {
    pub active: usize,
    pub widget_id: WidgetId,
    pub origin: Point,
    pub items: Arc<Vec<MenuItem>>,
    pub shown: bool,
}

impl KeyPressFocus for MenuData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "list_focus" => true,
            "menu_focus" => true,
            _ => false,
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) -> CommandExecuted {
        match command {
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, ctx: &mut EventCtx, c: &str) {}
}

impl MenuData {
    pub fn new() -> Self {
        Self {
            active: 0,
            widget_id: WidgetId::next(),
            items: Arc::new(Vec::new()),
            origin: Point::ZERO,
            shown: false,
        }
    }
}

pub struct Menu {
    widget_id: WidgetId,
}

impl Menu {
    pub fn new(data: &MenuData) -> Self {
        Self {
            widget_id: data.widget_id,
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
        let line_height = data.config.editor.line_height as f64;
        let n = (mouse_event.pos.y / line_height).floor() as usize;
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
        let line_height = data.config.editor.line_height as f64;
        let n = (mouse_event.pos.y / line_height).floor() as usize;
        if let Some(item) = data.menu.items.get(n) {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Widget(data.active_id),
            ));
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
                match command {
                    LapceUICommand::Focus => {
                        self.request_focus(ctx);
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceWindowData,
        env: &Env,
    ) {
        match event {
            LifeCycle::FocusChanged(is_focused) => {
                if !is_focused {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::HideMenu,
                        Target::Auto,
                    ));
                }
            }
            _ => (),
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceWindowData,
        data: &LapceWindowData,
        env: &Env,
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
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceWindowData,
        env: &Env,
    ) -> Size {
        let line_height = data.config.editor.line_height as f64;
        let height = line_height * data.menu.items.len() as f64;

        Size::new(300.0, height)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceWindowData, env: &Env) {
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

        let line_height = data.config.editor.line_height as f64;

        let line_rect = Rect::ZERO
            .with_origin(Point::new(0.0, data.menu.active as f64 * line_height))
            .with_size(Size::new(ctx.size().width, line_height));
        ctx.fill(
            line_rect,
            data.config.get_color_unchecked(LapceTheme::PANEL_CURRENT),
        );

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
                    line_height * i as f64
                        + (line_height - text_layout.size().height) / 2.0,
                ),
            );
        }
    }
}
