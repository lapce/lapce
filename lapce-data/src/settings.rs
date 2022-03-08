use druid::{Env, EventCtx, WidgetId};

use crate::{
    command::{CommandExecuted, LapceCommand},
    keypress::KeyPressFocus,
    state::Mode,
};

pub enum LapceSettingsKind {
    Core,
    Editor,
}

#[derive(Clone)]
pub struct LapceSettingsPanelData {
    pub shown: bool,
    pub panel_widget_id: WidgetId,

    pub keymap_widget_id: WidgetId,
    pub keymap_view_id: WidgetId,
    pub keymap_split_id: WidgetId,

    pub settings_widget_id: WidgetId,
    pub settings_view_id: WidgetId,
    pub settings_split_id: WidgetId,
}

impl LapceSettingsPanelData {
    pub fn new() -> Self {
        Self {
            shown: false,
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

    fn run_command(
        &mut self,
        _ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _env: &Env,
    ) -> CommandExecuted {
        match command {
            LapceCommand::Right => {
                self.cursor += 1;
                if self.cursor > self.input.len() {
                    self.cursor = self.input.len();
                }
            }
            LapceCommand::Left => {
                if self.cursor == 0 {
                    return CommandExecuted::Yes;
                }
                self.cursor -= 1;
            }
            LapceCommand::DeleteBackward => {
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
