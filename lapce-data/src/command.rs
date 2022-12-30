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
    pub data: Option<Value>,
    pub kind: CommandKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandKind {
    Edit(EditCommand),
    Focus(FocusCommand),
    MotionMode(MotionModeCommand),
    Move(MoveCommand),
    MultiSelection(MultiSelectionCommand),
    Workbench(LapceWorkbenchCommand),
}

impl CommandKind {
    pub fn desc(&self) -> Option<&'static str> {
        match &self {
            CommandKind::Edit(cmd) => cmd.get_message(),
            CommandKind::Focus(cmd) => cmd.get_message(),
            CommandKind::MotionMode(cmd) => cmd.get_message(),
            CommandKind::Move(cmd) => cmd.get_message(),
            CommandKind::MultiSelection(cmd) => cmd.get_message(),
            CommandKind::Workbench(cmd) => cmd.get_message(),
        }
    }

    pub fn str(&self) -> &'static str {
        match &self {
            CommandKind::Edit(cmd) => cmd.into(),
            CommandKind::Focus(cmd) => cmd.into(),
            CommandKind::MotionMode(cmd) => cmd.into(),
            CommandKind::Move(cmd) => cmd.into(),
            CommandKind::MultiSelection(cmd) => cmd.into(),
            CommandKind::Workbench(cmd) => cmd.into(),
        }
    }
}

impl LapceCommand {
    pub const PALETTE: &'static str = "palette";

    pub fn is_palette_command(&self) -> bool {
        if let CommandKind::Workbench(cmd) = &self.kind {
            match cmd {
                LapceWorkbenchCommand::ChangeColorTheme
                | LapceWorkbenchCommand::ChangeFileLanguage
                | LapceWorkbenchCommand::ChangeIconTheme
                | LapceWorkbenchCommand::ConnectSshHost
                | LapceWorkbenchCommand::ConnectWsl
                | LapceWorkbenchCommand::Palette
                | LapceWorkbenchCommand::PaletteCommand
                | LapceWorkbenchCommand::PaletteLine
                | LapceWorkbenchCommand::PaletteSymbol
                | LapceWorkbenchCommand::PaletteWorkspace => return true,
                _ => {}
            }
        }

        false
    }
}

#[derive(PartialEq, Eq)]
pub enum CommandExecuted {
    No,
    Yes,
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
    #[strum(serialize = "change_color_theme")]
    #[strum(message = "Change Color Theme")]
    ChangeColorTheme, 

    #[strum(serialize = "change_icon_theme")]
    #[strum(message = "Change Icon Theme")]
    ChangeIconTheme,

    #[strum(serialize = "change_file_language")]
    #[strum(message = "Change current file language")]
    ChangeFileLanguage,

    #[strum(serialize = "source_control.checkout_branch")]
    CheckoutBranch,

    #[strum(serialize = "close_folder")]
    #[strum(message = "Close Folder")]
    CloseFolder,

    #[strum(serialize = "close_terminal_tab")]
    #[strum(message = "Close Terminal Tab")]
    CloseTerminalTab,

    #[strum(message = "Close Window")]
    #[strum(serialize = "close_window")]
    CloseWindow,

    #[strum(serialize = "close_window_tab")]
    #[strum(message = "Close Current Window Tab")]
    CloseWindowTab,

    #[strum(serialize = "connect_ssh_host")]
    #[strum(message = "Connect to SSH Host")]
    ConnectSshHost,

    #[strum(serialize = "connect_wsl")]
    #[strum(message = "Connect to WSL")]
    ConnectWsl,

    #[strum(serialize = "disable_modal_editing")]
    #[strum(message = "Disable Modal Editing")]
    DisableModal,

    #[strum(serialize = "disconnect_remote")]
    #[strum(message = "Disconnect From Remote")]
    DisconnectRemote,

    #[strum(serialize = "enable_modal_editing")]
    #[strum(message = "Enable Modal Editing")]
    EnableModal,

    #[strum(serialize = "export_current_theme_settings")]
    #[strum(message = "Export current settings to a theme file")]
    ExportCurrentThemeSettings,

    #[strum(serialize = "focus_editor")]
    FocusEditor,

    #[strum(serialize = "focus_terminal")]
    FocusTerminal,

    #[strum(serialize = "hide_panel")]
    HidePanel,

    #[strum(serialize = "install_theme")]
    #[strum(message = "Install current theme file")]
    InstallTheme,

    #[cfg(target_os = "macos")]
    #[strum(message = "Install Lapce to PATH")]
    #[strum(serialize = "install_to_path")]
    InstallToPATH,

