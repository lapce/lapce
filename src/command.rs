use druid::Selector;
use strum;
use strum_macros::{Display, EnumProperty, EnumString};

pub const CRANE_COMMAND: Selector<CraneCommand> =
    Selector::new("crane.command");
pub const CRANE_UI_COMMAND: Selector<CraneUICommand> =
    Selector::new("crane.ui_command");

#[derive(Display, EnumString, Clone, PartialEq)]
pub enum CraneCommand {
    #[strum(serialize = "palette")]
    Palette,

    #[strum(serialize = "palette.cancel")]
    PaletteCancel,

    #[strum(serialize = "delete_backward")]
    DeleteBackward,

    Insert(String),
}

pub enum CraneUICommand {
    Show,
    Hide,
}
