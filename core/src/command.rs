use std::{collections::HashMap, path::PathBuf, sync::Arc};

use alacritty_terminal::ansi::CursorShape;
use anyhow::Result;
use druid::{Point, Rect, Selector, Size, WidgetId};
use indexmap::IndexMap;
use lapce_proxy::{
    dispatch::{DiffInfo, FileDiff, FileNodeItem},
    plugin::PluginDescription,
    terminal::TermId,
};
use lsp_types::{
    CodeActionResponse, CompletionItem, CompletionResponse, Location, Position,
    ProgressParams, PublishDiagnosticsParams, Range, TextEdit, WorkDoneProgress,
};
use serde_json::Value;
use strum::{self, EnumMessage, IntoEnumIterator};
use strum_macros::{Display, EnumIter, EnumMessage, EnumProperty, EnumString};
use tree_sitter::Tree;
use tree_sitter_highlight::Highlight;
use xi_rope::{spans::Spans, Rope};

use crate::{
    buffer::BufferId,
    buffer::{DiffLines, InvalLines, Style},
    data::{EditorTabChild, SplitContent},
    editor::{EditorLocation, EditorLocationNew, HighlightTextLayout},
    menu::MenuItem,
    movement::{LinePosition, Movement},
    palette::{NewPaletteItem, PaletteType},
    split::{SplitDirection, SplitMoveDirection},
    state::LapceWorkspace,
};

pub const LAPCE_NEW_COMMAND: Selector<LapceCommandNew> =
    Selector::new("lapce.new-command");
pub const LAPCE_COMMAND: Selector<LapceCommand> = Selector::new("lapce.command");
pub const LAPCE_UI_COMMAND: Selector<LapceUICommand> =
    Selector::new("lapce.ui_command");

#[derive(Clone, Debug)]
pub struct LapceCommandNew {
    pub cmd: String,
    pub data: Option<serde_json::Value>,
    pub palette_desc: Option<String>,
    pub target: CommandTarget,
}

impl LapceCommandNew {
    pub const PALETTE: &'static str = "palette";
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandTarget {
    Workbench,
    Focus,
    Plugin(String),
}

#[derive(PartialEq)]
pub enum CommandExecuted {
    Yes,
    No,
}

pub fn lapce_internal_commands() -> IndexMap<String, LapceCommandNew> {
    let mut commands = IndexMap::new();

    for c in LapceWorkbenchCommand::iter() {
        let command = LapceCommandNew {
            cmd: c.to_string(),
            data: None,
            palette_desc: c.get_message().map(|m| m.to_string()),
            target: CommandTarget::Workbench,
        };
        commands.insert(command.cmd.clone(), command);
    }

    for c in LapceCommand::iter() {
        let command = LapceCommandNew {
            cmd: c.to_string(),
            data: None,
            palette_desc: c.get_message().map(|m| m.to_string()),
            target: CommandTarget::Focus,
        };
        commands.insert(command.cmd.clone(), command);
    }

    commands
}

#[derive(Display, EnumString, EnumIter, Clone, PartialEq, Debug, EnumMessage)]
pub enum LapceWorkbenchCommand {
    #[strum(serialize = "enable_modal_editing")]
    #[strum(message = "Enable Modal Editing")]
    EnableModal,

    #[strum(serialize = "disable_modal_editing")]
    #[strum(message = "Disable Modal Editing")]
    DisableModal,

    #[strum(serialize = "open_folder")]
    #[strum(message = "Open Folder")]
    OpenFolder,

    #[strum(serialize = "change_theme")]
    #[strum(message = "Change Theme")]
    ChangeTheme,

    #[strum(serialize = "open_settings")]
    #[strum(message = "Open Settings")]
    OpenSettings,

    #[strum(serialize = "open_keyboard_shortcuts")]
    #[strum(message = "Open Keyboard Shortcuts")]
    OpenKeyboardShortcuts,

    #[strum(serialize = "close_tab")]
    #[strum(message = "Close Current Tab")]
    CloseTab,

