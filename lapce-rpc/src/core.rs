use lsp_types::{ProgressParams, PublishDiagnosticsParams};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

use crate::{
    buffer::BufferId, file::FileNodeItem, plugin::PluginDescription,
    source_control::DiffInfo, style::LineStyle, terminal::TermId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreNotification {
    ProxyConnected {},
    SemanticStyles {
        rev: u64,
        buffer_id: BufferId,
        path: PathBuf,
        len: usize,
        styles: Vec<LineStyle>,
    },
    ReloadBuffer {
        buffer_id: BufferId,
        new_content: String,
        rev: u64,
    },
    FileChange {
        event: notify::Event,
    },
    PublishDiagnostics {
        diagnostics: PublishDiagnosticsParams,
    },
    WorkDoneProgress {
        progress: ProgressParams,
    },
    HomeDir {
        path: PathBuf,
    },
    InstalledPlugins {
        plugins: HashMap<String, PluginDescription>,
    },
    ListDir {
        items: Vec<FileNodeItem>,
    },
    DiffFiles {
        files: Vec<PathBuf>,
    },
    DiffInfo {
        diff: DiffInfo,
    },
    UpdateTerminal {
        term_id: TermId,
        content: String,
    },
    CloseTerminal {
        term_id: TermId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreRequest {}
