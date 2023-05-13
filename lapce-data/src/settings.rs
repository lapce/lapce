use std::sync::Arc;

use druid::{Command, Env, EventCtx, Modifiers, Target, WidgetId};
use indexmap::IndexMap;
use lapce_core::{
    command::{EditCommand, FocusCommand, MoveCommand},
    mode::Mode,
};
use lapce_rpc::plugin::VoltID;
use serde::{Deserialize, Serialize};

use crate::{
    command::{CommandExecuted, CommandKind, LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceConfig,
    data::LapceMainSplitData,
    dropdown::DropdownData,
    keypress::KeyPressFocus,
    split::SplitDirection,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsValueKind {
    String,
    Integer,
    Float,
    Bool,
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub enum LapceSettingsKind {
    Core,
    UI,
    Editor,
    Terminal,
    Theme,
    Keymap,
    Plugin(VoltID),
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
    pub settings_sections: IndexMap<LapceSettingsKind, String>,
    pub plugin_section: String,

    pub filter_editor_id: WidgetId,
    pub filter_matches: IndexMap<String, usize>,

    /// Mapping of setting key to dropdown data for that key
    pub dropdown_data:
        im::HashMap<String, im::HashMap<String, DropdownData<String, ()>>>,
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
        let settings_sections = IndexMap::from([
            (LapceSettingsKind::Core, "Core Settings".to_string()),
            (LapceSettingsKind::UI, "UI Settings".to_string()),
            (LapceSettingsKind::Editor, "Editor Settings".to_string()),
            (LapceSettingsKind::Terminal, "Terminal Settings".to_string()),
            (LapceSettingsKind::Theme, "Theme Settings".to_string()),
            (LapceSettingsKind::Keymap, "Keybindings".to_string()),
        ]);
        Self {
            panel_widget_id: WidgetId::next(),
            keymap_widget_id: WidgetId::next(),
            keymap_view_id: WidgetId::next(),
            keymap_split_id: WidgetId::next(),
            settings_widget_id: WidgetId::next(),
            settings_view_id: WidgetId::next(),
            settings_split_id: WidgetId::next(),
            filter_editor_id: WidgetId::next(),
            dropdown_data: im::HashMap::new(),
            filter_matches: IndexMap::new(),
            settings_sections,
            plugin_section: String::from("Plugin Settings"),
        }
    }

    pub fn update_matches(&mut self, key: String, count: usize) {
        self.filter_matches.insert(key, count);
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
    pub config: Arc<LapceConfig>,
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
                    self.main_split.widget_close(
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
