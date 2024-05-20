use std::path::PathBuf;

use lapce_core::line_ending::LineEnding;
use lapce_rpc::dap_types::RunDebugConfig;
use lsp_types::{Range, SymbolKind};

use crate::{
    command::{LapceCommand, LapceWorkbenchCommand},
    debug::RunDebugMode,
    editor::location::EditorLocation,
    workspace::{self, LapceWorkspace},
};

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub content: PaletteItemContent,
    pub filter_text: String,
    pub score: u32,
    pub indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteItemContent {
    PaletteHelp {
        cmd: LapceWorkbenchCommand,
    },
    File {
        path: PathBuf,
        full_path: PathBuf,
    },
    Line {
        line: usize,
        content: String,
    },
    Command {
        cmd: LapceCommand,
    },
    Workspace {
        workspace: LapceWorkspace,
    },
    Reference {
        path: PathBuf,
        location: EditorLocation,
    },
    DocumentSymbol {
        kind: SymbolKind,
        name: String,
        range: Range,
        container_name: Option<String>,
    },
    WorkspaceSymbol {
        kind: SymbolKind,
        name: String,
        container_name: Option<String>,
        location: EditorLocation,
    },
    SshHost {
        host: workspace::ssh::Host,
    },
    GhHost {
        host: workspace::gh::Host,
    },
    #[cfg(windows)]
    WslHost {
        host: workspace::wsl::Host,
    },
    RunAndDebug {
        mode: RunDebugMode,
        config: RunDebugConfig,
    },
    ColorTheme {
        name: String,
    },
    IconTheme {
        name: String,
    },
    Language {
        name: String,
    },
    LineEnding {
        kind: LineEnding,
    },
    SCMReference {
        name: String,
    },
    TerminalProfile {
        name: String,
        profile: lapce_rpc::terminal::TerminalProfile,
    },
}
