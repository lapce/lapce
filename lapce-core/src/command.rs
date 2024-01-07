use strum_macros::{Display, EnumIter, EnumMessage, EnumString, IntoStaticStr};

use crate::movement::{LinePosition, Movement};

#[derive(
    Display,
    EnumString,
    EnumIter,
    Clone,
    PartialEq,
    Eq,
    Debug,
    EnumMessage,
    IntoStaticStr,
)]
pub enum EditCommand {
    #[strum(serialize = "move_line_up")]
    MoveLineUp,
    #[strum(serialize = "move_line_down")]
    MoveLineDown,
    #[strum(serialize = "insert_new_line")]
    InsertNewLine,
    #[strum(serialize = "insert_tab")]
    InsertTab,
    #[strum(serialize = "new_line_above")]
    NewLineAbove,
    #[strum(serialize = "new_line_below")]
    NewLineBelow,
    #[strum(serialize = "delete_backward")]
    DeleteBackward,
    #[strum(serialize = "delete_forward")]
    DeleteForward,
    #[strum(serialize = "delete_line")]
    DeleteLine,
    #[strum(serialize = "delete_forward_and_insert")]
    DeleteForwardAndInsert,
    #[strum(serialize = "delete_word_and_insert")]
    DeleteWordAndInsert,
    #[strum(serialize = "delete_line_and_insert")]
    DeleteLineAndInsert,
    #[strum(serialize = "delete_word_forward")]
    DeleteWordForward,
    #[strum(serialize = "delete_word_backward")]
    DeleteWordBackward,
    #[strum(serialize = "delete_to_beginning_of_line")]
    DeleteToBeginningOfLine,
    #[strum(serialize = "delete_to_end_of_line")]
    DeleteToEndOfLine,

    #[strum(serialize = "delete_to_end_and_insert")]
    DeleteToEndOfLineAndInsert,
    #[strum(message = "Join Lines")]
    #[strum(serialize = "join_lines")]
    JoinLines,
    #[strum(message = "Indent Line")]
    #[strum(serialize = "indent_line")]
    IndentLine,
    #[strum(message = "Outdent Line")]
    #[strum(serialize = "outdent_line")]
    OutdentLine,
    #[strum(message = "Toggle Line Comment")]
    #[strum(serialize = "toggle_line_comment")]
    ToggleLineComment,
    #[strum(serialize = "undo")]
    Undo,
    #[strum(serialize = "redo")]
    Redo,
    #[strum(message = "Copy")]
    #[strum(serialize = "clipboard_copy")]
    ClipboardCopy,
    #[strum(message = "Cut")]
    #[strum(serialize = "clipboard_cut")]
    ClipboardCut,
    #[strum(message = "Paste")]
    #[strum(serialize = "clipboard_paste")]
    ClipboardPaste,
    #[strum(serialize = "yank")]
    Yank,
    #[strum(serialize = "paste")]
    Paste,
    #[strum(serialize = "paste_before")]
    PasteBefore,

    #[strum(serialize = "normal_mode")]
    NormalMode,
    #[strum(serialize = "insert_mode")]
    InsertMode,
    #[strum(serialize = "insert_first_non_blank")]
    InsertFirstNonBlank,
    #[strum(serialize = "append")]
    Append,
    #[strum(serialize = "append_end_of_line")]
    AppendEndOfLine,
    #[strum(serialize = "toggle_visual_mode")]
    ToggleVisualMode,
    #[strum(serialize = "toggle_linewise_visual_mode")]
    ToggleLinewiseVisualMode,
    #[strum(serialize = "toggle_blockwise_visual_mode")]
    ToggleBlockwiseVisualMode,
    #[strum(serialize = "duplicate_line_up")]
    DuplicateLineUp,
    #[strum(serialize = "duplicate_line_down")]
    DuplicateLineDown,
}

impl EditCommand {
    pub fn not_changing_buffer(&self) -> bool {
        matches!(
            self,
            &EditCommand::ClipboardCopy
                | &EditCommand::Yank
                | &EditCommand::NormalMode
                | &EditCommand::InsertMode
                | &EditCommand::InsertFirstNonBlank
                | &EditCommand::Append
                | &EditCommand::AppendEndOfLine
                | &EditCommand::ToggleVisualMode
                | &EditCommand::ToggleLinewiseVisualMode
                | &EditCommand::ToggleBlockwiseVisualMode
        )
    }
}

