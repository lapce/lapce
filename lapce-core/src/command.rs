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
    #[strum(message = "Move Line Up")]
    #[strum(serialize = "move_line_up")]
    MoveLineUp,
    #[strum(message = "Move Line Down")]
    #[strum(serialize = "move_line_down")]
    MoveLineDown,
    #[strum(message = "Insert New Line")]
    #[strum(serialize = "insert_new_line")]
    InsertNewLine,
    #[strum(message = "Insert Tab")]
    #[strum(serialize = "insert_tab")]
    InsertTab,
    #[strum(message = "New Line Above")]
    #[strum(serialize = "new_line_above")]
    NewLineAbove,
    #[strum(message = "New Line Below")]
    #[strum(serialize = "new_line_below")]
    NewLineBelow,
    #[strum(message = "Delete Backward")]
    #[strum(serialize = "delete_backward")]
    DeleteBackward,
    #[strum(message = "Delete Forward")]
    #[strum(serialize = "delete_forward")]
    DeleteForward,
    #[strum(message = "Delete Line")]
    #[strum(serialize = "delete_line")]
    DeleteLine,
    #[strum(message = "Delete Forward and Insert")]
    #[strum(serialize = "delete_forward_and_insert")]
    DeleteForwardAndInsert,
    #[strum(message = "Delete Word and Insert")]
    #[strum(serialize = "delete_word_and_insert")]
    DeleteWordAndInsert,
    #[strum(message = "Delete Line and Insert")]
    #[strum(serialize = "delete_line_and_insert")]
    DeleteLineAndInsert,
    #[strum(message = "Delete Word Forward")]
    #[strum(serialize = "delete_word_forward")]
    DeleteWordForward,
    #[strum(message = "Delete Word Backward")]
    #[strum(serialize = "delete_word_backward")]
    DeleteWordBackward,
    #[strum(message = "Delete to Beginning of Line")]
    #[strum(serialize = "delete_to_beginning_of_line")]
    DeleteToBeginningOfLine,
    #[strum(message = "Delete to End of Line")]
    #[strum(serialize = "delete_to_end_of_line")]
    DeleteToEndOfLine,

    #[strum(message = "Delete to End and Insert")]
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
    #[strum(message = "Undo")]
    #[strum(serialize = "undo")]
    Undo,
    #[strum(message = "Redo")]
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
    #[strum(message = "Yank")]
    #[strum(serialize = "yank")]
    Yank,
    #[strum(message = "Paste")]
    #[strum(serialize = "paste")]
    Paste,
    #[strum(message = "Paste Before")]
    #[strum(serialize = "paste_before")]
    PasteBefore,

    #[strum(message = "Normal Mode")]
    #[strum(serialize = "normal_mode")]
    NormalMode,
    #[strum(message = "Insert Mode")]
    #[strum(serialize = "insert_mode")]
    InsertMode,
    #[strum(message = "Insert First non Blank")]
    #[strum(serialize = "insert_first_non_blank")]
    InsertFirstNonBlank,
    #[strum(message = "Append")]
    #[strum(serialize = "append")]
    Append,
    #[strum(message = "Append End of Line")]
    #[strum(serialize = "append_end_of_line")]
    AppendEndOfLine,
    #[strum(message = "Toggle Visual Mode")]
    #[strum(serialize = "toggle_visual_mode")]
    ToggleVisualMode,
    #[strum(message = "Toggle Linewise Visual Mode")]
    #[strum(serialize = "toggle_linewise_visual_mode")]
    ToggleLinewiseVisualMode,
    #[strum(message = "Toggle Blockwise Visual Mode")]
    #[strum(serialize = "toggle_blockwise_visual_mode")]
    ToggleBlockwiseVisualMode,
    #[strum(message = "Duplicate Line Up")]
    #[strum(serialize = "duplicate_line_up")]
    DuplicateLineUp,
    #[strum(message = "Duplicate Line Down")]
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
    #[strum(message = "Down")]
    #[strum(serialize = "down")]
    Down,
    #[strum(message = "Up")]
    #[strum(serialize = "up")]
    Up,
    #[strum(message = "Left")]
    #[strum(serialize = "left")]
    Left,
    #[strum(message = "Right")]
    #[strum(serialize = "right")]
    Right,
    #[strum(message = "Word Backward")]
    #[strum(serialize = "word_backward")]
    WordBackward,
    #[strum(message = "Word Forward")]
    #[strum(serialize = "word_forward")]
    WordForward,
    #[strum(message = "Word End Forward")]
    #[strum(serialize = "word_end_forward")]
    WordEndForward,
    #[strum(message = "Document Start")]
    #[strum(serialize = "document_start")]
    DocumentStart,
    #[strum(message = "Document End")]
    #[strum(serialize = "document_end")]
    DocumentEnd,
    #[strum(message = "Line End")]
    #[strum(serialize = "line_end")]
    LineEnd,
    #[strum(message = "Line Start")]
    #[strum(serialize = "line_start")]
    LineStart,
    #[strum(message = "Line Start non Blank")]
    #[strum(serialize = "line_start_non_blank")]
    LineStartNonBlank,
    #[strum(message = "Go to Line Default Last")]
    #[strum(serialize = "go_to_line_default_last")]
    GotoLineDefaultLast,
    #[strum(message = "Go to Line Default First")]
    #[strum(serialize = "go_to_line_default_first")]
    GotoLineDefaultFirst,
    #[strum(message = "Match Pairs")]
    #[strum(serialize = "match_pairs")]
    MatchPairs,
    #[strum(message = "Next Unmatched Right Bracket")]
    #[strum(serialize = "next_unmatched_right_bracket")]
    NextUnmatchedRightBracket,
    #[strum(message = "Previous Unmatched Left Bracket")]
    #[strum(serialize = "previous_unmatched_left_bracket")]
    PreviousUnmatchedLeftBracket,
    #[strum(message = "Next Unmatched Right Curly Bracket")]
    #[strum(serialize = "next_unmatched_right_curly_bracket")]
    NextUnmatchedRightCurlyBracket,
    #[strum(message = "Previous Unmatched Left Curly Bracket")]
    #[strum(serialize = "previous_unmatched_left_curly_bracket")]
    PreviousUnmatchedLeftCurlyBracket,
    #[strum(message = "Paragraph Forward")]
    #[strum(serialize = "paragraph_forward")]
    ParagraphForward,
    #[strum(message = "Paragraph Backward")]
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
    #[strum(message = "Split Vertical")]
    #[strum(serialize = "split_vertical")]
    SplitVertical,
    #[strum(message = "Split Horizontal")]
    #[strum(serialize = "split_horizontal")]
    SplitHorizontal,
    #[strum(message = "Split Exchange")]
    #[strum(serialize = "split_exchange")]
    SplitExchange,
    #[strum(message = "Split Close")]
    #[strum(serialize = "split_close")]
    SplitClose,
    #[strum(message = "Split Right")]
    #[strum(serialize = "split_right")]
    SplitRight,
    #[strum(message = "Split Left")]
    #[strum(serialize = "split_left")]
    SplitLeft,
    #[strum(message = "Split Up")]
    #[strum(serialize = "split_up")]
    SplitUp,
    #[strum(message = "Split Down")]
    #[strum(serialize = "split_down")]
    SplitDown,
    #[strum(message = "Search Whole Word Forward")]
    #[strum(serialize = "search_whole_word_forward")]
    SearchWholeWordForward,
    #[strum(message = "Search Forward")]
    #[strum(serialize = "search_forward")]
    SearchForward,
    #[strum(message = "Search Backward")]
    #[strum(serialize = "search_backward")]
    SearchBackward,
    #[strum(message = "Toggle Case Sensitive Search")]
    #[strum(serialize = "toggle_case_sensitive_search")]
    ToggleCaseSensitive,
    #[strum(message = "Global Search Refresh")]
    #[strum(serialize = "global_search_refresh")]
    GlobalSearchRefresh,
    #[strum(message = "Clear Search")]
    #[strum(serialize = "clear_search")]
    ClearSearch,
    #[strum(message = "Search In View")]
    #[strum(serialize = "search_in_view")]
    SearchInView,
    #[strum(message = "List Select")]
    #[strum(serialize = "list.select")]
    ListSelect,
    #[strum(message = "List Next")]
    #[strum(serialize = "list.next")]
    ListNext,
    #[strum(message = "List Next Page")]
    #[strum(serialize = "list.next_page")]
    ListNextPage,
    #[strum(message = "List Previous")]
    #[strum(serialize = "list.previous")]
    ListPrevious,
    #[strum(message = "List Previous Page")]
    #[strum(serialize = "list.previous_page")]
    ListPreviousPage,
    #[strum(message = "List Expand")]
    #[strum(serialize = "list.expand")]
    ListExpand,
    #[strum(message = "Jump to Next Snippet Placeholder")]
    #[strum(serialize = "jump_to_next_snippet_placeholder")]
    JumpToNextSnippetPlaceholder,
    #[strum(message = "Jump to Previous Snippet Placeholder")]
    #[strum(serialize = "jump_to_prev_snippet_placeholder")]
    JumpToPrevSnippetPlaceholder,
    #[strum(message = "Page Up")]
    #[strum(serialize = "page_up")]
    PageUp,
    #[strum(message = "Page Down")]
    #[strum(serialize = "page_down")]
    PageDown,
    #[strum(message = "Scroll Up")]
    #[strum(serialize = "scroll_up")]
    ScrollUp,
    #[strum(message = "Scroll Down")]
    #[strum(serialize = "scroll_down")]
    ScrollDown,
    #[strum(message = "Center Of Window")]
    #[strum(serialize = "center_of_window")]
    CenterOfWindow,
    #[strum(message = "Top Of Window")]
    #[strum(serialize = "top_of_window")]
    TopOfWindow,
    #[strum(message = "Bottom Of Window")]
    #[strum(serialize = "bottom_of_window")]
    BottomOfWindow,
    #[strum(message = "Show Code Actions")]
    #[strum(serialize = "show_code_actions")]
    ShowCodeActions,
    #[strum(message = "Get Completion")]
    #[strum(serialize = "get_completion")]
    GetCompletion,
    #[strum(message = "Get Signature")]
    #[strum(serialize = "get_signature")]
    GetSignature,
    #[strum(message = "Toggle Breakpoint")]
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
    #[strum(message = "Format Document")]
    #[strum(serialize = "format_document")]
    FormatDocument,
    #[strum(message = "Search")]
    #[strum(serialize = "search")]
    Search,
    #[strum(message = "Focus Replace Editor")]
    #[strum(serialize = "focus_replace_editor")]
    FocusReplaceEditor,
    #[strum(message = "Focus Find Editor")]
    #[strum(serialize = "focus_find_editor")]
    FocusFindEditor,
    #[strum(message = "Inline Find Right")]
    #[strum(serialize = "inline_find_right")]
    InlineFindRight,
    #[strum(message = "Inline Find Left")]
    #[strum(serialize = "inline_find_left")]
    InlineFindLeft,
    #[strum(message = "Create Mark")]
    #[strum(serialize = "create_mark")]
    CreateMark,
    #[strum(message = "Go to Mark")]
    #[strum(serialize = "go_to_mark")]
    GoToMark,
    #[strum(message = "Repeat Last Inline Find")]
    #[strum(serialize = "repeat_last_inline_find")]
    RepeatLastInlineFind,
    #[strum(message = "Save")]
    #[strum(serialize = "save")]
    Save,
    #[strum(message = "Save Without Formatting")]
    #[strum(serialize = "save_without_format")]
    SaveWithoutFormatting,
    #[strum(message = "Save And Exit")]
    #[strum(serialize = "save_and_exit")]
    SaveAndExit,
    #[strum(message = "Force Exit")]
    #[strum(serialize = "force_exit")]
    ForceExit,
    #[strum(message = "Rename Symbol")]
    #[strum(serialize = "rename_symbol")]
    Rename,
    #[strum(message = "Confirm Rename")]
    #[strum(serialize = "confirm_rename")]
    ConfirmRename,
    #[strum(message = "Select Next Syntax Item")]
    #[strum(serialize = "select_next_syntax_item")]
    SelectNextSyntaxItem,
    #[strum(message = "Select Previous Syntax Item")]
    #[strum(serialize = "select_previous_syntax_item")]
    SelectPreviousSyntaxItem,
    #[strum(message = "Open Source File")]
    #[strum(serialize = "open_source_file")]
    OpenSourceFile,
    #[strum(message = "Inline Completion Select")]
    #[strum(serialize = "inline_completion.select")]
    InlineCompletionSelect,
    #[strum(message = "Inline Completion Next")]
    #[strum(serialize = "inline_completion.next")]
    InlineCompletionNext,
    #[strum(message = "Inline Completion Previous")]
    #[strum(serialize = "inline_completion.previous")]
    InlineCompletionPrevious,
    #[strum(message = "Inline Completion Cancel")]
    #[strum(serialize = "inline_completion.cancel")]
    InlineCompletionCancel,
    #[strum(message = "Inline Completion Invoke")]
    #[strum(serialize = "inline_completion.invoke")]
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
    #[strum(message = "Motion Mode Delete")]
    #[strum(serialize = "motion_mode_delete")]
    MotionModeDelete,
    #[strum(message = "Motion Mode Indent")]
    #[strum(serialize = "motion_mode_indent")]
    MotionModeIndent,
    #[strum(message = "Motion Mode Outdent")]
    #[strum(serialize = "motion_mode_outdent")]
    MotionModeOutdent,
    #[strum(message = "Motion Mode Yank")]
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
    #[strum(message = "Select Undo")]
    #[strum(serialize = "select_undo")]
    SelectUndo,
    #[strum(message = "Insert Cursor Above")]
    #[strum(serialize = "insert_cursor_above")]
    InsertCursorAbove,
    #[strum(message = "Insert Cursor Below")]
    #[strum(serialize = "insert_cursor_below")]
    InsertCursorBelow,
    #[strum(message = "Insert Cursor End of Line")]
    #[strum(serialize = "insert_cursor_end_of_line")]
    InsertCursorEndOfLine,
    #[strum(message = "Select Current Line")]
    #[strum(serialize = "select_current_line")]
    SelectCurrentLine,
    #[strum(message = "Select All Current")]
    #[strum(serialize = "select_all_current")]
    SelectAllCurrent,
    #[strum(message = "Select Next Current")]
    #[strum(serialize = "select_next_current")]
    SelectNextCurrent,
    #[strum(message = "Select Skip Current")]
    #[strum(serialize = "select_skip_current")]
    SelectSkipCurrent,
    #[strum(message = "Select All")]
    #[strum(serialize = "select_all")]
    SelectAll,
}
