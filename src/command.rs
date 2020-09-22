use druid::{Rect, Selector, WidgetId};
use strum;
use strum_macros::{Display, EnumProperty, EnumString};

use crate::split::SplitMoveDirection;

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
    #[strum(serialize = "delete_word_backward")]
    DeleteWordBackward,
    #[strum(serialize = "delete_to_beginning_of_line")]
    DeleteToBeginningOfLine,
    #[strum(serialize = "down")]
    Down,
    #[strum(serialize = "up")]
    Up,
    #[strum(serialize = "left")]
    Left,
    #[strum(serialize = "right")]
    Right,
    #[strum(serialize = "page_up")]
    PageUp,
    #[strum(serialize = "page_down")]
    PageDown,
    #[strum(serialize = "scroll_up")]
    ScrollUp,
    #[strum(serialize = "scroll_down")]
    ScrollDown,
    #[strum(serialize = "list.select")]
    ListSelect,
    #[strum(serialize = "list.next")]
    ListNext,
    #[strum(serialize = "list.previous")]
    ListPrevious,
    #[strum(serialize = "split_vertical")]
    SplitVertical,
    #[strum(serialize = "split_horizontal")]
    SplitHorizontal,
    #[strum(serialize = "split_exchange")]
    SplitExchange,
    #[strum(serialize = "split_right")]
    SplitRight,
    #[strum(serialize = "split_left")]
    SplitLeft,
    #[strum(serialize = "insert_mode")]
    InsertMode,
    #[strum(serialize = "normal_mode")]
    NormalMode,
    #[strum(serialize = "new_line_above")]
    NewLineAbove,
    #[strum(serialize = "new_line_below")]
    NewLineBelow,
    #[strum(serialize = "insert_new_line")]
    InsertNewLine,
    #[strum(serialize = "word_backward")]
    WordBackward,
    #[strum(serialize = "word_foward")]
    WordFoward,
    #[strum(serialize = "word_end_foward")]
    WordEndFoward,
    #[strum(serialize = "line_end")]
    LineEnd,
    #[strum(serialize = "line_start")]
    LineStart,
    #[strum(serialize = "first_line")]
    FirstLine,
    #[strum(serialize = "last_line")]
    LastLine,
    #[strum(serialize = "append")]
    Append,
    #[strum(serialize = "append_end_of_line")]
    AppendEndOfLine,
    Insert(String),
}

#[derive(Debug)]
pub enum CraneUICommand {
    RequestLayout,
    RequestPaint,
    RequestPaintRect(Rect),
    EnsureVisible((Rect, (f64, f64))),
    Scroll((f64, f64)),
    ScrollTo((f64, f64)),
    Split(bool, WidgetId),
    SplitExchange(WidgetId),
    SplitMove(SplitMoveDirection, WidgetId),
}
