use lsp_types::{ProgressParams, PublishDiagnosticsParams};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

use crate::{
    file::FileNodeItem, plugin::PluginDescription, source_control::DiffInfo,
    terminal::TermId, RequestId, RpcError, RpcMessage,
};

pub type CoreRpcMessage = RpcMessage<CoreRequest, CoreNotification, CoreResponse>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreRpc {
    Notificiation(CoreNotification),
    Response(RequestId, CoreResponse),
    Error(RequestId, RpcError),
}

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
    WorkspaceFileChange {},
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
    DisabledPlugins {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum CoreResponse {
    NewBufferResponse {
        content: String,
    },
    BufferHeadResponse {
        version: String,
        content: String,
    },
    ReadDirResponse {
        items: HashMap<PathBuf, FileNodeItem>,
    },
    GetFilesResponse {
        items: Vec<PathBuf>,
    },
}
