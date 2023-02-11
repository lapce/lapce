use std::path::PathBuf;

use druid::{Command, Env, EventCtx, Modifiers, Target, WidgetId};
use indexmap::IndexMap;
use lapce_core::{
    command::{FocusCommand, MoveCommand},
    mode::Mode,
    movement::Movement,
};
use lapce_rpc::source_control::FileDiff;

use crate::{
    command::{CommandExecuted, CommandKind, LapceUICommand, LAPCE_UI_COMMAND},
    keypress::KeyPressFocus,
    split::{SplitDirection, SplitMoveDirection},
};

pub const SOURCE_CONTROL_BUFFER: &str = "[Source Control Buffer]";
pub const SEARCH_BUFFER: &str = "[Search Buffer]";

#[derive(Clone)]
pub struct SourceControlData {
    pub active: WidgetId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub split_direction: SplitDirection,
    /// Changed files
    pub file_list_id: WidgetId,
    pub file_list_index: usize,
    /// Branches
    pub commits_list_id: WidgetId,
    pub branches_list_id: WidgetId,
    pub tags_list_id: WidgetId,
    pub remotes_list_id: WidgetId,
    pub worktress_list_id: WidgetId,
    pub stashes_list_id: WidgetId,
    pub list_index: usize,

    pub editor_view_id: WidgetId,
    pub commit_button_id: WidgetId,
    // VCS modified files & whether they should be included in the next commit
    pub file_diffs: IndexMap<PathBuf, (FileDiff, bool)>,
    pub branch: Option<String>,
    pub commits: im::Vector<String>,
    pub branches: im::Vector<String>,
    pub tags: im::Vector<String>,
    pub remotes: im::Vector<String>,
    pub worktrees: im::Vector<String>,
    pub stashes: im::Vector<String>,
}

impl SourceControlData {
    pub fn new() -> Self {
        let editor_view_id = WidgetId::next();
        Self {
            active: editor_view_id,
            widget_id: WidgetId::next(),
            editor_view_id,
            file_list_id: WidgetId::next(),
            file_list_index: 0,
            commits_list_id: WidgetId::next(),
            branches_list_id: WidgetId::next(),
            tags_list_id: WidgetId::next(),
            remotes_list_id: WidgetId::next(),
            worktress_list_id: WidgetId::next(),
            stashes_list_id: WidgetId::next(),
            list_index: 0,
            commit_button_id: WidgetId::next(),
            split_id: WidgetId::next(),
            split_direction: SplitDirection::Horizontal,
            file_diffs: IndexMap::new(),
            branch: None,
            commits: im::Vector::new(),
            branches: im::Vector::new(),
            tags: im::Vector::new(),
            remotes: im::Vector::new(),
            worktrees: im::Vector::new(),
            stashes: im::Vector::new(),
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
        command: &crate::command::LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Focus(cmd) => match cmd {
                FocusCommand::SplitUp => {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::SplitEditorMove(
                            SplitMoveDirection::Up,
                            self.active,
                        ),
                        Target::Widget(self.split_id),
                    ));
                }
                FocusCommand::ListPrevious => {
                    self.file_list_index = Movement::Up.update_index(
                        self.file_list_index,
                        self.file_diffs.len(),
                        1,
                        true,
                    );
                }
                FocusCommand::ListNext => {
                    self.file_list_index = Movement::Down.update_index(
                        self.file_list_index,
                        self.file_diffs.len(),
                        1,
                        true,
                    );
                }
                FocusCommand::ListExpand => {
                    if !self.file_diffs.is_empty() {
                        self.file_diffs[self.file_list_index].1 =
                            !self.file_diffs[self.file_list_index].1;
                    }
                }
                FocusCommand::ListSelect => {
                    if !self.file_diffs.is_empty() {
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::OpenFileDiff {
                                path: self
                                    .file_diffs
                                    .get_index(self.file_list_index)
                                    .unwrap()
                                    .0
                                    .clone(),
                                history: "head".to_string(),
                            },
                            Target::Auto,
                        ));
                    }
                }
                _ => return CommandExecuted::No,
            },
            CommandKind::Move(cmd) => match cmd {
                MoveCommand::Up => {
                    self.file_list_index = Movement::Up.update_index(
                        self.file_list_index,
                        self.file_diffs.len(),
                        1,
                        true,
                    );
                }
                MoveCommand::Down => {
                    self.file_list_index = Movement::Down.update_index(
                        self.file_list_index,
                        self.file_diffs.len(),
                        1,
                        true,
                    );
                }
                _ => return CommandExecuted::No,
            },
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }
}
