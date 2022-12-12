use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Result;
use druid::{
    EventCtx, FileInfo, Point, Rect, Selector, SingleUse, Size, WidgetId, WindowId,
};
use indexmap::IndexMap;
use lapce_core::{
    buffer::DiffLines,
    command::{
        EditCommand, FocusCommand, MotionModeCommand, MoveCommand,
        MultiSelectionCommand,
    },
    syntax::Syntax,
};
use lapce_rpc::{
    buffer::BufferId,
    file::FileNodeItem,
    plugin::{PluginId, VoltInfo, VoltMetadata},
    source_control::DiffInfo,
    style::Style,
    terminal::TermId,
};
use lapce_xi_rope::{spans::Spans, Rope};
use lsp_types::{
    CodeActionOrCommand, CodeActionResponse, CompletionItem, CompletionResponse,
    InlayHint, Location, MessageType, Position, ProgressParams,
    PublishDiagnosticsParams, SelectionRange, SignatureHelp, TextEdit,
    WorkspaceEdit,
};
use serde_json::Value;
use strum::{self, EnumMessage, IntoEnumIterator};
use strum_macros::{Display, EnumIter, EnumMessage, EnumString, IntoStaticStr};

use crate::{
    alert::AlertContentData,
    data::{
        EditorTabChild, LapceMainSplitData, LapceTabData, LapceWorkspace,
        SplitContent,
    },
    document::BufferContent,
    editor::{EditorLocation, EditorPosition, Line, LineCol},
    keypress::{KeyMap, KeyPress},
    menu::MenuKind,
    palette::{PaletteItem, PaletteType},
    plugin::{PluginsInfo, VoltIconKind},
    proxy::ProxyStatus,
    rich_text::RichText,
    search::Match,
    selection_range::SelectionRangeDirection,
    settings::LapceSettingsKind,
    split::{SplitDirection, SplitMoveDirection},
    update::ReleaseInfo,
};

pub const LAPCE_OPEN_FOLDER: Selector<FileInfo> = Selector::new("lapce.open-folder");
pub const LAPCE_OPEN_FILE: Selector<FileInfo> = Selector::new("lapce.open-file");
pub const LAPCE_SAVE_FILE_AS: Selector<FileInfo> =
    Selector::new("lapce.save-file-as");
pub const LAPCE_COMMAND: Selector<LapceCommand> = Selector::new("lapce.new-command");
pub const LAPCE_UI_COMMAND: Selector<LapceUICommand> =
    Selector::new("lapce.ui_command");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LapceCommand {
    pub kind: CommandKind,
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

    pub fn is_palette_command(&self) -> bool {
        if let CommandKind::Workbench(cmd) = &self.kind {
            match cmd {
                LapceWorkbenchCommand::Palette
                | LapceWorkbenchCommand::PaletteLine
                | LapceWorkbenchCommand::PaletteSymbol
                | LapceWorkbenchCommand::PaletteCommand
                | LapceWorkbenchCommand::ChangeFileLanguage
                | LapceWorkbenchCommand::ChangeColorTheme
                | LapceWorkbenchCommand::ChangeIconTheme
                | LapceWorkbenchCommand::ConnectSshHost
                | LapceWorkbenchCommand::ConnectWsl
                | LapceWorkbenchCommand::PaletteWorkspace => return true,
                _ => {}
            }
        }

        false
    }
}

