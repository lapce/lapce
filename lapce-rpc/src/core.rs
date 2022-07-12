use lsp_types::{ProgressParams, PublishDiagnosticsParams};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[cfg(feature = "terminal")]
use crate::terminal::TermId;
use crate::{
    file::FileNodeItem, plugin::PluginDescription, source_control::DiffInfo,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreNotification {
    ProxyConnected {},
    OpenFileChanged {
        path: PathBuf,
        content: String,
    },
    ReloadBuffer {
        path: PathBuf,
        content: String,
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
    #[cfg(feature = "terminal")]
    UpdateTerminal {
        term_id: TermId,
        content: String,
    },
    #[cfg(feature = "terminal")]
    CloseTerminal {
        term_id: TermId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreRequest {}
