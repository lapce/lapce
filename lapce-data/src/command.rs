use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Result;
use druid::{Point, Rect, Selector, Size, WidgetId, WindowId};
use indexmap::IndexMap;
use lapce_core::command::{
    EditCommand, FocusCommand, MotionModeCommand, MoveCommand, MultiSelectionCommand,
};
use lapce_core::mode::MotionMode;
use lapce_core::syntax::Syntax;
use lapce_rpc::{
    buffer::BufferId, file::FileNodeItem, plugin::PluginDescription,
    source_control::DiffInfo, style::Style, terminal::TermId,
};
use lsp_types::{
    CodeActionResponse, CompletionItem, CompletionResponse, Hover, Location,
    Position, ProgressParams, PublishDiagnosticsParams, TextEdit,
};
use serde_json::Value;
use strum::{self, EnumMessage, IntoEnumIterator};
use strum_macros::{Display, EnumIter, EnumMessage, EnumString};
use xi_rope::{spans::Spans, Rope};

use crate::{
    buffer::DiffLines,
    data::{EditorTabChild, SplitContent},
    editor::EditorLocationNew,
    keypress::{KeyMap, KeyPress},
    menu::MenuItem,
    movement::{LinePosition, Movement},
    palette::{NewPaletteItem, PaletteType},
    proxy::ProxyStatus,
    search::Match,
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
    pub kind: CommandKind,
    pub data: Option<serde_json::Value>,
    pub palette_desc: Option<String>,
    pub target: CommandTarget,
}