    #[strum(serialize = "next_editor_tab")]
    #[strum(message = "Next editor tab")]
    NextEditorTab,

    #[strum(message = "New File")]
    #[strum(serialize = "new_file")]
    NewFile,

    #[strum(serialize = "new_terminal_tab")]
    #[strum(message = "Create New Terminal Tab")]
    NewTerminalTab,

    #[strum(message = "New Window")]
    #[strum(serialize = "new_window")]
    NewWindow,

    #[strum(serialize = "new_window_tab")]
    #[strum(message = "Create New Window Tab")]
    NewWindowTab,

    #[strum(serialize = "next_window_tab")]
    #[strum(message = "Go To Next Window Tab")]
    NextWindowTab,

    #[strum(serialize = "next_terminal_tab")]
    #[strum(message = "Next Terminal Tab")]
    NextTerminalTab,

    #[strum(serialize = "open_file")]
    #[strum(message = "Open File")]
    OpenFile,

    #[strum(serialize = "open_folder")]
    #[strum(message = "Open Folder")]
    OpenFolder,

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

    #[strum(serialize = "open_plugins_directory")]
    #[strum(message = "Open Plugins Directory")]
    OpenPluginsDirectory,

    #[strum(serialize = "open_proxy_directory")]
    #[strum(message = "Open Proxy Directory")]
    OpenProxyDirectory,

    #[strum(serialize = "open_settings")]
    #[strum(message = "Open Settings")]
    OpenSettings, 
 
    #[strum(serialize = "open_settings_directory")]
    #[strum(message = "Open Settings Directory")]
    OpenSettingsDirectory,

    #[strum(serialize = "open_settings_file")]
    #[strum(message = "Open Settings File")]
    OpenSettingsFile,

    #[strum(serialize = "open_themes_directory")]
    #[strum(message = "Open Themes Directory")]
    OpenThemesDirectory,

    #[strum(serialize = "palette")]
    #[strum(message = "Go to File")]
    Palette,

    #[strum(message = "Command Palette")]
    #[strum(serialize = "palette.command")]
    PaletteCommand,

    #[strum(serialize = "palette.line")]
    PaletteLine,

    #[strum(serialize = "palette.symbol")]
    PaletteSymbol,

    #[strum(message = "Open Recent Workspace")]
    #[strum(serialize = "palette.workspace")]
    PaletteWorkspace,

    #[strum(serialize = "palette.workspace_symbol")]
    PaletteWorkspaceSymbol,

    #[strum(serialize = "previous_editor_tab")]
    #[strum(message = "Previous editor tab")]
    PreviousEditorTab,

    #[strum(serialize = "previous_terminal_tab")]
    #[strum(message = "Previous Terminal Tab")]
    PreviousTerminalTab,

    #[strum(serialize = "previous_window_tab")]
    #[strum(message = "Go To Previous Window Tab")]
    PreviousWindowTab,

    #[strum(serialize = "quit")]
    #[strum(message = "Quit Editor")]
    Quit,

    #[strum(serialize = "reload_window")]
    #[strum(message = "Reload Window")]
    ReloadWindow,

    #[strum(serialize = "restart_to_update")]
    RestartToUpdate,

    #[strum(serialize = "reveal_active_file_in_file_explorer")]
    #[strum(message = "Reveal Active File in File Explorer")]
    RevealActiveFileInFileExplorer,

    #[strum(message = "Save All Files")]
    #[strum(serialize = "save_all")]
    SaveAll,

    #[strum(serialize = "show_about")]
    #[strum(message = "About Lapce")]
    ShowAbout,

    #[strum(serialize = "show_panel")]
    ShowPanel,

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

    #[strum(message = "Source Control: Init")]
    #[strum(serialize = "source_control_init")]
    SourceControlInit,

    #[strum(message = "Toggle File Explorer Focus")]
    #[strum(serialize = "toggle_file_explorer_focus")]
    ToggleFileExplorerFocus,

    #[strum(serialize = "toggle_file_explorer_visual")]
    ToggleFileExplorerVisual,

    #[strum(serialize = "toggle_inlay_hints")]
    #[strum(message = "Toggle Inlay Hints")]
    ToggleInlayHints,

    #[strum(serialize = "toggle_maximized_panel")]
    ToggleMaximizedPanel,

    #[strum(serialize = "toggle_panel_bottom_visual")]
    TogglePanelBottomVisual,

    /// Toggles the panel passed in parameter.
    #[strum(serialize = "toggle_panel_focus")]
    TogglePanelFocus,

    #[strum(serialize = "toggle_panel_left_visual")]
    TogglePanelLeftVisual,
 
