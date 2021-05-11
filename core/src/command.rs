use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use druid::{Rect, Selector, Size, WidgetId};
use lsp_types::{Location, TextEdit};
use serde_json::Value;
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

#[derive(Display, EnumString, Clone, PartialEq, Debug)]
pub enum LapceCommand {
    #[strum(serialize = "file_explorer")]
    FileExplorer,
    #[strum(serialize = "file_explorer.cancel")]
    FileExplorerCancel,
    #[strum(serialize = "source_control")]
    SourceControl,
    #[strum(serialize = "source_control.cancel")]
    SourceControlCancel,
    #[strum(serialize = "palette.line")]
    PaletteLine,
    #[strum(serialize = "palette")]
    Palette,
    #[strum(serialize = "palette.cancel")]
    PaletteCancel,
    #[strum(serialize = "palette.symbol")]
    PaletteSymbol,
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
    #[strum(serialize = "close_tab")]
    CloseTab,
    #[strum(serialize = "new_tab")]
    NewTab,
    #[strum(serialize = "next_tab")]
    NextTab,
    #[strum(serialize = "previous_tab")]
    PreviousTab,
    #[strum(serialize = "insert_mode")]
    InsertMode,
    #[strum(serialize = "insert_first_non_blank")]
    InsertFirstNonBlank,
    #[strum(serialize = "toggle_comment")]
    ToggleComment,
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
    #[strum(serialize = "get_references")]
    GetReferences,
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
    #[strum(serialize = "clipboard_paste")]
    ClipboardPaste,
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
    #[strum(serialize = "next_error")]
    NextError,
    #[strum(serialize = "previous_error")]
    PreviousError,
    #[strum(serialize = "document_formatting")]
    DocumentFormatting,
    #[strum(serialize = "save")]
    Save,
    #[strum(serialize = "show_code_actions")]
    ShowCodeActions,
    #[strum(serialize = "match_pairs")]
    MatchPairs,
    #[strum(serialize = "next_unmatched_right_bracket")]
    NextUnmatchedRightBracket,
    #[strum(serialize = "previous_unmatched_left_bracket")]
    PreviousUnmatchedLeftBracket,
    #[strum(serialize = "next_unmatched_right_curly_bracket")]
    NextUnmatchedRightCurlyBracket,
    #[strum(serialize = "previous_unmatched_left_curly_bracket")]
    PreviousUnmatchedLeftCurlyBracket,
    #[strum(serialize = "open_folder")]
    OpenFolder,
    #[strum(serialize = "join_lines")]
    JoinLines,
    #[strum(serialize = "search_whole_word_forward")]
    SearchWholeWordForward,
    #[strum(serialize = "search_forward")]
    SearchForward,
    #[strum(serialize = "search_backward")]
    SearchBackward,
    Insert(String),
}

#[derive(Debug)]
pub enum EnsureVisiblePosition {
    CenterOfWindow,
}

#[derive(Debug)]
pub enum LapceUICommand {
    LoadBuffer {
        id: BufferId,
        path: PathBuf,
        content: String,
    },
    OpenFile(String),
    FillTextLayouts,
    UpdateSize,
    RequestLayout,
    RequestPaint,
    ResetFade,
    CloseTab,
    NewTab,
    NextTab,
    PreviousTab,
    FilterItems,
    CloseBuffers(Vec<BufferId>),
    RequestPaintRect(Rect),
    ApplyEdits(usize, u64, Vec<TextEdit>),
    ApplyEditsAndSave(usize, u64, Result<Value>),
    UpdateHighlights(BufferId, u64, Vec<(usize, usize, Highlight)>),
    CenterOfWindow,
    UpdateLineChanges(BufferId),
    ReloadBuffer(BufferId, u64, String),
    EnsureVisible((Rect, (f64, f64), Option<EnsureVisiblePosition>)),
    EditorViewSize(Size),
    Scroll((f64, f64)),
    ScrollTo((f64, f64)),
    ForceScrollTo(f64, f64),
    Split(bool),
    SplitExchange,
    SplitClose,
    SplitMove(SplitMoveDirection),
    GotoLocation(Location),
}
