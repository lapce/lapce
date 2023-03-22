use std::path::PathBuf;

use crate::{command::LapceCommand, workspace::LapceWorkspace};

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub content: PaletteItemContent,
    pub filter_text: String,
    pub score: i64,
    pub indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteItemContent {
    File { path: PathBuf, full_path: PathBuf },
    Command { cmd: LapceCommand },
    Workspace { workspace: LapceWorkspace },
}
