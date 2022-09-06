use std::sync::Arc;

use druid::{Command, Env, EventCtx, Modifiers, Target, WidgetId};
use lapce_core::{
    command::{EditCommand, FocusCommand, MoveCommand},
    mode::Mode,
};
use serde::{Deserialize, Serialize};

use crate::{
    command::{CommandExecuted, CommandKind, LapceUICommand, LAPCE_UI_COMMAND},
    config::Config,
    data::LapceMainSplitData,
    keypress::KeyPressFocus,
    split::SplitDirection,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsValueKind {
    String,
    Number,
    Bool,
}

pub enum LapceSettingsKind {
    Core,
    Editor,
}

#[derive(Clone)]
pub struct LapceSettingsPanelData {
    pub panel_widget_id: WidgetId,

    pub keymap_widget_id: WidgetId,
    pub keymap_view_id: WidgetId,
    pub keymap_split_id: WidgetId,

    pub settings_widget_id: WidgetId,
    pub settings_view_id: WidgetId,
    pub settings_split_id: WidgetId,
}

impl KeyPressFocus for LapceSettingsPanelData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "modal_focus")
    }

    fn focus_only(&self) -> bool {
        true
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &crate::command::LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        if let CommandKind::Focus(FocusCommand::ModalClose) = command.kind {
            ctx.submit_command(Command::new(
                LAPCE_UI_COMMAND,
                LapceUICommand::Hide,
                Target::Widget(self.panel_widget_id),
            ));
            CommandExecuted::Yes
        } else {
            CommandExecuted::No
        }
    }
}

impl LapceSettingsPanelData {
    pub fn new() -> Self {
        Self {
            panel_widget_id: WidgetId::next(),
            keymap_widget_id: WidgetId::next(),
            keymap_view_id: WidgetId::next(),
            keymap_split_id: WidgetId::next(),
            settings_widget_id: WidgetId::next(),
            settings_view_id: WidgetId::next(),
            settings_split_id: WidgetId::next(),
        }
    }
}

impl Default for LapceSettingsPanelData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct LapceSettingsFocusData {
    pub widget_id: WidgetId,
    pub editor_tab_id: WidgetId,
    pub main_split: LapceMainSplitData,
    pub config: Arc<Config>,
}

impl KeyPressFocus for LapceSettingsFocusData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, _condition: &str) -> bool {
        false
    }

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
                FocusCommand::SplitVertical => {
                    self.main_split.split_settings(
                        ctx,
                        self.editor_tab_id,
                        SplitDirection::Vertical,
                        &self.config,
                    );
                }
                FocusCommand::SplitClose => {
                    self.main_split.settings_close(
                        ctx,
                        self.widget_id,
                        self.editor_tab_id,
                    );
                }
                _ => return CommandExecuted::No,
            },
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}
}

pub enum SettingsValue {
    Bool(bool),
}

pub struct LapceSettingsItemKeypress {
    input: String,
    cursor: usize,
}

impl KeyPressFocus for LapceSettingsItemKeypress {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, _condition: &str) -> bool {
        false
    }

    // fn run_command(
    //     &mut self,
    //     _ctx: &mut EventCtx,
    //     command: &LapceCommand,
    //     _count: Option<usize>,
    //     _mods: Modifiers,
    //     _env: &Env,
    // ) -> CommandExecuted {
    //     match command {
    //         LapceCommand::Right => {
    //             self.cursor += 1;
    //             if self.cursor > self.input.len() {
    //                 self.cursor = self.input.len();
    //             }
    //         }
    //         LapceCommand::Left => {
    //             if self.cursor == 0 {
    //                 return CommandExecuted::Yes;
    //             }
    //             self.cursor -= 1;
    //         }
    //         LapceCommand::DeleteBackward => {
    //             if self.cursor == 0 {
    //                 return CommandExecuted::Yes;
    //             }
    //             self.input.remove(self.cursor - 1);
    //             self.cursor -= 1;
    //         }
    //         _ => return CommandExecuted::No,
    //     }
    //     CommandExecuted::Yes
    // }

    fn run_command(
        &mut self,
        _ctx: &mut EventCtx,
        command: &crate::command::LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Move(cmd) => match cmd {
                MoveCommand::Right => {
                    self.cursor += 1;
                    if self.cursor > self.input.len() {
                        self.cursor = self.input.len();
                    }
                }
                MoveCommand::Left => {
                    if self.cursor == 0 {
                        return CommandExecuted::Yes;
                    }
                    self.cursor -= 1;
                }
                _ => return CommandExecuted::No,
            },
            CommandKind::Edit(EditCommand::DeleteBackward) => {
                if self.cursor == 0 {
                    return CommandExecuted::Yes;
                }
                self.input.remove(self.cursor - 1);
                self.cursor -= 1;
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn receive_char(&mut self, _ctx: &mut EventCtx, c: &str) {
        self.input.insert_str(self.cursor, c);
        self.cursor += c.len();
    }
}