#[derive(
    Display,
    EnumString,
    EnumIter,
    Clone,
    PartialEq,
    Eq,
    Debug,
    EnumMessage,
    IntoStaticStr,
)]
pub enum MoveCommand {
    #[strum(serialize = "down")]
    Down,
    #[strum(serialize = "up")]
    Up,
    #[strum(serialize = "left")]
    Left,
    #[strum(serialize = "right")]
    Right,
    #[strum(serialize = "word_backward")]
    WordBackward,
    #[strum(serialize = "word_forward")]
    WordForward,
    #[strum(serialize = "word_end_forward")]
    WordEndForward,
    #[strum(message = "Document Start")]
    #[strum(serialize = "document_start")]
    DocumentStart,
    #[strum(message = "Document End")]
    #[strum(serialize = "document_end")]
    DocumentEnd,
    #[strum(serialize = "line_end")]
    LineEnd,
    #[strum(serialize = "line_start")]
    LineStart,
    #[strum(serialize = "line_start_non_blank")]
    LineStartNonBlank,
    #[strum(serialize = "go_to_line_default_last")]
    GotoLineDefaultLast,
    #[strum(serialize = "go_to_line_default_first")]
    GotoLineDefaultFirst,
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
    #[strum(message = "Paragraph forward")]
    #[strum(serialize = "paragraph_forward")]
    ParagraphForward,
    #[strum(message = "Paragraph backward")]
    #[strum(serialize = "paragraph_backward")]
    ParagraphBackward,
}

impl MoveCommand {
    pub fn to_movement(&self, count: Option<usize>) -> Movement {
        use MoveCommand::*;
        match self {
            Left => Movement::Left,
            Right => Movement::Right,
            Up => Movement::Up,
            Down => Movement::Down,
            DocumentStart => Movement::DocumentStart,
            DocumentEnd => Movement::DocumentEnd,
            LineStart => Movement::StartOfLine,
            LineStartNonBlank => Movement::FirstNonBlank,
            LineEnd => Movement::EndOfLine,
            GotoLineDefaultFirst => match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::First),
            },
            GotoLineDefaultLast => match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::Last),
            },
            WordBackward => Movement::WordBackward,
            WordForward => Movement::WordForward,
            WordEndForward => Movement::WordEndForward,
            MatchPairs => Movement::MatchPairs,
            NextUnmatchedRightBracket => Movement::NextUnmatched(')'),
            PreviousUnmatchedLeftBracket => Movement::PreviousUnmatched('('),
            NextUnmatchedRightCurlyBracket => Movement::NextUnmatched('}'),
            PreviousUnmatchedLeftCurlyBracket => Movement::PreviousUnmatched('{'),
            ParagraphForward => Movement::ParagraphForward,
            ParagraphBackward => Movement::ParagraphBackward,
        }
    }
}

