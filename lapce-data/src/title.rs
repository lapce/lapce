use std::sync::Arc;

use druid::{Env, EventCtx, Point, WidgetId};
use lapce_core::{command::FocusCommand, mode::Mode};

use crate::{
    command::{CommandExecuted, CommandKind, LapceCommand},
    config::LapceConfig,
    keypress::KeyPressFocus,
    list::ListData,
};

#[derive(Clone)]
pub struct TitleData {
    pub widget_id: WidgetId,
    pub branches: BranchListData,
}

impl TitleData {
    pub fn new(config: Arc<LapceConfig>) -> TitleData {
        let widget_id = WidgetId::next();
        TitleData {
            widget_id,
            branches: BranchListData::new(config, widget_id),
        }
    }
}

#[derive(Clone)]
pub struct BranchListData {
    pub filter_editor: WidgetId,
    pub list: ListData<String, ()>,
    pub active: bool,
    /// The origin the list should appear at, this is updated whenever the
    /// branch list is opened in the titlebar
    pub origin: Point,
}

impl BranchListData {
    fn new(config: Arc<LapceConfig>, parent: WidgetId) -> Self {
        let list = ListData::new(config, parent, ());
        BranchListData {
            filter_editor: WidgetId::next(),
            list,
            active: false,
            origin: Point::ZERO,
        }
    }
}

impl KeyPressFocus for BranchListData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "list_focus" | "modal_focus")
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _mods: druid::Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        if let CommandKind::Focus(FocusCommand::ModalClose) = command.kind {
            self.active = false;
            return CommandExecuted::Yes;
        }
        self.list.run_command(ctx, command)
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {
        // Currently, this does not have any sort of input (such as for filtering)
    }
}
