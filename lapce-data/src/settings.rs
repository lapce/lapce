use druid::{Command, Env, EventCtx, Modifiers, Target, WidgetId};

use crate::{
    command::{CommandExecuted, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
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

impl KeyPressFocus for LapceSettingsPanelData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: &str) -> bool {
        matches!(condition, "modal_focus" | "settings")
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
        _env: &Env,
    ) -> CommandExecuted {
        if let LapceCommand::ModalClose = command {
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

    fn receive_char(&mut self, _ctx: &mut EventCtx, _c: &str) {}
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