    #[strum(serialize = "new_tab")]
    #[strum(message = "Create New Tab")]
    NewTab,

    #[strum(serialize = "next_tab")]
    #[strum(message = "Go To Next Tab")]
    NextTab,

    #[strum(serialize = "previous_tab")]
    #[strum(message = "Go To Previous Tab")]
    PreviousTab,

    #[strum(serialize = "reload_window")]
    #[strum(message = "Reload Window")]
    ReloadWindow,

    #[strum(serialize = "connect_ssh_host")]
    #[strum(message = "Connect to SSH Host")]
    ConnectSshHost,

    #[strum(serialize = "palette.line")]
    PaletteLine,

    #[strum(serialize = "palette")]
    Palette,

    #[strum(serialize = "palette.symbol")]
    PaletteSymbol,

    #[strum(serialize = "palette.command")]
    PaletteCommand,

    #[strum(message = "Open Recent Workspace")]
    #[strum(serialize = "palette.workspace")]
    PaletteWorkspace,

    #[strum(serialize = "source_control.checkout_branch")]
    CheckoutBranch,

    #[strum(serialize = "toggle_maximized_panel")]
    ToggleMaximizedPanel,

    #[strum(serialize = "hide_panel")]
    HidePanel,

    #[strum(serialize = "show_panel")]
    ShowPanel,

    #[strum(serialize = "toggle_panel")]
    TogglePanel,

    #[strum(serialize = "toggle_terminal")]
    ToggleTerminal,

    #[strum(serialize = "toggle_source_control")]
    ToggleSourceControl,

    #[strum(serialize = "toggle_plugin")]
    TogglePlugin,

    #[strum(serialize = "toggle_file_explorer")]
    ToggleFileExplorer,

    #[strum(serialize = "toggle_problem")]
    ToggleProblem,

    #[strum(serialize = "toggle_search")]
    ToggleSearch,

    #[strum(serialize = "focus_editor")]
    FocusEditor,

    #[strum(serialize = "focus_terminal")]
    FocusTerminal,

    #[strum(serialize = "source_control_commit")]
    SourceControlCommit,
}

#[derive(Display, EnumString, EnumIter, Clone, PartialEq, Debug, EnumMessage)]
pub enum LapceCommand {
    #[strum(serialize = "file_explorer")]
    FileExplorer,
    #[strum(serialize = "file_explorer.cancel")]
    FileExplorerCancel,
    #[strum(serialize = "source_control")]
    SourceControl,
    #[strum(serialize = "source_control.cancel")]
    SourceControlCancel,
    #[strum(serialize = "code_actions.cancel")]
    CodeActionsCancel,
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
    #[strum(serialize = "inline_find_right")]
    InlineFindRight,
    #[strum(serialize = "inline_find_left")]
    InlineFindLeft,
    #[strum(serialize = "repeat_last_inline_find")]
    RepeatLastInlineFind,
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
    #[strum(serialize = "list.expand")]
    ListExpand,
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
    #[strum(serialize = "split_up")]
    SplitUp,
    #[strum(serialize = "split_down")]
    SplitDown,
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
    #[strum(serialize = "insert_tab")]
    InsertTab,
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
    #[strum(serialize = "clipboard_copy")]
    ClipboardCopy,
    #[strum(serialize = "clipboard_paste")]
    ClipboardPaste,
    #[strum(serialize = "undo")]
    Undo,
    #[strum(serialize = "redo")]
    Redo,
    #[strum(serialize = "center_of_window")]
    CenterOfWindow,

