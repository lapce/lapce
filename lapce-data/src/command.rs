use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Result;
use druid::{Point, Rect, Selector, Size, WidgetId, WindowId};
use indexmap::IndexMap;
use lapce_core::buffer::DiffLines;
use lapce_core::command::{
    EditCommand, FocusCommand, MotionModeCommand, MoveCommand, MultiSelectionCommand,
};
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
use strum_macros::{Display, EnumIter, EnumMessage, EnumString, IntoStaticStr};
use xi_rope::{spans::Spans, Rope};

use crate::{
    data::{EditorTabChild, SplitContent},
    editor::EditorLocationNew,
    keypress::{KeyMap, KeyPress},
    menu::MenuItem,
    palette::{NewPaletteItem, PaletteType},
    proxy::ProxyStatus,
    search::Match,
    split::{SplitDirection, SplitMoveDirection},
    state::LapceWorkspace,
};

pub const LAPCE_COMMAND: Selector<LapceCommand> = Selector::new("lapce.new-command");
pub const LAPCE_UI_COMMAND: Selector<LapceUICommand> =
    Selector::new("lapce.ui_command");

#[derive(Clone, Debug)]
pub struct LapceCommand {
    pub kind: CommandKind,
    pub data: Option<serde_json::Value>,
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

impl CommandKind {
    pub fn desc(&self) -> Option<&'static str> {
        match &self {
            CommandKind::Workbench(cmd) => cmd.get_message(),
            CommandKind::Edit(cmd) => cmd.get_message(),
            CommandKind::Move(cmd) => cmd.get_message(),
            CommandKind::Focus(cmd) => cmd.get_message(),
            CommandKind::MotionMode(cmd) => cmd.get_message(),
            CommandKind::MultiSelection(cmd) => cmd.get_message(),
        }
    }

    pub fn str(&self) -> &'static str {
        match &self {
            CommandKind::Workbench(cmd) => cmd.into(),
            CommandKind::Edit(cmd) => cmd.into(),
            CommandKind::Move(cmd) => cmd.into(),
            CommandKind::Focus(cmd) => cmd.into(),
            CommandKind::MotionMode(cmd) => cmd.into(),
            CommandKind::MultiSelection(cmd) => cmd.into(),
        }
    }
}

impl LapceCommand {
    pub const PALETTE: &'static str = "palette";
}

#[derive(PartialEq)]
pub enum CommandExecuted {
    Yes,
    No,
}

pub fn lapce_internal_commands() -> IndexMap<String, LapceCommand> {
    let mut commands = IndexMap::new();

    for c in LapceWorkbenchCommand::iter() {
        let command = LapceCommand {
            kind: CommandKind::Workbench(c.clone()),
            data: None,
        };
        commands.insert(c.to_string(), command);
    }

    for c in EditCommand::iter() {
        let command = LapceCommand {
            kind: CommandKind::Edit(c.clone()),
            data: None,
        };
        commands.insert(c.to_string(), command);
    }

    for c in MoveCommand::iter() {
        let command = LapceCommand {
            kind: CommandKind::Move(c.clone()),
            data: None,
        };
        commands.insert(c.to_string(), command);
    }

    for c in FocusCommand::iter() {
        let command = LapceCommand {
            kind: CommandKind::Focus(c.clone()),
            data: None,
        };
        commands.insert(c.to_string(), command);
    }

    for c in MotionModeCommand::iter() {
        let command = LapceCommand {
            kind: CommandKind::MotionMode(c.clone()),
            data: None,
        };
        commands.insert(c.to_string(), command);
    }

    for c in MultiSelectionCommand::iter() {
        let command = LapceCommand {
            kind: CommandKind::MultiSelection(c.clone()),
            data: None,
        };
        commands.insert(c.to_string(), command);
    }

    commands
}

#[derive(
    Display,
    EnumString,
    EnumIter,
    Clone,
    PartialEq,
    Debug,
    EnumMessage,
    IntoStaticStr,
)]
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
    #[strum(message = "Go to File")]
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
        version: String,
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
    InitPaletteInput(String),
    UpdatePaletteInput(String),
    UpdatePaletteItems(String, Vec<NewPaletteItem>),
    FilterPaletteItems(String, String, Vec<NewPaletteItem>),
    UpdateKeymapsFilter(String),
    UpdateSettingsFile(String, serde_json::Value),
    UpdateSettingsFilter(String),
    FilterKeymaps(String, Arc<Vec<KeyMap>>, Arc<Vec<LapceCommand>>),
    UpdatePickerPwd(PathBuf),
    UpdatePickerItems(PathBuf, HashMap<PathBuf, FileNodeItem>),
    UpdateExplorerItems(usize, PathBuf, Vec<FileNodeItem>),
    UpdateInstalledPlugins(HashMap<String, PluginDescription>),
    UpdatePluginDescriptions(Vec<PluginDescription>),
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
