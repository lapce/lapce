use std::sync::Arc;

use druid::{Command, Env, EventCtx, Modifiers, Target, WidgetId};
use lapce_core::{command::FocusCommand, mode::Mode};

use crate::{
    command::{CommandExecuted, CommandKind, LapceCommand, LAPCE_COMMAND},
    data::LapceTabData,
    keypress::KeyPressFocus,
};

#[derive(Clone, Debug)]
pub struct AlertContentData {
    pub title: String,
    pub msg: String,
    pub buttons: Vec<(String, WidgetId, LapceCommand)>,
}

#[derive(Clone)]
pub struct AlertData {
    pub widget_id: WidgetId,
    pub active: bool,
    pub content: AlertContentData,
}

pub struct AlertFocusData {
    alert: Arc<AlertData>,
}

impl Default for AlertData {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            active: false,
            content: AlertContentData {
                title: "".to_string(),
                msg: "".to_string(),
                buttons: Vec::new(),
            },
        }
    }
}

impl KeyPressFocus for AlertFocusData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn focus_only(&self) -> bool {
        true
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "modal_focus")
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        if let CommandKind::Focus(FocusCommand::ModalClose) = command.kind {
            ctx.submit_command(Command::new(
                LAPCE_COMMAND,
                LapceCommand {
                    kind: CommandKind::Focus(FocusCommand::ModalClose),
                    data: None,
                },
                Target::Widget(self.alert.widget_id),
            ));
            CommandExecuted::Yes
        } else {
            CommandExecuted::No
        }
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}
}

impl AlertFocusData {
    pub fn new(data: &LapceTabData) -> Self {
        Self {
            alert: data.alert.clone(),
        }
    }
}
