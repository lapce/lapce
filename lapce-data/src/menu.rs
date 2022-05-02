use std::sync::Arc;

use druid::{Command, Env, EventCtx, Modifiers, Point, Target, WidgetId};
use lapce_core::{command::FocusCommand, mode::Mode};

use crate::{
    command::{
        CommandExecuted, CommandKind, LapceCommandNew, LapceUICommand,
        LAPCE_UI_COMMAND,
    },
    keypress::KeyPressFocus,
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
        matches!(condition, "list_focus" | "menu_focus" | "modal_focus")
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommandNew,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        if let CommandKind::Focus(FocusCommand::ModalClose) = command.kind {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::HideMenu,
                Target::Auto,
            ));
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Focus,
                Target::Auto,
            ));
            CommandExecuted::Yes
        } else {
            CommandExecuted::No
        }
    }
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