    #[strum(serialize = "toggle_panel_right_visual")]
    TogglePanelRightVisual,

    /// Toggles the panel passed in parameter.
    #[strum(serialize = "toggle_panel_visual")]
    TogglePanelVisual,

    #[strum(message = "Toggle Plugin Focus")]
    #[strum(serialize = "toggle_plugin_focus")]
    TogglePluginFocus,

    #[strum(serialize = "toggle_plugin_visual")]
    TogglePluginVisual,

    #[strum(message = "Toggle Problem Focus")]
    #[strum(serialize = "toggle_problem_focus")]
    ToggleProblemFocus,

    #[strum(serialize = "toggle_problem_visual")]
    ToggleProblemVisual,

    #[strum(message = "Toggle Search Focus")]
    #[strum(serialize = "toggle_search_focus")]
    ToggleSearchFocus,

    #[strum(serialize = "toggle_search_visual")]
    ToggleSearchVisual,

    #[strum(serialize = "toggle_source_control_focus")]
    ToggleSourceControlFocus,

    #[strum(serialize = "toggle_source_control_visual")]
    ToggleSourceControlVisual,

    // Focus toggle commands
    #[strum(message = "Toggle Terminal Focus")]
    #[strum(serialize = "toggle_terminal_focus")]
    ToggleTerminalFocus,

    // Visual toggle commands
    #[strum(serialize = "toggle_terminal_visual")]
    ToggleTerminalVisual,

    #[cfg(target_os = "macos")]
    #[strum(message = "Uninstall Lapce from PATH")]
    #[strum(serialize = "uninstall_from_path")]
    UninstallFromPATH,
}

#[derive(Debug)]
pub enum EnsureVisiblePosition {
    // Cursor will be at the bottom edge, up by a margin.  If the cursor is near
    // the beginning of the buffer, the view might not change.
    BottomOfWindow, 
    // Move the view so the cursor line will be at the center of the window.  If
    // the cursor is near the beginning of the buffer, the view might not
    // change.
    CenterOfWindow,
    // Cursor will be at the top edge, down by a margin.
    TopOfWindow,
}

pub enum LapceUICommand {
    CancelFilePicker,
    CancelPalette,
    CenterOfWindow,
    ChildrenChanged,
    CloseTab,
    EditorContentChanged,
    EnsureEditorTabActiveVisible,
    FileExplorerRefresh,
    FilterItems,
    Focus,
    FocusEditor,
    FocusLost,
    FocusSourceControl,
    //FocusTab,
    Hide,
    InitChildren,
    ListItemSelected, /// An item in a list was chosen. This is typically targeted at the widget which contains the list
    LoadPluginsFailed,
    NextEditorTab,
    NextTab,
    PreviousEditorTab,
    PreviousTab,
    ReloadConfig,
    ReloadWindow,
    RequestLayout,
    RequestPaint,
    ResetFade,
    ResetSettings,
    ResignFocus,
    ShowAbout,
    ShowKeybindings,
    ShowSettings,
    ShowWindow,
    SplitClose,
    UpdateFailed,
    UpdateStarted,
    WorkspaceFileChange,

