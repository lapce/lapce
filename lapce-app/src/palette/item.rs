use std::path::PathBuf;

use lsp_types::{Range, SymbolKind};

use crate::{
    command::LapceCommand, editor::location::EditorLocation,
    workspace::LapceWorkspace,
};

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub content: PaletteItemContent,
    pub filter_text: String,
    pub score: i64,
    pub indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteItemContent {
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
}
