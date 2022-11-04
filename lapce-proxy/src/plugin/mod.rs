pub mod catalog;
pub mod lsp;
pub mod psp;
pub mod wasi;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use dyn_clone::DynClone;
use flate2::read::GzDecoder;
use lapce_core::directory::Directory;
use lapce_rpc::{
    core::CoreRpcHandler,
    plugin::{PluginId, VoltInfo, VoltMetadata},
    proxy::ProxyRpcHandler,
    style::LineStyle,
    RequestId, RpcError,
};
use lapce_xi_rope::{Rope, RopeDelta};
use lsp_types::{
    request::{
        CodeActionRequest, CodeActionResolveRequest, Completion,
        DocumentSymbolRequest, Formatting, GotoDefinition, GotoTypeDefinition,
        GotoTypeDefinitionParams, GotoTypeDefinitionResponse, HoverRequest,
        InlayHintRequest, PrepareRenameRequest, References, Rename, Request,
        ResolveCompletionItem, SelectionRangeRequest, SemanticTokensFullRequest,
        WorkspaceSymbol,
    },
    CodeAction, CodeActionContext, CodeActionParams, CodeActionResponse,
    CompletionItem, CompletionParams, CompletionResponse, DocumentFormattingParams,
    DocumentSymbolParams, DocumentSymbolResponse, FormattingOptions,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams, InlayHint,
    InlayHintParams, Location, PartialResultParams, Position, PrepareRenameResponse,
    Range, ReferenceContext, ReferenceParams, RenameParams, SelectionRange,
    SelectionRangeParams, SemanticTokens, SemanticTokensParams, SymbolInformation,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, TextEdit,
    Url, VersionedTextDocumentIdentifier, WorkDoneProgressParams, WorkspaceEdit,
    WorkspaceSymbolParams,
};
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tar::Archive;

use self::{
    catalog::PluginCatalog,
    psp::{ClonableCallback, PluginServerRpcHandler, RpcCallback},
    wasi::{load_volt, start_volt},
};
use crate::buffer::language_id_from_path;

pub type PluginName = String;

#[allow(clippy::large_enum_variant)]
pub enum PluginCatalogRpc {
    ServerRequest {
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        method: &'static str,
        params: Value,
        language_id: Option<String>,
        path: Option<PathBuf>,
        f: Box<dyn ClonableCallback>,
    },
    ServerNotification {
        method: &'static str,
        params: Value,
        language_id: Option<String>,
        path: Option<PathBuf>,
    },
    FormatSemanticTokens {
        plugin_id: PluginId,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    },
    DidOpenTextDocument {
        document: TextDocumentItem,
    },
    DidChangeTextDocument {
        language_id: String,
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
    },
    DidSaveTextDocument {
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: Rope,
    },
    Handler(PluginCatalogNotification),
    Shutdown,
}

#[allow(clippy::large_enum_variant)]
pub enum PluginCatalogNotification {
    UpdatePluginConfigs(HashMap<String, HashMap<String, serde_json::Value>>),
    UnactivatedVolts(Vec<VoltMetadata>),
    PluginServerLoaded(PluginServerRpcHandler),
    InstallVolt(VoltInfo),
    StopVolt(VoltInfo),
    EnableVolt(VoltInfo),
    ReloadVolt(VoltMetadata),
    Shutdown,
}

#[derive(Clone)]
pub struct PluginCatalogRpcHandler {
    core_rpc: CoreRpcHandler,
    proxy_rpc: ProxyRpcHandler,
    plugin_tx: Sender<PluginCatalogRpc>,
    plugin_rx: Arc<Mutex<Option<Receiver<PluginCatalogRpc>>>>,
    #[allow(dead_code)]
    id: Arc<AtomicU64>,
    #[allow(dead_code, clippy::type_complexity)]
    pending: Arc<Mutex<HashMap<u64, Sender<Result<Value, RpcError>>>>>,
}