#[derive(PartialEq, Eq)]
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
    Eq,
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

    #[strum(serialize = "reveal_active_file_in_file_explorer")]
    #[strum(message = "Reveal Active File in File Explorer")]
    RevealActiveFileInFileExplorer,

    #[strum(serialize = "change_color_theme")]
    #[strum(message = "Change Color Theme")]
    ChangeColorTheme,

    #[strum(serialize = "change_icon_theme")]
    #[strum(message = "Change Icon Theme")]
    ChangeIconTheme,

    #[strum(serialize = "open_settings")]
    #[strum(message = "Open Settings")]
    OpenSettings,

    #[strum(serialize = "open_settings_file")]
    #[strum(message = "Open Settings File")]
    OpenSettingsFile,

    #[strum(serialize = "open_settings_directory")]
    #[strum(message = "Open Settings Directory")]
    OpenSettingsDirectory,

    #[strum(serialize = "open_keyboard_shortcuts")]
    #[strum(message = "Open Keyboard Shortcuts")]
    OpenKeyboardShortcuts,

    #[strum(serialize = "open_keyboard_shortcuts_file")]
    #[strum(message = "Open Keyboard Shortcuts File")]
    OpenKeyboardShortcutsFile,

    #[strum(serialize = "open_log_file")]
    #[strum(message = "Open Log File")]
    OpenLogFile,

    #[strum(serialize = "open_logs_directory")]
    #[strum(message = "Open Logs Directory")]
    OpenLogsDirectory,

    #[strum(serialize = "open_proxy_directory")]
    #[strum(message = "Open Proxy Directory")]
    OpenProxyDirectory,

    #[strum(serialize = "open_themes_directory")]
    #[strum(message = "Open Themes Directory")]
    OpenThemesDirectory,

    #[strum(serialize = "open_plugins_directory")]
    #[strum(message = "Open Plugins Directory")]
    OpenPluginsDirectory,

    #[strum(serialize = "close_window_tab")]
    #[strum(message = "Close Current Window Tab")]
    CloseWindowTab,

    #[strum(serialize = "new_window_tab")]
    #[strum(message = "Create New Window Tab")]
    NewWindowTab,

    #[strum(serialize = "new_terminal_tab")]
    #[strum(message = "Create New Terminal Tab")]
    NewTerminalTab,

    #[strum(serialize = "close_terminal_tab")]
    #[strum(message = "Close Terminal Tab")]
    CloseTerminalTab,

    #[strum(serialize = "next_terminal_tab")]
    #[strum(message = "Next Terminal Tab")]
    NextTerminalTab,

    #[strum(serialize = "previous_terminal_tab")]
    #[strum(message = "Previous Terminal Tab")]
    PreviousTerminalTab,

    #[strum(serialize = "next_window_tab")]
    #[strum(message = "Go To Next Window Tab")]
    NextWindowTab,

    #[strum(serialize = "previous_window_tab")]
    #[strum(message = "Go To Previous Window Tab")]
    PreviousWindowTab,

    #[strum(serialize = "reload_window")]
    #[strum(message = "Reload Window")]
    ReloadWindow,

    #[strum(message = "New Window")]
    #[strum(serialize = "new_window")]
    NewWindow,

    #[strum(message = "Close Window")]
    #[strum(serialize = "close_window")]
    CloseWindow,

    #[strum(message = "New File")]
    #[strum(serialize = "new_file")]
    NewFile,

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

    #[strum(serialize = "palette.workspace_symbol")]
    PaletteWorkspaceSymbol,

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

    #[strum(serialize = "toggle_panel_left_visual")]
    TogglePanelLeftVisual,

    #[strum(serialize = "toggle_panel_right_visual")]
    TogglePanelRightVisual,

    #[strum(serialize = "toggle_panel_bottom_visual")]
    TogglePanelBottomVisual,

    // Focus toggle commands
    #[strum(message = "Toggle Terminal Focus")]
    #[strum(serialize = "toggle_terminal_focus")]
    ToggleTerminalFocus,

    #[strum(serialize = "toggle_source_control_focus")]
    ToggleSourceControlFocus,

    #[strum(message = "Toggle Plugin Focus")]
    #[strum(serialize = "toggle_plugin_focus")]
    TogglePluginFocus,

    #[strum(message = "Toggle File Explorer Focus")]
    #[strum(serialize = "toggle_file_explorer_focus")]
    ToggleFileExplorerFocus,

    #[strum(message = "Toggle Problem Focus")]
    #[strum(serialize = "toggle_problem_focus")]
    ToggleProblemFocus,

    #[strum(message = "Toggle Search Focus")]
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

    #[strum(message = "Source Control: Init")]
    #[strum(serialize = "source_control_init")]
    SourceControlInit,

    #[strum(serialize = "source_control_commit")]
    SourceControlCommit,

    #[strum(message = "Source Control: Copy Remote File Url")]
    #[strum(serialize = "source_control_copy_active_file_remote_url")]
    SourceControlCopyActiveFileRemoteUrl,

    #[strum(message = "Source Control: Discard File Changes")]
    #[strum(serialize = "source_control_discard_active_file_changes")]
    SourceControlDiscardActiveFileChanges,

    #[strum(serialize = "source_control_discard_target_file_changes")]
    SourceControlDiscardTargetFileChanges,

    #[strum(message = "Source Control: Discard Workspace Changes")]
    #[strum(serialize = "source_control_discard_workspace_changes")]
    SourceControlDiscardWorkspaceChanges,

    #[strum(serialize = "export_current_theme_settings")]
    #[strum(message = "Export current settings to a theme file")]
    ExportCurrentThemeSettings,

    #[strum(serialize = "install_theme")]
    #[strum(message = "Install current theme file")]
    InstallTheme,

    #[strum(serialize = "change_file_language")]
    #[strum(message = "Change current file language")]
    ChangeFileLanguage,

    #[strum(serialize = "next_editor_tab")]
    #[strum(message = "Next editor tab")]
    NextEditorTab,

    #[strum(serialize = "previous_editor_tab")]
    #[strum(message = "Previous editor tab")]
    PreviousEditorTab,

    #[strum(serialize = "toggle_inlay_hints")]
    #[strum(message = "Toggle Inlay Hints")]
    ToggleInlayHints,

    #[strum(serialize = "restart_to_update")]
    RestartToUpdate,

    #[strum(serialize = "show_about")]
    #[strum(message = "About Lapce")]
    ShowAbout,

    #[strum(message = "Save All Files")]
    #[strum(serialize = "save_all")]
    SaveAll,

    #[cfg(target_os = "macos")]
    #[strum(message = "Install Lapce to PATH")]
    #[strum(serialize = "install_to_path")]
    InstallToPATH,

    #[cfg(target_os = "macos")]
    #[strum(message = "Uninstall Lapce from PATH")]
    #[strum(serialize = "uninstall_from_path")]
    UninstallFromPATH,

    #[strum(serialize = "quit")]
    #[strum(message = "Quit Editor")]
    Quit,
}

