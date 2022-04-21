use druid::{Command, Env, EventCtx, Modifiers, Target, WidgetId};
use lapce_rpc::source_control::FileDiff;

use crate::{
    command::{CommandExecuted, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    keypress::KeyPressFocus,
    movement::Movement,
    split::{SplitDirection, SplitMoveDirection},
    state::Mode,
};

pub const SOURCE_CONTROL_BUFFER: &str = "[Source Control Buffer]";
pub const SEARCH_BUFFER: &str = "[Search Buffer]";

#[derive(Clone)]
pub struct SourceControlData {
    pub active: WidgetId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub split_direction: SplitDirection,
    pub file_list_id: WidgetId,
    pub file_list_index: usize,
    pub editor_view_id: WidgetId,
    pub file_diffs: Vec<(FileDiff, bool)>,
    pub branch: String,
    pub branches: Vec<String>,
}

impl SourceControlData {
    pub fn new() -> Self {
        let file_list_id = WidgetId::next();
        let editor_view_id = WidgetId::next();
        Self {
            active: editor_view_id,
            widget_id: WidgetId::next(),
            editor_view_id,
            file_list_id,
            file_list_index: 0,
            split_id: WidgetId::next(),
            split_direction: SplitDirection::Horizontal,
            file_diffs: Vec::new(),
            branch: "".to_string(),
            branches: Vec::new(),
        }
    }
}

impl Default for SourceControlData {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyPressFocus for SourceControlData {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "source_control_focus" => true,
            "list_focus" => self.active == self.file_list_id,
            _ => false,
        }
    }

    // fn run_command(
    //     &mut self,
    //     ctx: &mut EventCtx,
    //     command: &LapceCommand,
    //     _count: Option<usize>,
    //     _mods: Modifiers,
    //     _env: &Env,
    // ) -> CommandExecuted {
    //     match command {
    //         LapceCommand::SplitUp => {
    //             ctx.submit_command(Command::new(
    //                 LAPCE_UI_COMMAND,
    //                 LapceUICommand::SplitEditorMove(
    //                     SplitMoveDirection::Up,
    //                     self.active,
    //                 ),
    //                 Target::Widget(self.split_id),
    //             ));
    //         }
    //         LapceCommand::SourceControlCancel => {
    //             ctx.submit_command(Command::new(
    //                 LAPCE_UI_COMMAND,
    //                 LapceUICommand::FocusEditor,
    //                 Target::Auto,
    //             ));
    //         }
    //         LapceCommand::Up | LapceCommand::ListPrevious => {
    //             self.file_list_index = Movement::Up.update_index(
    //                 self.file_list_index,
    //                 self.file_diffs.len(),
    //                 1,
    //                 true,
    //             );
    //         }
    //         LapceCommand::Down | LapceCommand::ListNext => {
    //             self.file_list_index = Movement::Down.update_index(
    //                 self.file_list_index,
    //                 self.file_diffs.len(),
    //                 1,
    //                 true,
    //             );
    //         }
    //         LapceCommand::ListExpand => {
    //             if !self.file_diffs.is_empty() {
    //                 self.file_diffs[self.file_list_index].1 =
    //                     !self.file_diffs[self.file_list_index].1;
    //             }
    //         }
    //         LapceCommand::ListSelect => {
    //             if !self.file_diffs.is_empty() {
    //                 ctx.submit_command(Command::new(
    //                     LAPCE_UI_COMMAND,
    //                     LapceUICommand::OpenFileDiff(
    //                         self.file_diffs[self.file_list_index].0.path().clone(),
    //                         "head".to_string(),
    //                     ),
    //                     Target::Auto,
    //                 ));
    //             }
    //         }
    //         _ => return CommandExecuted::No,
    //     }
    //     CommandExecuted::Yes
    // }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &crate::command::LapceCommandNew,
        count: Option<usize>,
        mods: Modifiers,
        env: &Env,
    ) -> CommandExecuted {
        todo!()
    }
}