    #[strum(message = "Go to Definition")]
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
    #[strum(serialize = "next_diff")]
    NextDiff,
    #[strum(serialize = "previous_diff")]
    PreviousDiff,
    #[strum(serialize = "format_document")]
    #[strum(message = "Format Document")]
    FormatDocument,
    #[strum(serialize = "save")]
    Save,
    #[strum(serialize = "show_code_actions")]
    ShowCodeActions,
    #[strum(serialize = "match_pairs")]
    MatchPairs,
    #[strum(serialize = "next_unmatched_right_bracket")]
    NextUnmatchedRightBracket,
    #[strum(serialize = "jump_to_next_snippet_placeholder")]
    JumpToNextSnippetPlaceholder,
    #[strum(serialize = "jump_to_prev_snippet_placeholder")]
    JumpToPrevSnippetPlaceholder,
    #[strum(serialize = "previous_unmatched_left_bracket")]
    PreviousUnmatchedLeftBracket,
    #[strum(serialize = "next_unmatched_right_curly_bracket")]
    NextUnmatchedRightCurlyBracket,
    #[strum(serialize = "previous_unmatched_left_curly_bracket")]
    PreviousUnmatchedLeftCurlyBracket,
    #[strum(serialize = "join_lines")]
    JoinLines,
    #[strum(serialize = "search_whole_word_forward")]
    SearchWholeWordForward,
    #[strum(serialize = "search_forward")]
    SearchForward,
    #[strum(serialize = "search_backward")]
    SearchBackward,
    #[strum(serialize = "clear_search")]
    ClearSearch,
    Insert(String),
}

impl LapceCommand {
    pub fn move_command(&self, count: Option<usize>) -> Option<Movement> {
        match self {
            LapceCommand::Left => Some(Movement::Left),
            LapceCommand::Right => Some(Movement::Right),
            LapceCommand::Up => Some(Movement::Up),
            LapceCommand::Down => Some(Movement::Down),
            LapceCommand::LineStart => Some(Movement::StartOfLine),
            LapceCommand::LineEnd => Some(Movement::EndOfLine),
            LapceCommand::GotoLineDefaultFirst => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::First),
            }),
            LapceCommand::GotoLineDefaultLast => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::Last),
            }),
            LapceCommand::WordBackward => Some(Movement::WordBackward),
            LapceCommand::WordFoward => Some(Movement::WordForward),
            LapceCommand::WordEndForward => Some(Movement::WordEndForward),
            LapceCommand::MatchPairs => Some(Movement::MatchPairs),
            LapceCommand::NextUnmatchedRightBracket => {
                Some(Movement::NextUnmatched(')'))
            }
            LapceCommand::PreviousUnmatchedLeftBracket => {
                Some(Movement::PreviousUnmatched('('))
            }
            LapceCommand::NextUnmatchedRightCurlyBracket => {
                Some(Movement::NextUnmatched('}'))
            }
            LapceCommand::PreviousUnmatchedLeftCurlyBracket => {
                Some(Movement::PreviousUnmatched('{'))
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum EnsureVisiblePosition {
    CenterOfWindow,
}

#[derive(Debug)]
pub enum LapceUICommand {
    InitTerminalPanel(bool),
    ReloadConfig,
    LoadBuffer {
        path: PathBuf,
        content: String,
        locations: Vec<(WidgetId, EditorLocationNew)>,
    },
    LoadBufferHead {
        path: PathBuf,
        id: String,
        content: Rope,
    },
    LoadBufferAndGoToPosition {
        path: PathBuf,
        content: String,
        editor_view_id: WidgetId,
        location: EditorLocationNew,
    },
    HideMenu,
    ShowMenu(Point, Arc<Vec<MenuItem>>),
    UpdateSearch(String),
    GlobalSearchResult(
        String,
        Arc<HashMap<PathBuf, Vec<(usize, (usize, usize), String)>>>,
    ),
    SetWorkspace(LapceWorkspace),
    SetTheme(String, bool),
    OpenFile(PathBuf),
    OpenFileDiff(PathBuf, String),
    CancelCompletion(usize),
    ResolveCompletion(BufferId, u64, usize, CompletionItem),
    UpdateCompletion(usize, String, CompletionResponse),
    UpdateCodeActions(PathBuf, u64, usize, CodeActionResponse),
    CancelPalette,
    ShowCodeActions,
    CancelCodeActions,
    Focus,
    FocusSourceControl,
    FocusEditor,
    RunPalette(Option<PaletteType>),
    RunPaletteReferences(Vec<EditorLocationNew>),
    UpdatePaletteItems(String, Vec<NewPaletteItem>),
    FilterPaletteItems(String, String, Vec<NewPaletteItem>),
    UpdateExplorerItems(usize, PathBuf, Vec<FileNodeItem>),
    UpdateInstalledPlugins(HashMap<String, PluginDescription>),
    UpdatePluginDescriptions(Vec<PluginDescription>),
    UpdateWindowOrigin,
    RequestLayout,
    RequestPaint,
    ResetFade,
    //FocusTab,
    CloseTab,
    CloseTabId(WidgetId),
    FocusTabId(WidgetId),
    SwapTab(usize),
    NewTab,
    NextTab,
    PreviousTab,
    FilterItems,
    ReloadWindow,
    CloseBuffers(Vec<BufferId>),
    RequestPaintRect(Rect),
    ApplyEdits(usize, u64, Vec<TextEdit>),
    ApplyEditsAndSave(usize, u64, Result<Value>),
    DocumentFormat(PathBuf, u64, Result<Value>),
    DocumentFormatAndSave(PathBuf, u64, Result<Value>),
    BufferSave(PathBuf, u64),
    UpdateSemanticTokens(BufferId, PathBuf, u64, Vec<(usize, usize, String)>),
    UpdateHighlights(BufferId, u64, Vec<(usize, usize, Highlight)>),
    UpdateTerminalTitle(TermId, String),
    UpdateStyle {
        id: BufferId,
        path: PathBuf,
        rev: u64,
        highlights: Spans<Style>,
        semantic_tokens: bool,
    },
    UpdateHistoryStyle {
        id: BufferId,
        path: PathBuf,
        history: String,
        highlights: Spans<Style>,
    },
    UpdateSyntaxTree {
        id: BufferId,
        path: PathBuf,
        rev: u64,
        tree: Tree,
    },
    UpdateHisotryChanges {
        id: BufferId,
        path: PathBuf,
        rev: u64,
        history: String,
        changes: Arc<Vec<DiffLines>>,
    },
    CenterOfWindow,
    UpdateLineChanges(BufferId),
    PublishDiagnostics(PublishDiagnosticsParams),
    WorkDoneProgress(ProgressParams),
    UpdateDiffInfo(DiffInfo),
    ReloadBuffer(BufferId, u64, String),
    EnsureVisible((Rect, (f64, f64), Option<EnsureVisiblePosition>)),
    EnsureRectVisible(Rect),
    EnsureCursorVisible(Option<EnsureVisiblePosition>),
    EnsureCursorCenter,
    EditorViewSize(Size),
    Scroll((f64, f64)),
    ScrollTo((f64, f64)),
    ForceScrollTo(f64, f64),
    CloseTerminal(TermId),
    SplitTerminal(bool, WidgetId),
    SplitTerminalClose(TermId, WidgetId),
    SplitEditor(bool, WidgetId),
    SplitEditorMove(SplitMoveDirection, WidgetId),
    SplitEditorExchange(WidgetId),
    SplitEditorClose(WidgetId),
    Split(bool),
    SplitExchange,
    SplitClose,
    SplitMove(SplitMoveDirection),
    SplitAdd(usize, SplitContent, bool),
    SplitReplace(usize, SplitContent),
    SplitChangeDirectoin(SplitDirection),
    EditorTabAdd(usize, EditorTabChild),
    EditorTabRemove(usize),
    JumpToPosition(Option<WidgetId>, Position),
    JumpToLine(Option<WidgetId>, usize),
    JumpToLocation(Option<WidgetId>, EditorLocationNew),
    TerminalJumpToLine(i32),
    GoToLocationNew(WidgetId, EditorLocationNew),
    GotoReference(WidgetId, usize, EditorLocationNew),
    GotoDefinition(WidgetId, usize, EditorLocationNew),
    PaletteReferences(usize, Vec<Location>),
    GotoLocation(Location),
}
