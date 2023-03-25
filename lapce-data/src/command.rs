use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Result;
use druid::{
    EventCtx, FileInfo, Point, Rect, Selector, SingleUse, WidgetId, WindowId,
};
use indexmap::IndexMap;
use lapce_core::{
    buffer::DiffLines,
    command::{
        EditCommand, FocusCommand, MotionModeCommand, MoveCommand,
        MultiSelectionCommand,
    },
    movement::LineCol,
    syntax::Syntax,
};
use lapce_rpc::{
    buffer::BufferId,
    file::FileNodeItem,
    plugin::{PluginId, VoltID, VoltInfo, VoltMetadata},
    source_control::DiffInfo,
    style::Style,
    terminal::TermId,
};
use lapce_xi_rope::{spans::Spans, Rope};
use lsp_types::{
    CodeActionOrCommand, CodeActionResponse, CompletionItem, CompletionResponse,
    InlayHint, Location, MessageType, Position, ProgressParams,
    PublishDiagnosticsParams, SelectionRange, SignatureHelp, TextEdit, Url,
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
    editor::{EditorLocation, EditorPosition, Line},
    images,
    keypress::{KeyMap, KeyPress},
    markdown::Content,
    menu::MenuKind,
    palette::{PaletteItem, PaletteType},
    plugin::{PluginsInfo, VoltIconKind},
    proxy::ProxyStatus,
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
                | LapceWorkbenchCommand::PaletteWorkspace => return true,
                #[cfg(windows)]
                LapceWorkbenchCommand::ConnectWsl => return true,
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

    #[cfg(windows)]
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
    /// Reload the config file
    ReloadConfig,
    /// Initializes the buffer content at a particular position.  
    /// (UTF8 offsets into the file)
    InitBufferContent(InitBufferContent<usize>),
    /// Initializes the buffer content at a particular position.  
    /// (Start of line position)
    InitBufferContentLine(InitBufferContent<Line>),
    /// Initializes the buffer content at a particular position.  
    /// (Line and UTF8 Column Positions)
    InitBufferContentLineCol(InitBufferContent<LineCol>),
    /// Initializes the buffer content at a particular position.  
    /// (UTF16 LSP positions)
    InitBufferContentLsp(InitBufferContent<Position>),
    /// Informs the editor that the file has been changed, giving the new content.  
    /// (Sent by the proxy on certain watcher events)
    OpenFileChanged {
        path: PathBuf,
        content: Rope,
    },
    ReloadBuffer {
        path: PathBuf,
        rev: u64,
        content: Rope,
    },
    /// Informs the editor of the `head` version of the buffer.  
    /// (Sent by the proxy when history is requested for a document)
    LoadBufferHead {
        path: PathBuf,
        version: String,
        content: Rope,
    },
    /// Display the about modal
    ShowAbout,
    /// Display the alert modal (ex: the "Are you sure you want to close without saving?")
    ShowAlert(AlertContentData),
    /// Display the context menu at a specific point
    ShowMenu(Point, Arc<Vec<MenuKind>>),
    /// Bring the window to the front
    ShowWindow,
    /// Display the git branch picker at the specified point
    ShowGitBranches {
        origin: Point,
        branches: im::Vector<String>,
    },
    /// Update global search input with the given pattern
    UpdateSearchInput(String),
    /// Update the global search with the given pattern (actually performs the search)
    UpdateSearch(
        String,
        // If present, will update the case-sensitivity
        Option<bool>,
    ),
    /// Informs the editor of the results from the global search, this is caused by the
    /// `UpdateSearch{,WithCaseSensitivity}` commands
    GlobalSearchResult(String, Arc<IndexMap<PathBuf, Vec<Match>>>),
    CancelFilePicker,
    /// Change the workspace to the given path/remote (or clear it)
    SetWorkspace(LapceWorkspace),
    SetColorTheme {
        theme: String,
        /// Whether the changes are temporary, and thus whether we should update the config file
        preview: bool,
    },
    SetIconTheme {
        theme: String,
        /// Whether the changes are temporary, and thus whether we should update the config file
        preview: bool,
    },
    UpdateKeymap(KeyMap, Vec<KeyPress>),
    /// Open the URI in the respective program, such as urls for the browser, or paths in the file
    /// explorer
    OpenURI(String),
    /// Open multiple folders/files in the editor, potentially as new window tabs
    OpenPaths {
        window_tab_id: Option<(WindowId, WidgetId)>,
        folders: Vec<PathBuf>,
        files: Vec<PathBuf>,
    },
    /// Open a specific file in the editor; along with `same_tab` which decides which tabs to look
    /// at for whether the file is already open.
    OpenFile(PathBuf, bool),
    /// Open a specific file in the editor as a source control diff view
    OpenFileDiff {
        path: PathBuf,
        /// Ex: "head"
        history: String,
    },
    /// Shows a specific file in the user's file explorer
    RevealInFileExplorer(PathBuf),
    /// Cancel the completion request
    CancelCompletion {
        request_id: usize,
    },
    /// Receieved when the request for completion items has completed
    UpdateCompletion {
        request_id: usize,
        input: String,
        resp: CompletionResponse,
        plugin_id: PluginId,
    },
    /// Received when the completion item has been selected and we've resolved it to the actual
    /// actions needed to expand it.
    ResolveCompletion {
        id: BufferId,
        rev: u64,
        offset: usize,
        item: Box<CompletionItem>,
    },
    /// Completion 'internal' event that indicates that it should recompute the layouts for
    /// the completion documentation.
    RefreshCompletionDocumentation,
    /// Received when the request for signature information has completed
    UpdateSignature {
        request_id: usize,
        resp: SignatureHelp,
        plugin_id: PluginId,
    },
    /// Signature 'internal' event that indicates that it should recompute the layouts for
    /// the signature view.
    RefreshSignature,
    /// Received when the request for hover information has completed
    UpdateHover {
        request_id: usize,
        items: Arc<Vec<Content>>,
    },
    /// Received when the request for the plugin's description completed
    UpdateVoltReadme(Arc<Vec<Content>>),
    UpdateInlayHints {
        path: PathBuf,
        rev: u64,
        hints: Spans<InlayHint>,
    },
    /// Received when the request for code actions in the file completed
    UpdateCodeActions {
        path: PathBuf,
        plugin_id: PluginId,
        rev: u64,
        offset: usize,
        resp: CodeActionResponse,
    },
    /// Received when there was an error in getting code actions
    CodeActionsError {
        path: PathBuf,
        rev: u64,
        offset: usize,
    },
    /// Run a system command with the given args
    RunCommand(String, Vec<String>),
    /// Execute a code action from a specific plugin
    RunCodeAction(CodeActionOrCommand, PluginId),
    /// Apply a workspace edit, which comes from an LSP
    ApplyWorkspaceEdit(WorkspaceEdit),
    /// Display a list of the current code actions at the given point
    ShowCodeActions(Option<Point>),
    /// Sets the information about the latest Lapce release
    UpdateLatestRelease(ReleaseInfo),
    /// Hide the widget which receives this (requires that it handles this)
    Hide,
    /// Focus the widget which receives this
    Focus,
    /// Inform the widget that they lost focus (not often used)
    FocusLost,
    /// Inform that the widget that their children have changed, typically so that they can call
    /// `ctx.children_changed()` on the next `event`
    ChildrenChanged,
    /// Changes the active file in the explorer panel to the current file
    EnsureEditorTabActiveVisible,
    /// Displays the (core) settings in the settings view
    ShowSettings,
    /// Displays the keybindings in the settings view
    ShowKeybindings,
    /// Displays the given settings in the settings view
    ShowSettingsKind(LapceSettingsKind),
    /// Display the palette, initialized to a specific type
    RunPalette(Option<PaletteType>),
    /// Receives the positions of the requested references
    RunPaletteReferences(Vec<EditorLocation<Position>>),
    /// Sets the palette's input to the given string. This changes it without updating.
    InitPaletteInput(String),
    /// Sets the palette's input to the given string. This updates the contents of the palette
    /// based on the input.
    UpdatePaletteInput(String),
    /// Event received to set the palette's items after they were loaded
    UpdatePaletteItems {
        run_id: String,
        items: im::Vector<PaletteItem>,
    },
    /// Event received to set the palette's items after they were filtered
    FilterPaletteItems {
        run_id: String,
        input: String,
        filtered_items: im::Vector<PaletteItem>,
    },
    /// Set the filter for the keymaps (in the settings), updating which keymaps are shown
    UpdateKeymapsFilter(String),
    /// Reset a specific settings item to its default value by sending it to the widget  
    /// (note: only handled by theme settings items currently)
    ResetSettingsItem,
    /// Reset a specific settings item to its default value by the path to it
    ResetSettingsFile {
        kind: String,
        key: String,
    },
    /// Set a specific settings item to a new value by the path to it
    UpdateSettingsFile {
        kind: String,
        key: String,
        value: Value,
    },
    /// Update the filter for the settings
    /// TODO: currently unused!
    UpdateSettingsFilter(String),
    FilterKeymaps {
        pattern: String,
        /// The filtered keymaps
        keymaps: Arc<Vec<KeyMap>>,
        /// The filtered commands
        commands: Arc<Vec<LapceCommand>>,
    },
    UpdatePickerPwd(PathBuf),
    UpdatePickerItems(PathBuf, HashMap<PathBuf, FileNodeItem>),
    /// Event received when the directory of a folder has been read, so that we can include the new
    /// files in the explorer.
    UpdateExplorerItems {
        /// The path of the folder we're updating the items of
        path: PathBuf,
        /// The items within the folder
        items: HashMap<PathBuf, FileNodeItem>,
        /// Whether the folder should be open or not
        expand: bool,
    },
    LoadPluginLatest(VoltInfo),
    /// Event to inform what plugins the user has installed, and thus should be loaded.
    LoadPlugins(PluginsInfo),
    LoadPluginsFailed,
    /// Event received when the plugin's icon has been loaded
    LoadPluginIcon(VoltID, VoltIconKind),
    VoltInstalled(VoltMetadata, Option<Vec<u8>>),
    VoltInstalling(VoltInfo, String),
    VoltRemoving(VoltMetadata, String),
    VoltInstallStatusClear(VoltID),
    VoltRemoved(VoltInfo, bool),
    EnableVolt(VoltInfo),
    DisableVolt(VoltInfo),
    EnableVoltWorkspace(VoltInfo),
    DisableVoltWorkspace(VoltInfo),
    RequestLayout,
    RequestPaint,
    ResetFade,
    /// Close a window tab (which is distinct from the typical editor tab!)
    CloseTab,
    /// Close a window tab by its id
    CloseTabId(WidgetId),
    /// Focus on a window tab by its id
    FocusTabId(WidgetId),
    /// Swap the active window tab with the window tab at the index
    SwapTab(usize),
    /// Move a window tab out into its own window
    TabToWindow(WindowId, WidgetId),
    /// Create a new window tab, optionally with a specific workspace
    NewTab(Option<LapceWorkspace>),
    /// Switch to the next window tab (in terms of order, not usage)
    NextTab,
    /// Switch to the previous window tab (in terms of order, not usage)
    PreviousTab,
    /// Switch to the next editor tab (in terms of order, not usage)
    NextEditorTab,
    /// Switch to the previous editor tab (in terms of order, not usage)
    PreviousEditorTab,
    /// Restart Lapce at the given path so that we can apply the update
    RestartToUpdate(PathBuf, ReleaseInfo),
    UpdateStarted,
    UpdateFailed,
    /// Create a new Lapce window
    NewWindow(WindowId),
    /// Close a Lapce window, saving the DB as needed
    CloseWindow(WindowId),
    /// Reload the current Lapce window
    ReloadWindow,
    /// Event received when the formatting request has been completed, which formats the document
    /// with the given path using the edits.
    DocumentFormat {
        path: PathBuf,
        rev: u64,
        /// The resulting edits
        result: Result<Vec<TextEdit>>,
    },
    /// Event received when the formatting request has been completed, and the document should be
    /// saved with the given path using the edits.
    DocumentFormatAndSave {
        path: PathBuf,
        rev: u64,
        result: Result<Vec<TextEdit>>,
        exit: Option<WidgetId>,
    },
    /// Save the document with the given path
    DocumentSave {
        path: PathBuf,
        exit: Option<WidgetId>,
    },
    /// Mark the document as saved/pristine, if the revision still matches
    BufferSave {
        path: PathBuf,
        rev: u64,
        exit: Option<WidgetId>,
    },
    /// Update the semantic styles for the document with the given path
    UpdateSemanticStyles {
        // TODO: This doesn't actually use the buffer id, perhaps it should just be removed?
        id: BufferId,
        path: PathBuf,
        rev: u64,
        styles: Arc<Spans<Style>>,
    },
    /// Set the terminal's title
    UpdateTerminalTitle(TermId, String),
    UpdateHistoryStyle {
        id: BufferId,
        path: PathBuf,
        history: String,
        highlights: Arc<Spans<Style>>,
    },
    /// Update the syntax highlighting for the document with the given content
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
        diff_context_lines: i32,
    },
    /// Publish diagnostics changes (from the proxy)
    PublishDiagnostics(PublishDiagnosticsParams),
    /// Update the current progress information (from the proxy)
    WorkDoneProgress(ProgressParams),
    UpdateDiffInfo(DiffInfo),
    /// Scrolls the editor-view so that the rect is visible  
    EnsureRectVisible(Rect),
    /// Scrolls the editor-view so that the cursor is visible
    EnsureCursorVisible(Option<EnsureVisiblePosition>),
    EnsureCursorPosition(EnsureVisiblePosition),
    /// Scroll the editor-view by the given amount
    Scroll((f64, f64)),
    /// Scroll the editor-view to the given point
    ScrollTo((f64, f64)),
    ForceScrollTo(f64, f64),
    /// Save the given content to the path
    SaveAs {
        content: BufferContent,
        path: PathBuf,
        view_id: WidgetId,
        exit: bool,
    },
    /// Event received when save-as succeeds, used for updating the existing tab
    SaveAsSuccess {
        content: BufferContent,
        rev: u64,
        path: PathBuf,
        view_id: WidgetId,
        exit: bool,
    },
    /// Sets the picker home directory
    HomeDir(PathBuf),
    /// Event received from the proxy when a file has changed in the current workspace, used for
    /// updating the file explorer.
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
    /// Informs the editor about the locations for requested references
    PaletteReferences(usize, Vec<Location>),
    /// Update the current file highlighted in the explorer panel
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
    /// Copy an existing file to the given name and then open it
    DuplicateFileOpen {
        existing_path: PathBuf,
        new_path: PathBuf,
    },
    RenamePath {
        from: PathBuf,
        to: PathBuf,
    },
    /// Move a file/directory to the os-specific trash
    TrashPath {
        path: PathBuf,
    },
    /// Start duplicating a specific file in view at the given index
    ExplorerStartDuplicate {
        /// The index into the explorer's file listing
        list_index: usize,
        /// The level that it should be indented to
        indent_level: usize,
        /// The folder that the file/directory is being created within
        base_path: PathBuf,
        /// The name of the file being duplicated
        name: String,
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
    /// An item in the dropdown list was selected as the current item  
    /// This is typically targeted at the widget which contains the dropdown
    DropdownItemSelected,
    NewMessage {
        kind: MessageType,
        title: String,
        message: String,
    },
    CloseMessage(WidgetId),
    ImageLoaded {
        url: Url,
        image: Result<images::Image, anyhow::Error>,
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