#[derive(
    Display,
    EnumString,
    EnumIter,
    Clone,
    PartialEq,
    Eq,
    Debug,
    EnumMessage,
    IntoStaticStr,
)]
pub enum FocusCommand {
    #[strum(serialize = "split_vertical")]
    SplitVertical,
    #[strum(serialize = "split_horizontal")]
    SplitHorizontal,
    #[strum(serialize = "split_exchange")]
    SplitExchange,
    #[strum(serialize = "split_close")]
    SplitClose,
    #[strum(serialize = "split_right")]
    SplitRight,
    #[strum(serialize = "split_left")]
    SplitLeft,
    #[strum(serialize = "split_up")]
    SplitUp,
    #[strum(serialize = "split_down")]
    SplitDown,
    #[strum(serialize = "search_whole_word_forward")]
    SearchWholeWordForward,
    #[strum(serialize = "search_forward")]
    SearchForward,
    #[strum(serialize = "search_backward")]
    SearchBackward,
    #[strum(serialize = "toggle_case_sensitive_search")]
    ToggleCaseSensitive,
    #[strum(serialize = "global_search_refresh")]
    GlobalSearchRefresh,
    #[strum(serialize = "clear_search")]
    ClearSearch,
    #[strum(serialize = "search_in_view")]
    SearchInView,
    #[strum(serialize = "list.select")]
    ListSelect,
    #[strum(serialize = "list.next")]
    ListNext,
    #[strum(serialize = "list.next_page")]
    ListNextPage,
    #[strum(serialize = "list.previous")]
    ListPrevious,
    #[strum(serialize = "list.previous_page")]
    ListPreviousPage,
    #[strum(serialize = "list.expand")]
    ListExpand,
    #[strum(serialize = "jump_to_next_snippet_placeholder")]
    JumpToNextSnippetPlaceholder,
    #[strum(serialize = "jump_to_prev_snippet_placeholder")]
    JumpToPrevSnippetPlaceholder,
    #[strum(serialize = "page_up")]
    PageUp,
    #[strum(serialize = "page_down")]
    PageDown,
    #[strum(serialize = "scroll_up")]
    ScrollUp,
    #[strum(serialize = "scroll_down")]
    ScrollDown,
    #[strum(serialize = "center_of_window")]
    CenterOfWindow,
    #[strum(serialize = "top_of_window")]
    TopOfWindow,
    #[strum(serialize = "bottom_of_window")]
    BottomOfWindow,
    #[strum(serialize = "show_code_actions")]
    ShowCodeActions,
    #[strum(serialize = "get_completion")]
    GetCompletion,
    #[strum(serialize = "get_signature")]
    GetSignature,
    #[strum(serialize = "toggle_breakpoint")]
    ToggleBreakpoint,
    /// This will close a modal, such as the settings window or completion
    #[strum(message = "Close Modal")]
    #[strum(serialize = "modal.close")]
    ModalClose,
    #[strum(message = "Go to Definition")]
    #[strum(serialize = "goto_definition")]
    GotoDefinition,
    #[strum(message = "Go to Type Definition")]
    #[strum(serialize = "goto_type_definition")]
    GotoTypeDefinition,
    #[strum(message = "Show Hover")]
    #[strum(serialize = "show_hover")]
    ShowHover,
    #[strum(message = "Go to Next Difference")]
    #[strum(serialize = "next_diff")]
    NextDiff,
    #[strum(message = "Go to Previous Difference")]
    #[strum(serialize = "previous_diff")]
    PreviousDiff,
    #[strum(message = "Toggle Code Lens")]
    #[strum(serialize = "toggle_code_lens")]
    ToggleCodeLens,
    #[strum(message = "Toggle History")]
    #[strum(serialize = "toggle_history")]
    ToggleHistory,
    #[strum(serialize = "format_document")]
    #[strum(message = "Format Document")]
    FormatDocument,
    #[strum(serialize = "search")]
    Search,
    #[strum(serialize = "focus_replace_editor")]
    FocusReplaceEditor,
    #[strum(serialize = "focus_find_editor")]
    FocusFindEditor,
    #[strum(serialize = "inline_find_right")]
    InlineFindRight,
    #[strum(serialize = "inline_find_left")]
    InlineFindLeft,
    #[strum(serialize = "create_mark")]
    CreateMark,
    #[strum(serialize = "go_to_mark")]
    GoToMark,
    #[strum(serialize = "repeat_last_inline_find")]
    RepeatLastInlineFind,
    #[strum(message = "Save")]
    #[strum(serialize = "save")]
    Save,
    #[strum(message = "Save Without Formatting")]
    #[strum(serialize = "save_without_format")]
    SaveWithoutFormatting,
    #[strum(serialize = "save_and_exit")]
    SaveAndExit,
    #[strum(serialize = "force_exit")]
    ForceExit,
    #[strum(serialize = "rename_symbol")]
    #[strum(message = "Rename Symbol")]
    Rename,
    #[strum(serialize = "confirm_rename")]
    ConfirmRename,
    #[strum(serialize = "select_next_syntax_item")]
    SelectNextSyntaxItem,
    #[strum(serialize = "select_previous_syntax_item")]
    SelectPreviousSyntaxItem,
    #[strum(serialize = "open_source_file")]
    OpenSourceFile,
    #[strum(serialize = "inline_completion.select")]
    #[strum(message = "Inline Completion Select")]
    InlineCompletionSelect,
    #[strum(serialize = "inline_completion.next")]
    #[strum(message = "Inline Completion Next")]
    InlineCompletionNext,
    #[strum(serialize = "inline_completion.previous")]
    #[strum(message = "Inline Completion Previous")]
    InlineCompletionPrevious,
    #[strum(serialize = "inline_completion.cancel")]
    #[strum(message = "Inline Completion Cancel")]
    InlineCompletionCancel,
    #[strum(serialize = "inline_completion.invoke")]
    #[strum(message = "Inline Completion Invoke")]
    InlineCompletionInvoke,
}

#[derive(
    Display,
    EnumString,
    EnumIter,
    Clone,
    PartialEq,
    Eq,
    Debug,
    EnumMessage,
    IntoStaticStr,
)]
pub enum MotionModeCommand {
    #[strum(serialize = "motion_mode_delete")]
    MotionModeDelete,
    #[strum(serialize = "motion_mode_indent")]
    MotionModeIndent,
    #[strum(serialize = "motion_mode_outdent")]
    MotionModeOutdent,
    #[strum(serialize = "motion_mode_yank")]
    MotionModeYank,
}

#[derive(
    Display,
    EnumString,
    EnumIter,
    Clone,
    PartialEq,
    Eq,
    Debug,
    EnumMessage,
    IntoStaticStr,
)]
pub enum MultiSelectionCommand {
    #[strum(serialize = "select_undo")]
    SelectUndo,
    #[strum(serialize = "insert_cursor_above")]
    InsertCursorAbove,
    #[strum(serialize = "insert_cursor_below")]
    InsertCursorBelow,
    #[strum(serialize = "insert_cursor_end_of_line")]
    InsertCursorEndOfLine,
    #[strum(serialize = "select_current_line")]
    SelectCurrentLine,
    #[strum(serialize = "select_all_current")]
    SelectAllCurrent,
    #[strum(serialize = "select_next_current")]
    SelectNextCurrent,
    #[strum(serialize = "select_skip_current")]
    SelectSkipCurrent,
    #[strum(serialize = "select_all")]
    SelectAll,
}
