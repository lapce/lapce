use druid::{Rect, Selector};
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
    #[strum(serialize = "delete_to_beginning_of_line")]
    DeleteToBeginningOfLine,
    #[strum(serialize = "left")]
    Left,
    #[strum(serialize = "right")]
    Right,
    #[strum(serialize = "list.select")]
    ListSelect,
    #[strum(serialize = "list.next")]
    ListNext,
    #[strum(serialize = "list.previous")]
    ListPrevious,
    Insert(String),
}

#[derive(Debug)]
pub enum CraneUICommand {
    RequestLayout,
    RequestPaint,
    EnsureVisible((Rect, (f64, f64))),
    ScrollTo((f64, f64)),
}