#[derive(Clone, Debug)]
pub enum CommandKind {
    Workbench(LapceWorkbenchCommand),
    Edit(EditCommand),
    Move(MoveCommand),
    Focus(FocusCommand),
    MotionMode(MotionModeCommand),
    MultiSelection(MultiSelectionCommand),
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
            kind: CommandKind::Workbench(c.clone()),
            data: None,
            palette_desc: c.get_message().map(|m| m.to_string()),
            target: CommandTarget::Workbench,
        };
        commands.insert(command.cmd.clone(), command);
    }

    for c in EditCommand::iter() {
        let command = LapceCommandNew {
            cmd: c.to_string(),
            kind: CommandKind::Edit(c.clone()),
            data: None,
            palette_desc: c.get_message().map(|m| m.to_string()),
            target: CommandTarget::Focus,
        };
        commands.insert(command.cmd.clone(), command);
    }

    for c in MoveCommand::iter() {
        let command = LapceCommandNew {
            cmd: c.to_string(),
            kind: CommandKind::Move(c.clone()),
            data: None,
            palette_desc: c.get_message().map(|m| m.to_string()),
            target: CommandTarget::Focus,
        };
        commands.insert(command.cmd.clone(), command);
    }

    for c in FocusCommand::iter() {
        let command = LapceCommandNew {
            cmd: c.to_string(),
            kind: CommandKind::Focus(c.clone()),
            data: None,
            palette_desc: c.get_message().map(|m| m.to_string()),
            target: CommandTarget::Focus,
        };
        commands.insert(command.cmd.clone(), command);
    }

    for c in MotionModeCommand::iter() {
        let command = LapceCommandNew {
            cmd: c.to_string(),
            kind: CommandKind::MotionMode(c.clone()),
            data: None,
            palette_desc: c.get_message().map(|m| m.to_string()),
            target: CommandTarget::Focus,
        };
        commands.insert(command.cmd.clone(), command);
    }

    for c in MultiSelectionCommand::iter() {
        let command = LapceCommandNew {
            cmd: c.to_string(),
            kind: CommandKind::MultiSelection(c.clone()),
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

    #[strum(serialize = "close_folder")]
    #[strum(message = "Close Folder")]
    CloseFolder,

    #[strum(serialize = "open_file")]
    #[strum(message = "Open File")]
    OpenFile,

    #[strum(serialize = "change_theme")]
    #[strum(message = "Change Theme")]
    ChangeTheme,

    #[strum(serialize = "open_settings")]
    #[strum(message = "Open Settings")]
    OpenSettings,

    #[strum(serialize = "open_settings_file")]
    #[strum(message = "Open Settings File")]
    OpenSettingsFile,

    #[strum(serialize = "open_keyboard_shortcuts")]
    #[strum(message = "Open Keyboard Shortcuts")]
    OpenKeyboardShortcuts,

    #[strum(serialize = "open_keyboard_shortcuts_file")]
    #[strum(message = "Open Keyboard Shortcuts File")]
    OpenKeyboardShortcutsFile,

    #[strum(serialize = "open_log_file")]
    #[strum(message = "Open Log File")]
    OpenLogFile,

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

    #[strum(message = "New Window")]
    #[strum(serialize = "new_window")]
    NewWindow,

    #[strum(serialize = "connect_ssh_host")]
    #[strum(message = "Connect to SSH Host")]
    ConnectSshHost,

    #[strum(serialize = "connect_wsl")]
    #[strum(message = "Connect to WSL")]
    ConnectWsl,

    #[strum(serialize = "disconnect_remote")]
    #[strum(message = "Disconnect From Remote")]
    DisconnectRemote,

    #[strum(serialize = "palette.line")]
    PaletteLine,

    #[strum(serialize = "palette")]
    Palette,

    #[strum(serialize = "palette.symbol")]
    PaletteSymbol,

    #[strum(message = "Command Palette")]
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

    /// Toggles the panel passed in parameter.
    #[strum(serialize = "toggle_panel_focus")]
    TogglePanelFocus,

    /// Toggles the panel passed in parameter.
    #[strum(serialize = "toggle_panel_visual")]
    TogglePanelVisual,

    // Focus toggle commands
    #[strum(serialize = "toggle_terminal_focus")]
    ToggleTerminalFocus,

    #[strum(serialize = "toggle_source_control_focus")]
    ToggleSourceControlFocus,

    #[strum(serialize = "toggle_plugin_focus")]
    TogglePluginFocus,

    #[strum(serialize = "toggle_file_explorer_focus")]
    ToggleFileExplorerFocus,

    #[strum(serialize = "toggle_problem_focus")]
    ToggleProblemFocus,

    #[strum(serialize = "toggle_search_focus")]
    ToggleSearchFocus,

    // Visual toggle commands
    #[strum(serialize = "toggle_terminal_visual")]
    ToggleTerminalVisual,

    #[strum(serialize = "toggle_source_control_visual")]
    ToggleSourceControlVisual,

    #[strum(serialize = "toggle_plugin_visual")]
    TogglePluginVisual,

    #[strum(serialize = "toggle_file_explorer_visual")]
    ToggleFileExplorerVisual,

    #[strum(serialize = "toggle_problem_visual")]
    ToggleProblemVisual,

    #[strum(serialize = "toggle_search_visual")]
    ToggleSearchVisual,

    #[strum(serialize = "focus_editor")]
    FocusEditor,

    #[strum(serialize = "focus_terminal")]
    FocusTerminal,

    #[strum(serialize = "source_control_commit")]
    SourceControlCommit,
}

#[derive(Display, EnumString, EnumIter, Clone, PartialEq, Debug, EnumMessage)]
pub enum LapceCommand {
    #[strum(serialize = "move_line_up")]
    MoveLineUp,
    #[strum(serialize = "move_line_down")]
    MoveLineDown,
    #[strum(serialize = "insert_cursor_above")]
    InsertCursorAbove,
    #[strum(serialize = "insert_cursor_below")]
    InsertCursorBelow,
    #[strum(serialize = "insert_cursor_end_of_line")]
    InsertCursorEndOfLine,
    #[strum(serialize = "select_undo")]
    SelectUndo,
    #[strum(serialize = "select_current_line")]
    SelectCurrentLine,
    #[strum(serialize = "select_all_current")]
    SelectAllCurrent,
    #[strum(serialize = "select_next_current")]
    SelectNextCurrent,
    #[strum(serialize = "select_skip_current")]
    SelectSkipCurrent,
    #[strum(serialize = "file_explorer")]
    FileExplorer,
    #[strum(serialize = "file_explorer.cancel")]
    FileExplorerCancel,
    #[strum(serialize = "source_control")]
    SourceControl,
    #[strum(serialize = "source_control.cancel")]
    SourceControlCancel,
    /// This will close a modal, such as the settings window or completion
    #[strum(message = "Close Modal")]
    #[strum(serialize = "modal.close")]
    ModalClose,
    #[strum(serialize = "delete_backward")]
    DeleteBackward,
    #[strum(serialize = "delete_forward")]
    DeleteForward,
    #[strum(serialize = "delete_forward_and_insert")]
    DeleteForwardAndInsert,
    #[strum(serialize = "delete_visual")]
    DeleteVisual,
    #[strum(serialize = "delete_operator")]
    DeleteOperator,
    #[strum(serialize = "delete_word_backward")]
    DeleteWordBackward,
    #[strum(serialize = "delete_word_forward")]
    DeleteWordForward,
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

    #[strum(message = "Toggle Line Comment")]
    #[strum(serialize = "toggle_line_comment")]
    ToggleLineComment,

    #[strum(message = "Indent Line")]
    #[strum(serialize = "indent_line")]
    IndentLine,

    #[strum(message = "Outdent Line")]
    #[strum(serialize = "outdent_line")]
    OutdentLine,

    #[strum(serialize = "normal_mode")]
    NormalMode,
    #[strum(serialize = "toggle_visual_mode")]
    ToggleVisualMode,
    #[strum(serialize = "toggle_linewise_visual_mode")]
    ToggleLinewiseVisualMode,
    #[strum(serialize = "toggle_blockwise_visual_mode")]
    ToggleBlockwiseVisualMode,
    #[strum(serialize = "motion_mode_delete")]
    MotionModeDelete,
    #[strum(serialize = "motion_mode_indent")]
    MotionModeIndent,
    #[strum(serialize = "motion_mode_outdent")]
    MotionModeOutdent,
    #[strum(serialize = "motion_mode_yank")]
    MotionModeYank,
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
    #[strum(serialize = "append")]
    Append,
    #[strum(serialize = "append_end_of_line")]
    AppendEndOfLine,
    #[strum(serialize = "yank")]
    Yank,
    #[strum(serialize = "paste")]
    Paste,
    #[strum(serialize = "clipboard_cut")]
    ClipboardCut,
    #[strum(serialize = "clipboard_copy")]
    ClipboardCopy,
    #[strum(serialize = "clipboard_paste")]
    ClipboardPaste,
    #[strum(serialize = "undo")]
    Undo,
    #[strum(serialize = "redo")]
    Redo,

    #[strum(message = "Toggle Code Lens")]
    #[strum(serialize = "toggle_code_lens")]
    ToggleCodeLens,

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
    #[strum(message = "Go to Next Difference")]
    #[strum(serialize = "next_diff")]
    NextDiff,
    #[strum(message = "Go to Previous Difference")]
    #[strum(serialize = "previous_diff")]
    PreviousDiff,
    #[strum(serialize = "format_document")]
    #[strum(message = "Format Document")]
    FormatDocument,
    #[strum(message = "Save")]
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
    #[strum(serialize = "search")]
    Search,
    #[strum(serialize = "select_all")]
    SelectAll,
    #[strum(serialize = "search_whole_word_forward")]
    SearchWholeWordForward,
    #[strum(serialize = "search_forward")]
    SearchForward,
    #[strum(serialize = "search_backward")]
    SearchBackward,
    #[strum(serialize = "clear_search")]
    ClearSearch,
    #[strum(serialize = "search_in_view")]
    SearchInView,
    Insert(String),
}

impl LapceCommand {
    pub fn motion_mode_command(&self) -> Option<MotionMode> {
        let mode = match self {
            LapceCommand::MotionModeYank => MotionMode::Yank,
            LapceCommand::MotionModeDelete => MotionMode::Delete,
            LapceCommand::MotionModeIndent => MotionMode::Indent,
            LapceCommand::MotionModeOutdent => MotionMode::Outdent,
            _ => return None,
        };
        Some(mode)
    }

    pub fn move_command(&self, count: Option<usize>) -> Option<Movement> {
        match self {
            LapceCommand::Left => Some(Movement::Left),
            LapceCommand::Right => Some(Movement::Right),
            LapceCommand::Up => Some(Movement::Up),
            LapceCommand::Down => Some(Movement::Down),
            LapceCommand::DocumentStart => Some(Movement::DocumentStart),
            LapceCommand::DocumentEnd => Some(Movement::DocumentEnd),
            LapceCommand::LineStart => Some(Movement::StartOfLine),
            LapceCommand::LineStartNonBlank => Some(Movement::FirstNonBlank),
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
            LapceCommand::WordForward => Some(Movement::WordForward),
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
    InitChildren,
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
    GlobalSearchResult(String, Arc<HashMap<PathBuf, Vec<Match>>>),
    CancelFilePicker,
    SetWorkspace(LapceWorkspace),
    SetTheme(String, bool),
    UpdateKeymap(KeyMap, Vec<KeyPress>),
    OpenFile(PathBuf),
    OpenFileDiff(PathBuf, String),
    CancelCompletion(usize),
    ResolveCompletion(BufferId, u64, usize, Box<CompletionItem>),
    UpdateCompletion(usize, String, CompletionResponse),
    UpdateHover(usize, Hover),
    UpdateCodeActions(PathBuf, u64, usize, CodeActionResponse),
    CancelPalette,
    ShowCodeActions,
    CancelCodeActions,
    Hide,
    ResignFocus,
    Focus,
    EnsureEditorTabActiveVisble,
    FocusSourceControl,
    ShowSettings,
    ShowKeybindings,
    FocusEditor,
    RunPalette(Option<PaletteType>),
    RunPaletteReferences(Vec<EditorLocationNew>),
    UpdatePaletteItems(String, Vec<NewPaletteItem>),
    FilterPaletteItems(String, String, Vec<NewPaletteItem>),
    UpdateKeymapsFilter(String),
    UpdateSettingsFile(String, serde_json::Value),
    UpdateSettingsFilter(String),
    FilterKeymaps(String, Arc<Vec<KeyMap>>, Arc<Vec<LapceCommandNew>>),
    UpdatePickerPwd(PathBuf),
    UpdatePickerItems(PathBuf, HashMap<PathBuf, FileNodeItem>),
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
    NewWindow(WindowId),
    ReloadWindow,
    CloseBuffers(Vec<BufferId>),
    RequestPaintRect(Rect),
    ApplyEdits(usize, u64, Vec<TextEdit>),
    ApplyEditsAndSave(usize, u64, Result<Value>),
    DocumentFormat(PathBuf, u64, Result<Value>),
    DocumentFormatAndSave(PathBuf, u64, Result<Value>),
    BufferSave(PathBuf, u64),
    UpdateSemanticStyles(BufferId, PathBuf, u64, Arc<Spans<Style>>),
    UpdateTerminalTitle(TermId, String),
    UpdateHistoryStyle {
        id: BufferId,
        path: PathBuf,
        history: String,
        highlights: Arc<Spans<Style>>,
    },
    UpdateSyntax {
        path: PathBuf,
        rev: u64,
        syntax: Syntax,
    },
    UpdateHistoryChanges {
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
    HomeDir(PathBuf),
    ProxyUpdateStatus(ProxyStatus),
    CloseTerminal(TermId),
    SplitTerminal(bool, WidgetId),
    SplitTerminalClose(TermId, WidgetId),
    SplitEditor(bool, WidgetId),
    SplitEditorMove(SplitMoveDirection, WidgetId),
    SplitEditorExchange(WidgetId),
    SplitEditorClose(WidgetId),
    Split(bool),
    SplitClose,
    SplitExchange(SplitContent),
    SplitRemove(SplitContent),
    SplitMove(SplitMoveDirection),
    SplitAdd(usize, SplitContent, bool),
    SplitReplace(usize, SplitContent),
    SplitChangeDirectoin(SplitDirection),
    EditorTabAdd(usize, EditorTabChild),
    EditorTabRemove(usize, bool, bool),
    EditorTabSwap(usize, usize),
    JumpToPosition(Option<WidgetId>, Position),
    JumpToLine(Option<WidgetId>, usize),
    JumpToLocation(Option<WidgetId>, EditorLocationNew),
    TerminalJumpToLine(i32),
    GoToLocationNew(WidgetId, EditorLocationNew),
    GotoReference(WidgetId, usize, EditorLocationNew),
    GotoDefinition(WidgetId, usize, EditorLocationNew),
    PaletteReferences(usize, Vec<Location>),
    GotoLocation(Location),
    ActiveFileChanged {
        path: Option<PathBuf>,
    },
}