#[derive(Debug)]
pub enum EnsureVisiblePosition {
    // Move the view so the cursor line will be at the center of the window.  If
    // the cursor is near the beginning of the buffer, the view might not
    // change.
    CenterOfWindow,
    // Cursor will be at the top edge, down by a margin.
    TopOfWindow,
    // Cursor will be at the bottom edge, up by a margin.  If the cursor is near
    // the beginning of the buffer, the view might not change.
    BottomOfWindow,
}

pub enum LapceUICommand {
    InitChildren,
    InitTerminalPanel(bool),
    ReloadConfig,
    /// UTF8 offsets into the file
    InitBufferContent(InitBufferContent<usize>),
    /// Start of line position
    InitBufferContentLine(InitBufferContent<Line>),
    /// Line and UTF8 Column Positions
    InitBufferContentLineCol(InitBufferContent<LineCol>),
    /// UTF16 LSP positions
    InitBufferContentLsp(InitBufferContent<Position>),
    OpenFileChanged {
        path: PathBuf,
        content: Rope,
    },
    ReloadBuffer {
        path: PathBuf,
        rev: u64,
        content: Rope,
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
        location: EditorLocation,
    },
    ShowAbout,
    ShowAlert(AlertContentData),
    ShowMenu(Point, Arc<Vec<MenuKind>>),
    ShowWindow,
    ShowGitBranches {
        origin: Point,
        branches: im::Vector<String>,
    },
    UpdateSearchInput(String),
    UpdateSearch(String),
    UpdateSearchWithCaseSensitivity {
        pattern: String,
        case_sensitive: bool,
    },
    GlobalSearchResult(String, Arc<IndexMap<PathBuf, Vec<Match>>>),
    CancelFilePicker,
    SetWorkspace(LapceWorkspace),
    SetColorTheme(String, bool),
    SetIconTheme(String, bool),
    UpdateKeymap(KeyMap, Vec<KeyPress>),
    OpenURI(String),
    OpenPaths {
        window_tab_id: Option<(WindowId, WidgetId)>,
        folders: Vec<PathBuf>,
        files: Vec<PathBuf>,
    },
    OpenFile(PathBuf, bool),
    OpenFileDiff(PathBuf, String),
    RevealInFileExplorer(PathBuf),
    CancelCompletion(usize),
    ResolveCompletion(BufferId, u64, usize, Box<CompletionItem>),
    UpdateCompletion(usize, String, CompletionResponse, PluginId),
    UpdateSignature {
        request_id: usize,
        resp: SignatureHelp,
        plugin_id: PluginId,
    },
    UpdateHover(usize, Arc<Vec<RichText>>),
    UpdateVoltReadme(RichText),
    UpdateInlayHints {
        path: PathBuf,
        rev: u64,
        hints: Spans<InlayHint>,
    },
    UpdateCodeActions {
        path: PathBuf,
        plugin_id: PluginId,
        rev: u64,
        offset: usize,
        resp: CodeActionResponse,
    },
    CodeActionsError {
        path: PathBuf,
        rev: u64,
        offset: usize,
    },
    CancelPalette,
    RunCommand(String, Vec<String>),
    RunCodeAction(CodeActionOrCommand, PluginId),
    ApplyWorkspaceEdit(WorkspaceEdit),
    ShowCodeActions(Option<Point>),
    Hide,
    ResignFocus,
    UpdateLatestRelease(ReleaseInfo),
    Focus,
    FocusLost,
    ChildrenChanged,
    EnsureEditorTabActiveVisible,
    FocusSourceControl,
    ShowSettings,
    ShowKeybindings,
    ShowSettingsKind(LapceSettingsKind),
    FocusEditor,
    RunPalette(Option<PaletteType>),
    RunPaletteReferences(Vec<EditorLocation<Position>>),
    InitPaletteInput(String),
    UpdatePaletteInput(String),
    UpdatePaletteItems(String, im::Vector<PaletteItem>),
    FilterPaletteItems(String, String, im::Vector<PaletteItem>),
    UpdateKeymapsFilter(String),
    ResetSettings,
    ResetSettingsFile(String, String),
    UpdateSettingsFile(String, String, Value),
    UpdateSettingsFilter(String),
    FilterKeymaps(String, Arc<Vec<KeyMap>>, Arc<Vec<LapceCommand>>),
    UpdatePickerPwd(PathBuf),
    UpdatePickerItems(PathBuf, HashMap<PathBuf, FileNodeItem>),
    UpdateExplorerItems(PathBuf, HashMap<PathBuf, FileNodeItem>, bool),
    LoadPluginLatest(VoltInfo),
    LoadPlugins(PluginsInfo),
    LoadPluginsFailed,
    LoadPluginIcon(String, VoltIconKind),
    VoltInstalled(VoltMetadata, Option<String>),
    VoltInstalling(VoltInfo, String),
    VoltRemoving(VoltMetadata, String),
    VoltInstallStatusClear(String),
    VoltRemoved(VoltInfo, bool),
    EnableVolt(VoltInfo),
    DisableVolt(VoltInfo),
    EnableVoltWorkspace(VoltInfo),
    DisableVoltWorkspace(VoltInfo),
    RequestLayout,
    RequestPaint,
    ResetFade,
    //FocusTab,
    CloseTab,
    CloseTabId(WidgetId),
    FocusTabId(WidgetId),
    SwapTab(usize),
    TabToWindow(WindowId, WidgetId),
    NewTab(Option<LapceWorkspace>),
    NextTab,
    PreviousTab,
    NextEditorTab,
    PreviousEditorTab,
    FilterItems,
    RestartToUpdate(PathBuf, ReleaseInfo),
    UpdateStarted,
    UpdateFailed,
    NewWindow(WindowId),
    CloseWindow(WindowId),
    ReloadWindow,
    CloseBuffers(Vec<BufferId>),
    RequestPaintRect(Rect),
    ApplyEdits(usize, u64, Vec<TextEdit>),
    ApplyEditsAndSave(usize, u64, Result<Value>),
    DocumentFormat(PathBuf, u64, Result<Vec<TextEdit>>),
    DocumentFormatAndSave(PathBuf, u64, Result<Vec<TextEdit>>, Option<WidgetId>),
    DocumentSave(PathBuf, Option<WidgetId>),
    BufferSave(PathBuf, u64, Option<WidgetId>),
    UpdateSemanticStyles(BufferId, PathBuf, u64, Arc<Spans<Style>>),
    UpdateTerminalTitle(TermId, String),
    UpdateHistoryStyle {
        id: BufferId,
        path: PathBuf,
        history: String,
        highlights: Arc<Spans<Style>>,
    },
    UpdateSyntax {
        content: BufferContent,
        syntax: SingleUse<Syntax>,
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
    EnsureVisible((Rect, (f64, f64), Option<EnsureVisiblePosition>)),
    EnsureRectVisible(Rect),
    EnsureCursorVisible(Option<EnsureVisiblePosition>),
    EnsureCursorPosition(EnsureVisiblePosition),
    EditorViewSize(Size),
    Scroll((f64, f64)),
    ScrollTo((f64, f64)),
    ForceScrollTo(f64, f64),
    SaveAs(BufferContent, PathBuf, WidgetId, bool),
    SaveAsSuccess(BufferContent, u64, PathBuf, WidgetId, bool),
    HomeDir(PathBuf),
    WorkspaceFileChange,
    ProxyUpdateStatus(ProxyStatus),
    CloseTerminal(TermId),
    OpenPluginInfo(VoltInfo),
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
    SplitChangeDirection(SplitDirection),
    EditorTabAdd(usize, EditorTabChild),
    EditorTabRemove(usize, bool, bool),
    EditorTabSwap(usize, usize),
    EditorContentChanged,
    JumpToPosition(Option<WidgetId>, Position, bool),
    JumpToLine(Option<WidgetId>, usize),
    JumpToLocation(Option<WidgetId>, EditorLocation, bool),
    JumpToLspLocation(Option<WidgetId>, EditorLocation<Position>, bool),
    JumpToLineLocation(Option<WidgetId>, EditorLocation<Line>),
    JumpToLineColLocation(Option<WidgetId>, EditorLocation<LineCol>, bool),
    ToggleProblem(PathBuf),
    TerminalJumpToLine(i32),
    GoToLocation(Option<WidgetId>, EditorLocation, bool),
    GotoDefinition {
        editor_view_id: WidgetId,
        offset: usize,
        location: EditorLocation<Position>,
    },
    PrepareRename {
        path: PathBuf,
        rev: u64,
        offset: usize,
        start: usize,
        end: usize,
        placeholder: String,
    },
    PaletteReferences(usize, Vec<Location>),
    GotoLocation(Location),
    ActiveFileChanged {
        path: Option<PathBuf>,
    },
    /// Create a file in the given path with the given name and then open it
    CreateFileOpen {
        path: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    RenamePath {
        from: PathBuf,
        to: PathBuf,
    },
    /// Move a file/directory to the os-specific trash
    TrashPath {
        path: PathBuf,
    },
    /// Start renaming a specific file in view at the given index
    ExplorerStartRename {
        /// The index into the explorer's file listing
        list_index: usize,
        /// The level that it should be indented to
        indent_level: usize,
        /// The text it will start with
        text: String,
    },
    /// Start creating a new file/directory
    ExplorerNew {
        /// The index in the explorer's file listing that this should appear *after*
        list_index: usize,
        /// The level that it should be indented to
        indent_level: usize,
        /// Whether we are creating a file or a directory
        is_dir: bool,
        /// The folder that it would be created in
        base_path: PathBuf,
    },
    ExplorerEndNaming {
        /// Whether it should name/rename the file with the input data
        apply_naming: bool,
    },
    ExplorerRevealPath {
        path: PathBuf,
    },
    FileExplorerRefresh,
    PutToClipboard(String),
    CopyPath(PathBuf),
    CopyRelativePath(PathBuf),
    SetLanguage(String),
    ApplySelectionRange {
        buffer_id: BufferId,
        rev: u64,
        direction: SelectionRangeDirection,
    },
    StoreSelectionRangeAndApply {
        buffer_id: BufferId,
        rev: u64,
        current_selection: Option<(usize, usize)>,
        ranges: Vec<SelectionRange>,
        direction: SelectionRangeDirection,
    },

    /// An item in a list was chosen
    /// This is typically targeted at the widget which contains the list
    ListItemSelected,
    NewMessage {
        kind: MessageType,
        title: String,
        message: String,
    },
    CloseMessage(WidgetId),
}

/// This can't be an `FnOnce` because we only ever get a reference to
/// [`InitBufferContent`]
/// However, in reality, it should only ever be called once.
/// This could be more powerful if it was given `&mut LapceTabData` but that would
/// require moving the callers of it into `LapceTabData`.
///
/// Parameters:
/// `(ctx: &mut EventCtx, data: &mut LapceMainSplitData)`
pub type InitBufferContentCb =
    Box<dyn Fn(&mut EventCtx, &mut LapceMainSplitData) + Send>;

pub struct InitBufferContent<P: EditorPosition> {
    pub path: PathBuf,
    pub content: Rope,
    pub locations: Vec<(WidgetId, EditorLocation<P>)>,
    pub edits: Option<Rope>,
    pub cb: Option<InitBufferContentCb>,
}

impl<P: EditorPosition + Clone + Send + 'static> InitBufferContent<P> {
    pub fn execute(&self, ctx: &mut EventCtx, data: &mut LapceTabData) {
        let doc = data.main_split.open_docs.get_mut(&self.path).unwrap();
        let doc = Arc::make_mut(doc);
        doc.init_content(self.content.to_owned());

        if let Some(rope) = &self.edits {
            doc.reload(rope.clone(), false);
        }
        if let BufferContent::File(path) = doc.content() {
            if let Some(d) = data.main_split.diagnostics.get(path) {
                doc.set_diagnostics(d);
            }
        }

        for (view_id, location) in &self.locations {
            data.main_split.go_to_location(
                ctx,
                Some(*view_id),
                false,
                location.clone(),
                &data.config,
            );
        }

        // We've loaded the buffer and added it to the view, so inform the caller about it
        if let Some(cb) = &self.cb {
            (cb)(ctx, &mut data.main_split);
        }

        ctx.set_handled();
    }
}
