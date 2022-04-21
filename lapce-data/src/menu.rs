use std::sync::Arc;

use druid::{Command, Env, EventCtx, Modifiers, Point, Target, WidgetId};

use crate::{
    command::{
        CommandExecuted, LapceCommand, LapceCommandNew, LapceUICommand,
        LAPCE_UI_COMMAND,
    },
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
        matches!(condition, "list_focus" | "menu_focus" | "modal_focus")
    }

    // fn run_command(
    //     &mut self,
    //     ctx: &mut EventCtx,
    //     command: &LapceCommand,
    //     _count: Option<usize>,
    //     _mods: Modifiers,
    //     _env: &Env,
    // ) -> CommandExecuted {
    //     if let LapceCommand::ModalClose = command {
    //         ctx.submit_command(Command::new(
    //             LAPCE_UI_COMMAND,
    //             LapceUICommand::HideMenu,
    //             Target::Auto,
    //         ));
    //         ctx.submit_command(Command::new(
    //             LAPCE_UI_COMMAND,
    //             LapceUICommand::Focus,
    //             Target::Auto,
    //         ));
    //         CommandExecuted::Yes
    //     } else {
    //         CommandExecuted::No
    //     }
    // }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommandNew,
        count: Option<usize>,
        mods: Modifiers,
        env: &Env,
    ) -> CommandExecuted {
        todo!()
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