    ApplyWorkspaceEdit(WorkspaceEdit),
    CancelCompletion(usize),
    CloseBuffers(Vec<BufferId>),
    CloseMessage(WidgetId),
    CloseTabId(WidgetId),
    CloseTerminal(TermId),
    CloseWindow(WindowId),
    CopyPath(PathBuf),
    CopyRelativePath(PathBuf),
    DisableVolt(VoltInfo),
    DisableVoltWorkspace(VoltInfo),
    EditorViewSize(Size),
    EnableVolt(VoltInfo),
    EnableVoltWorkspace(VoltInfo),
    EnsureCursorPosition(EnsureVisiblePosition),
    EnsureCursorVisible(Option<EnsureVisiblePosition>),
    EnsureRectVisible(Rect),
    FocusTabId(WidgetId),
    GotoLocation(Location),
    HomeDir(PathBuf),
    InitBufferContent(InitBufferContent<usize>), /// UTF8 offsets into the file
    InitBufferContentLine(InitBufferContent<Line>), /// Start of line position
    InitBufferContentLineCol(InitBufferContent<LineCol>), /// Line and UTF8 Column Positions
    InitBufferContentLsp(InitBufferContent<Position>), /// UTF16 LSP positions
    InitPaletteInput(String),
    InitTerminalPanel(bool),
    LoadPlugins(PluginsInfo),
    LoadPluginLatest(VoltInfo),
    NewTab(Option<LapceWorkspace>),
    NewWindow(WindowId),
    OpenPluginInfo(VoltInfo),
    OpenURI(String),
    ProxyUpdateStatus(ProxyStatus),
    PublishDiagnostics(PublishDiagnosticsParams),
    PutToClipboard(String),
    RequestPaintRect(Rect),
    RevealInFileExplorer(PathBuf),
    RunPalette(Option<PaletteType>),
    RunPaletteReferences(Vec<EditorLocation<Position>>),
    Scroll((f64, f64)),
    ScrollTo((f64, f64)),
    SetLanguage(String),
    SetWorkspace(LapceWorkspace),
    ShowAlert(AlertContentData),   
    ShowCodeActions(Option<Point>),
    ShowSettingsKind(LapceSettingsKind),
    Split(bool),
    SplitChangeDirection(SplitDirection),
    SplitEditorClose(WidgetId),
    SplitEditorExchange(WidgetId),
    SplitExchange(SplitContent),
    SplitMove(SplitMoveDirection),
    SplitRemove(SplitContent),
    SwapTab(usize),
    TerminalJumpToLine(i32),
    ToggleProblem(PathBuf),
    UpdateDiffInfo(DiffInfo),
    UpdateSearch(String),
    UpdateSearchInput(String),
    UpdateKeymapsFilter(String),
    UpdateLatestRelease(ReleaseInfo),
    UpdateLineChanges(BufferId),
    UpdatePaletteInput(String),
    UpdatePickerPwd(PathBuf),
    UpdateSettingsFilter(String),
    UpdateVoltReadme(RichText),
    VoltInstallStatusClear(String),
    WorkDoneProgress(ProgressParams),

    DocumentSave(PathBuf, Option<WidgetId>),
    EditorTabAdd(usize, EditorTabChild),
    EditorTabSwap(usize, usize),
    ForceScrollTo(f64, f64),
    JumpToLine(Option<WidgetId>, usize),
    JumpToLineLocation(Option<WidgetId>, EditorLocation<Line>),
    JumpToPosition(Option<WidgetId>, Position, bool),
    LoadPluginIcon(String, VoltIconKind),
    OpenFile(PathBuf, bool),
    OpenFileDiff(PathBuf, String),
    PaletteReferences(usize, Vec<Location>),
    ResetSettingsFile(String, String),
    RestartToUpdate(PathBuf, ReleaseInfo),
    RunCodeAction(CodeActionOrCommand, PluginId),
    RunCommand(String, Vec<String>),
    SetColorTheme(String, bool),
    SetIconTheme(String, bool),
    ShowMenu(Point, Arc<Vec<MenuKind>>),
    SplitEditor(bool, WidgetId),
    SplitEditorMove(SplitMoveDirection, WidgetId),
    SplitReplace(usize, SplitContent),
    SplitTerminal(bool, WidgetId),
    SplitTerminalClose(TermId, WidgetId),
    TabToWindow(WindowId, WidgetId),
    UpdateHover(usize, Arc<Vec<RichText>>),
    UpdateKeymap(KeyMap, Vec<KeyPress>),
    UpdatePaletteItems(String, im::Vector<PaletteItem>),
    UpdateTerminalTitle(TermId, String),
    VoltInstalled(VoltMetadata, Option<String>),
    VoltInstalling(VoltInfo, String),
    VoltRemoved(VoltInfo, bool),
    VoltRemoving(VoltMetadata, String),

    ApplyEdits(usize, u64, Vec<TextEdit>),
    ApplyEditsAndSave(usize, u64, Result<Value>),
    BufferSave(PathBuf, u64, Option<WidgetId>),
    DocumentFormat(PathBuf, u64, Result<Vec<TextEdit>>),
    EditorTabRemove(usize, bool, bool),
    EnsureVisible((Rect, (f64, f64), Option<EnsureVisiblePosition>)),
    FilterKeymaps(String, Arc<Vec<KeyMap>>, Arc<Vec<LapceCommand>>),
    FilterPaletteItems(String, String, im::Vector<PaletteItem>),
    GlobalSearchResult(String, Arc<IndexMap<PathBuf, Vec<Match>>>),
    GoToLocation(Option<WidgetId>, EditorLocation, bool),
    JumpToLineColLocation(Option<WidgetId>, EditorLocation<LineCol>, bool),
    JumpToLocation(Option<WidgetId>, EditorLocation, bool),
    JumpToLspLocation(Option<WidgetId>, EditorLocation<Position>, bool),
    SplitAdd(usize, SplitContent, bool),
    UpdatePickerItems(PathBuf, HashMap<PathBuf, FileNodeItem>),
    UpdateSettingsFile(String, String, Value),

