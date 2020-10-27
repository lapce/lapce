use druid::{Rect, Selector, Size, WidgetId};
use lsp_types::Location;
use strum;
use strum_macros::{Display, EnumProperty, EnumString};
use tree_sitter_highlight::Highlight;

use crate::{
    buffer::BufferId,
    buffer::InvalLines,
    editor::{EditorLocation, HighlightTextLayout},
    split::SplitMoveDirection,
};

pub const LAPCE_COMMAND: Selector<LapceCommand> = Selector::new("lapce.command");
pub const LAPCE_UI_COMMAND: Selector<LapceUICommand> =
    Selector::new("lapce.ui_command");

#[derive(Display, EnumString, Clone, PartialEq)]
pub enum LapceCommand {
    #[strum(serialize = "file_explorer")]
    FileExplorer,
    #[strum(serialize = "file_explorer.cancel")]
    FileExplorerCancel,
    #[strum(serialize = "palette.line")]
    PaletteLine,
    #[strum(serialize = "palette")]
    Palette,
    #[strum(serialize = "palette.cancel")]
    PaletteCancel,
    #[strum(serialize = "delete_backward")]
    DeleteBackward,
    #[strum(serialize = "delete_foreward")]
    DeleteForeward,
    #[strum(serialize = "delete_foreward_and_insert")]
    DeleteForewardAndInsert,
    #[strum(serialize = "delete_visual")]
    DeleteVisual,
    #[strum(serialize = "delete_operator")]
    DeleteOperator,
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
    #[strum(serialize = "split_close")]
    SplitClose,
    #[strum(serialize = "split_exchange")]
    SplitExchange,
    #[strum(serialize = "split_right")]
    SplitRight,
    #[strum(serialize = "split_left")]
    SplitLeft,
    #[strum(serialize = "insert_mode")]
    InsertMode,
    #[strum(serialize = "insert_first_non_blank")]
    InsertFirstNonBlank,
    #[strum(serialize = "normal_mode")]
    NormalMode,
    #[strum(serialize = "toggle_visual_mode")]
    ToggleVisualMode,
    #[strum(serialize = "toggle_linewise_visual_mode")]
    ToggleLinewiseVisualMode,
    #[strum(serialize = "toggle_blockwise_visual_mode")]
    ToggleBlockwiseVisualMode,
    #[strum(serialize = "new_line_above")]
    NewLineAbove,
    #[strum(serialize = "new_line_below")]
    NewLineBelow,
    #[strum(serialize = "get_completion")]
    GetCompletion,
    #[strum(serialize = "insert_new_line")]
    InsertNewLine,
    #[strum(serialize = "word_backward")]
    WordBackward,
    #[strum(serialize = "word_foward")]
    WordFoward,
    #[strum(serialize = "word_end_forward")]
    WordEndForward,
    #[strum(serialize = "line_end")]
    LineEnd,
    #[strum(serialize = "line_start")]
    LineStart,
    #[strum(serialize = "go_to_line_deault_last")]
    GotoLineDefaultLast,
    #[strum(serialize = "go_to_line_deault_first")]
    GotoLineDefaultFirst,
    #[strum(serialize = "append")]
    Append,
    #[strum(serialize = "append_end_of_line")]
    AppendEndOfLine,
    #[strum(serialize = "yank")]
    Yank,
    #[strum(serialize = "paste")]
    Paste,
    #[strum(serialize = "undo")]
    Undo,
    #[strum(serialize = "redo")]
    Redo,
    #[strum(serialize = "center_of_window")]
    CenterOfWindow,
    #[strum(serialize = "goto_definition")]
    GotoDefinition,
    #[strum(serialize = "jump_location_backward")]
    JumpLocationBackward,
    #[strum(serialize = "jump_location_forward")]
    JumpLocationForward,
    Insert(String),
}

#[derive(Debug)]
pub enum EnsureVisiblePosition {
    CenterOfWindow,
}

#[derive(Debug)]
pub enum LapceUICommand {
    OpenFile(String),
    FillTextLayouts,
    RequestLayout,
    RequestPaint,
    RequestPaintRect(Rect),
    UpdateHighlights(BufferId, String, Vec<(usize, usize, Highlight)>),
    CenterOfWindow,
    EnsureVisible((Rect, (f64, f64), Option<EnsureVisiblePosition>)),
    EditorViewSize(Size),
    Scroll((f64, f64)),
    ScrollTo((f64, f64)),
    Split(bool),
    SplitExchange,
    SplitClose,
    SplitMove(SplitMoveDirection),
    GotoLocation(Location),
}