impl PluginCatalogRpcHandler {
    pub fn new(core_rpc: CoreRpcHandler, proxy_rpc: ProxyRpcHandler) -> Self {
        let (plugin_tx, plugin_rx) = crossbeam_channel::unbounded();
        Self {
            core_rpc,
            proxy_rpc,
            plugin_tx,
            plugin_rx: Arc::new(Mutex::new(Some(plugin_rx))),
            id: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[allow(dead_code)]
    fn handle_response(&self, id: RequestId, result: Result<Value, RpcError>) {
        if let Some(chan) = { self.pending.lock().remove(&id) } {
            let _ = chan.send(result);
        }
    }

    pub fn mainloop(&self, plugin: &mut PluginCatalog) {
        let plugin_rx = self.plugin_rx.lock().take().unwrap();
        for msg in plugin_rx {
            match msg {
                PluginCatalogRpc::ServerRequest {
                    plugin_id,
                    request_sent,
                    method,
                    params,
                    language_id,
                    path,
                    f,
                } => {
                    plugin.handle_server_request(
                        plugin_id,
                        request_sent,
                        method,
                        params,
                        language_id,
                        path,
                        f,
                    );
                }
                PluginCatalogRpc::ServerNotification {
                    method,
                    params,
                    language_id,
                    path,
                } => {
                    plugin.handle_server_notification(
                        method,
                        params,
                        language_id,
                        path,
                    );
                }
                PluginCatalogRpc::Handler(notification) => {
                    plugin.handle_notification(notification);
                }
                PluginCatalogRpc::FormatSemanticTokens {
                    plugin_id,
                    tokens,
                    text,
                    f,
                } => {
                    plugin.format_semantic_tokens(plugin_id, tokens, text, f);
                }
                PluginCatalogRpc::DidOpenTextDocument { document } => {
                    plugin.handle_did_open_text_document(document);
                }
                PluginCatalogRpc::DidSaveTextDocument {
                    language_id,
                    path,
                    text_document,
                    text,
                } => {
                    plugin.handle_did_save_text_document(
                        language_id,
                        path,
                        text_document,
                        text,
                    );
                }
                PluginCatalogRpc::DidChangeTextDocument {
                    language_id,
                    document,
                    delta,
                    text,
                    new_text,
                } => {
                    plugin.handle_did_change_text_document(
                        language_id,
                        document,
                        delta,
                        text,
                        new_text,
                    );
                }
                PluginCatalogRpc::Shutdown => {
                    return;
                }
            }
        }
    }

    pub fn shutdown(&self) {
        let _ = self.catalog_notification(PluginCatalogNotification::Shutdown);
        let _ = self.plugin_tx.send(PluginCatalogRpc::Shutdown);
    }

    fn catalog_notification(
        &self,
        notification: PluginCatalogNotification,
    ) -> Result<()> {
        self.plugin_tx
            .send(PluginCatalogRpc::Handler(notification))
            .map_err(|e| anyhow!(e.to_string()))?;
        Ok(())
    }

    fn send_request_to_all_plugins<P, Resp>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        path: Option<PathBuf>,
        cb: impl FnOnce(PluginId, Result<Resp, RpcError>) + Clone + Send + 'static,
    ) where
        P: Serialize,
        Resp: DeserializeOwned,
    {
        let got_success = Arc::new(AtomicBool::new(false));
        let request_sent = Arc::new(AtomicUsize::new(0));
        let err_received = Arc::new(AtomicUsize::new(0));
        self.send_request(
            None,
            Some(request_sent.clone()),
            method,
            params,
            language_id,
            path,
            move |plugin_id, result| {
                if got_success.load(Ordering::Acquire) {
                    return;
                }
                let result = match result {
                    Ok(value) => {
                        if let Ok(item) = serde_json::from_value::<Resp>(value) {
                            got_success.store(true, Ordering::Release);
                            Ok(item)
                        } else {
                            Err(RpcError {
                                code: 0,
                                message: "deserialize error".to_string(),
                            })
                        }
                    }
                    Err(e) => Err(e),
                };
                if result.is_ok() {
                    cb(plugin_id, result)
                } else {
                    let rx = err_received.fetch_add(1, Ordering::Relaxed) + 1;
                    if request_sent.load(Ordering::Acquire) == rx {
                        cb(plugin_id, result)
                    }
                }
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn send_request<P: Serialize>(
        &self,
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        path: Option<PathBuf>,
        f: impl FnOnce(PluginId, Result<Value, RpcError>) + Send + DynClone + 'static,
    ) {
        let params = serde_json::to_value(params).unwrap();
        let rpc = PluginCatalogRpc::ServerRequest {
            plugin_id,
            request_sent,
            method,
            params,
            language_id,
            path,
            f: Box::new(f),
        };
        let _ = self.plugin_tx.send(rpc);
    }

    pub fn format_semantic_tokens(
        &self,
        plugin_id: PluginId,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    ) {
        let _ = self.plugin_tx.send(PluginCatalogRpc::FormatSemanticTokens {
            plugin_id,
            tokens,
            text,
            f,
        });
    }

    pub fn did_save_text_document(&self, path: &Path, text: Rope) {
        let text_document =
            TextDocumentIdentifier::new(Url::from_file_path(path).unwrap());
        let language_id = language_id_from_path(path).unwrap_or("").to_string();
        let _ = self.plugin_tx.send(PluginCatalogRpc::DidSaveTextDocument {
            language_id,
            text_document,
            path: path.into(),
            text,
        });
    }

    pub fn did_change_text_document(
        &self,
        path: &Path,
        rev: u64,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
    ) {
        let document = VersionedTextDocumentIdentifier::new(
            Url::from_file_path(path).unwrap(),
            rev as i32,
        );
        let language_id = language_id_from_path(path).unwrap_or("").to_string();
        let _ = self
            .plugin_tx
            .send(PluginCatalogRpc::DidChangeTextDocument {
                language_id,
                document,
                delta,
                text,
                new_text,
            });
    }

    pub fn get_definition(
        &self,
        path: &Path,
        position: Position,
        cb: impl FnOnce(PluginId, Result<GotoDefinitionResponse, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = GotoDefinition::METHOD;
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_type_definition(
        &self,
        path: &Path,
        position: Position,
        cb: impl FnOnce(PluginId, Result<GotoTypeDefinitionResponse, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = GotoTypeDefinition::METHOD;
        let params = GotoTypeDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_references(
        &self,
        path: &Path,
        position: Position,
        cb: impl FnOnce(PluginId, Result<Vec<Location>, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = References::METHOD;
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: false,
            },
        };

        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_code_actions(
        &self,
        path: &Path,
        position: Position,
        cb: impl FnOnce(PluginId, Result<CodeActionResponse, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = CodeActionRequest::METHOD;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range {
                start: position,
                end: position,
            },
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_inlay_hints(
        &self,
        path: &Path,
        range: Range,
        cb: impl FnOnce(PluginId, Result<Vec<InlayHint>, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = InlayHintRequest::METHOD;
        let params = InlayHintParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            range,
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_document_symbols(
        &self,
        path: &Path,
        cb: impl FnOnce(PluginId, Result<DocumentSymbolResponse, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = DocumentSymbolRequest::METHOD;
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_workspace_symbols(
        &self,
        query: String,
        cb: impl FnOnce(PluginId, Result<Vec<SymbolInformation>, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let method = WorkspaceSymbol::METHOD;
        let params = WorkspaceSymbolParams {
            query,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.send_request_to_all_plugins(method, params, None, None, cb);
    }

    pub fn get_document_formatting(
        &self,
        path: &Path,
        cb: impl FnOnce(PluginId, Result<Vec<TextEdit>, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = Formatting::METHOD;
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                ..Default::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn prepare_rename(
        &self,
        path: &Path,
        position: Position,
        cb: impl FnOnce(PluginId, Result<PrepareRenameResponse, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = PrepareRenameRequest::METHOD;
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn rename(
        &self,
        path: &Path,
        position: Position,
        new_name: String,
        cb: impl FnOnce(PluginId, Result<WorkspaceEdit, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = Rename::METHOD;
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            new_name,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_semantic_tokens(
        &self,
        path: &Path,
        cb: impl FnOnce(PluginId, Result<SemanticTokens, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = SemanticTokensFullRequest::METHOD;
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn get_selection_range(
        &self,
        path: &Path,
        positions: Vec<Position>,
        cb: impl FnOnce(PluginId, Result<Vec<SelectionRange>, RpcError>)
            + Clone
            + Send
            + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = SelectionRangeRequest::METHOD;
        let params = SelectionRangeParams {
            text_document: TextDocumentIdentifier { uri },
            positions,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: Default::default(),
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn hover(
        &self,
        path: &Path,
        position: Position,
        cb: impl FnOnce(PluginId, Result<Hover, RpcError>) + Clone + Send + 'static,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = HoverRequest::METHOD;
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());

        self.send_request_to_all_plugins(
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            cb,
        );
    }

    pub fn completion(
        &self,
        request_id: usize,
        path: &Path,
        input: String,
        position: Position,
    ) {
        let uri = Url::from_file_path(path).unwrap();
        let method = Completion::METHOD;
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };

        let core_rpc = self.core_rpc.clone();
        let language_id =
            Some(language_id_from_path(path).unwrap_or("").to_string());
        self.send_request(
            None,
            None,
            method,
            params,
            language_id,
            Some(path.to_path_buf()),
            move |plugin_id, result| {
                if let Ok(value) = result {
                    if let Ok(resp) =
                        serde_json::from_value::<CompletionResponse>(value)
                    {
                        core_rpc
                            .completion_response(request_id, input, resp, plugin_id);
                    }
                }
            },
        );
    }

    pub fn completion_resolve(
        &self,
        plugin_id: PluginId,
        item: CompletionItem,
        cb: impl FnOnce(Result<CompletionItem, RpcError>) + Send + Clone + 'static,
    ) {
        let method = ResolveCompletionItem::METHOD;
        self.send_request(
            Some(plugin_id),
            None,
            method,
            item,
            None,
            None,
            move |_, result| {
                let result = match result {
                    Ok(value) => {
                        if let Ok(item) =
                            serde_json::from_value::<CompletionItem>(value)
                        {
                            Ok(item)
                        } else {
                            Err(RpcError {
                                code: 0,
                                message: "completion item deserialize error"
                                    .to_string(),
                            })
                        }
                    }
                    Err(e) => Err(e),
                };
                cb(result)
            },
        );
    }

    pub fn action_resolve(
        &self,
        item: CodeAction,
        plugin_id: PluginId,
        cb: impl FnOnce(Result<CodeAction, RpcError>) + Send + Clone + 'static,
    ) {
        let method = CodeActionResolveRequest::METHOD;
        self.send_request(
            Some(plugin_id),
            None,
            method,
            item,
            None,
            None,
            move |_, result| {
                let result = match result {
                    Ok(value) => {
                        if let Ok(item) = serde_json::from_value::<CodeAction>(value)
                        {
                            Ok(item)
                        } else {
                            Err(RpcError {
                                code: 0,
                                message: "code_action item deserialize error"
                                    .to_string(),
                            })
                        }
                    }
                    Err(e) => Err(e),
                };
                cb(result)
            },
        );
    }

    pub fn did_open_document(
        &self,
        path: &Path,
        language_id: String,
        version: i32,
        text: String,
    ) {
        let _ = self.plugin_tx.send(PluginCatalogRpc::DidOpenTextDocument {
            document: TextDocumentItem::new(
                Url::from_file_path(path).unwrap(),
                language_id,
                version,
                text,
            ),
        });
    }

    pub fn unactivated_volts(&self, volts: Vec<VoltMetadata>) -> Result<()> {
        self.catalog_notification(PluginCatalogNotification::UnactivatedVolts(volts))
    }

    pub fn plugin_server_loaded(
        &self,
        plugin: PluginServerRpcHandler,
    ) -> Result<()> {
        self.catalog_notification(PluginCatalogNotification::PluginServerLoaded(
            plugin,
        ))
    }

    pub fn update_plugin_configs(
        &self,
        configs: HashMap<String, HashMap<String, serde_json::Value>>,
    ) -> Result<()> {
        self.catalog_notification(PluginCatalogNotification::UpdatePluginConfigs(
            configs,
        ))
    }

    pub fn install_volt(&self, volt: VoltInfo) -> Result<()> {
        self.catalog_notification(PluginCatalogNotification::InstallVolt(volt))
    }

    pub fn stop_volt(&self, volt: VoltInfo) -> Result<()> {
        self.catalog_notification(PluginCatalogNotification::StopVolt(volt))
    }

    pub fn reload_volt(&self, volt: VoltMetadata) -> Result<()> {
        self.catalog_notification(PluginCatalogNotification::ReloadVolt(volt))
    }

    pub fn enable_volt(&self, volt: VoltInfo) -> Result<()> {
        self.catalog_notification(PluginCatalogNotification::EnableVolt(volt))
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum PluginNotification {
    StartLspServer {
        exec_path: String,
        language_id: String,
        options: Option<Value>,
        system_lsp: Option<bool>,
    },
    DownloadFile {
        url: String,
        path: PathBuf,
    },
    LockFile {
        path: PathBuf,
    },
    MakeFileExecutable {
        path: PathBuf,
    },
}

pub fn download_volt(volt: &VoltInfo) -> Result<VoltMetadata> {
    let url = format!(
        "https://plugins.lapce.dev/api/v1/plugins/{}/{}/{}/download",
        volt.author, volt.name, volt.version
    );

    let resp = reqwest::blocking::get(url)?;
    if !resp.status().is_success() {
        return Err(anyhow!("can't download plugin"));
    }

    // this is the s3 url
    let url = resp.text()?;

    let mut resp = reqwest::blocking::get(url)?;
    if !resp.status().is_success() {
        return Err(anyhow!("can't download plugin"));
    }

    let id = volt.id();
    let plugin_dir = Directory::plugins_directory()
        .ok_or_else(|| anyhow!("can't get plugin directory"))?
        .join(&id);
    let _ = fs::remove_dir_all(&plugin_dir);
    fs::create_dir_all(&plugin_dir)?;

    let tar = GzDecoder::new(&mut resp);
    let mut archive = Archive::new(tar);
    archive.unpack(&plugin_dir)?;

    let meta_path = plugin_dir.join("volt.toml");
    let meta = load_volt(&meta_path)?;
    Ok(meta)
}

pub fn install_volt(
    catalog_rpc: PluginCatalogRpcHandler,
    workspace: Option<PathBuf>,
    configurations: Option<HashMap<String, serde_json::Value>>,
    volt: VoltInfo,
) -> Result<()> {
    let download_volt_result = download_volt(&volt);
    if download_volt_result.is_err() {
        catalog_rpc
            .core_rpc
            .volt_installing(volt, "Could not download Plugin".to_string());
    }
    let meta = download_volt_result?;
    let local_catalog_rpc = catalog_rpc.clone();
    let local_meta = meta.clone();

    let _ = start_volt(workspace, configurations, local_catalog_rpc, local_meta);
    catalog_rpc.core_rpc.volt_installed(meta, false);
    Ok(())
}

pub fn remove_volt(
    catalog_rpc: PluginCatalogRpcHandler,
    volt: VoltMetadata,
) -> Result<()> {
    thread::spawn(move || -> Result<()> {
        let path = volt.dir.as_ref().ok_or_else(|| {
            catalog_rpc
                .core_rpc
                .volt_removing(volt.clone(), "Plugin Directory not set".to_string());
            anyhow::anyhow!("don't have dir")
        })?;
        if let Err(e) = std::fs::remove_dir_all(path) {
            eprintln!("Could not delete plugin folder: {}", e);
            catalog_rpc.core_rpc.volt_removing(
                volt.clone(),
                "Could not remove Plugin Directory".to_string(),
            );
        } else {
            catalog_rpc.core_rpc.volt_removed(volt.info(), false);
        }
        Ok(())
    });
    Ok(())
}