    DocumentFormatAndSave(PathBuf, u64, Result<Vec<TextEdit>>, Option<WidgetId>),
    ResolveCompletion(BufferId, u64, usize, Box<CompletionItem>),
    SaveAs(BufferContent, PathBuf, WidgetId, bool),
    UpdateCompletion(usize, String, CompletionResponse, PluginId),
    UpdateExplorerItems(PathBuf, HashMap<PathBuf, FileNodeItem>, bool),
    UpdateSemanticStyles(BufferId, PathBuf, u64, Arc<Spans<Style>>),

    SaveAsSuccess(BufferContent, u64, PathBuf, WidgetId, bool),



    ActiveFileChanged {
        path: Option<PathBuf>,
    },

    CreateDirectory {
        path: PathBuf,
    },

    /// Create a file in the given path with the given name and then open it
    CreateFileOpen {
        path: PathBuf,
    },

    ExplorerEndNaming {
        /// Whether it should name/rename the file with the input data
        apply_naming: bool,
    },

    ExplorerRevealPath {
        path: PathBuf,
    },

    /// Move a file/directory to the os-specific trash
    TrashPath {
        path: PathBuf,
    },



    OpenFileChanged {
        content: Rope,
        path: PathBuf,
    },

    RenamePath {
        from: PathBuf,
        to: PathBuf,
    },

    ShowGitBranches {
        branches: im::Vector<String>,
        origin: Point,
    },

    UpdateSearchWithCaseSensitivity {
        case_sensitive: bool,
        pattern: String,
    },

    UpdateSyntax {
        content: BufferContent,
        syntax: SingleUse<Syntax>,
    },



    ApplySelectionRange {
        buffer_id: BufferId,
        direction: SelectionRangeDirection,
        rev: u64,
    },

    CodeActionsError {
        offset: usize,
        path: PathBuf,
        rev: u64,
    },

    /// Start renaming a specific file in view at the given index
    ExplorerStartRename {
        /// The level that it should be indented to
        indent_level: usize,
        /// The index into the explorer's file listing
        list_index: usize,
        /// The text it will start with
        text: String,
    },

    GotoDefinition {
        editor_view_id: WidgetId,
        location: EditorLocation<Position>,
        offset: usize,
    },

    LoadBufferHead {
        content: Rope,
        path: PathBuf,
        version: String,
    },

    NewMessage {
        kind: MessageType,
        message: String,
        title: String,
    },

    OpenPaths {
        files: Vec<PathBuf>,
        folders: Vec<PathBuf>,
        window_tab_id: Option<(WindowId, WidgetId)>,
    },

    ReloadBuffer {
        content: Rope,
        path: PathBuf,
        rev: u64,
    },

    UpdateInlayHints {
        hints: Spans<InlayHint>,
        path: PathBuf,
        rev: u64,
    },

    UpdateSignature {
        plugin_id: PluginId,
        request_id: usize,
        resp: SignatureHelp,
    },



    /// Start creating a new file/directory
    ExplorerNew {
        /// The folder that it would be created in
        base_path: PathBuf,
        /// The level that it should be indented to
        indent_level: usize,
        /// Whether we are creating a file or a directory
        is_dir: bool,
        /// The index in the explorer's file listing that this should appear *after*
        list_index: usize,
    },

    LoadBufferAndGoToPosition {
        content: String,
        editor_view_id: WidgetId,
        location: EditorLocation,
        path: PathBuf,
    },

    UpdateHistoryStyle {
        highlights: Arc<Spans<Style>>,
        history: String,
        id: BufferId,
        path: PathBuf,
    },



    StoreSelectionRangeAndApply {
        buffer_id: BufferId,
        current_selection: Option<(usize, usize)>,
        direction: SelectionRangeDirection,
        ranges: Vec<SelectionRange>,
        rev: u64,
    },

    UpdateCodeActions {
        offset: usize,
        path: PathBuf,
        plugin_id: PluginId,
        resp: CodeActionResponse,
        rev: u64,
    },

    UpdateHistoryChanges {
        changes: Arc<Vec<DiffLines>>,
        history: String,
        id: BufferId,
        path: PathBuf,
        rev: u64,
    },



    PrepareRename {
        end: usize,
        offset: usize,
        path: PathBuf,
        placeholder: String,
        rev: u64,
        start: usize,
    },
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
    pub cb: Option<InitBufferContentCb>,
    pub content: Rope,
    pub edits: Option<Rope>,
    pub locations: Vec<(WidgetId, EditorLocation<P>)>,
    pub path: PathBuf,
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
