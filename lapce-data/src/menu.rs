use std::sync::Arc;

use druid::{
    piet::{Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, FontFamily,
    LayoutCtx, LifeCycle, LifeCycleCtx, MouseEvent, PaintCtx, Point, Rect,
    RenderContext, Size, Target, UpdateCtx, Widget, WidgetId,
};

use crate::{
    command::{
        CommandExecuted, LapceCommand, LapceCommandNew, LapceUICommand,
        LAPCE_NEW_COMMAND, LAPCE_UI_COMMAND,
    },
    config::LapceTheme,
    data::LapceWindowData,
    keypress::{Alignment, KeyPressFocus},
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
        matches!(condition, "list_focus" | "menu_focus")
    }

    fn run_command(
        &mut self,
        _ctx: &mut EventCtx,
        _command: &LapceCommand,
        _count: Option<usize>,
        _env: &Env,
    ) -> CommandExecuted {
        CommandExecuted::No
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}
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

impl Default for MenuData {
    fn default() -> Self {
        Self::new()
    }
}

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
