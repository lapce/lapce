use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use dyn_clone::DynClone;
use globset::Glob;
use jsonrpc_lite::{Id, JsonRpc, Params};
use lapce_core::{buffer::rope_text::RopeText, encoding::offset_utf16_to_utf8};
use lapce_rpc::{
    plugin::{PluginId, VoltID},
    style::{LineStyle, Style},
    RpcError,
};
use lapce_xi_rope::{Rope, RopeDelta};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidChangeWatchedFiles, DidOpenTextDocument,
        DidSaveTextDocument, Initialized, LogMessage, Notification, Progress,
        PublishDiagnostics, ShowMessage,
    },
    request::{
        CodeActionRequest, CodeActionResolveRequest, Completion,
        DocumentSymbolRequest, Formatting, GotoDefinition, GotoTypeDefinition,
        HoverRequest, Initialize, InlayHintRequest, PrepareRenameRequest,
        References, RegisterCapability, Rename, ResolveCompletionItem,
        SelectionRangeRequest, SemanticTokensFullRequest, SignatureHelpRequest,
        WorkDoneProgressCreate, WorkspaceSymbol,
    },
    CodeActionProviderCapability, DidChangeTextDocumentParams,
    DidChangeWatchedFilesRegistrationOptions, DidSaveTextDocumentParams,
    DocumentSelector, HoverProviderCapability, LogMessageParams, OneOf,
    ProgressParams, PublishDiagnosticsParams, Range, Registration,
    RegistrationParams, SemanticTokens, SemanticTokensLegend,
    SemanticTokensServerCapabilities, ServerCapabilities, ShowMessageParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentSaveRegistrationOptions, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncSaveOptions,
    VersionedTextDocumentIdentifier, WatchKind,
};
use parking_lot::Mutex;
use psp_types::{
    ExecuteProcess, ExecuteProcessParams, ExecuteProcessResult, Request,
    StartLspServer, StartLspServerParams,
};
use serde::Serialize;
use serde_json::Value;

use super::{
    lsp::{DocumentFilter, LspClient},
    PluginCatalogRpcHandler,
};

pub enum ResponseHandler<Resp, Error> {
    Chan(Sender<Result<Resp, Error>>),
    Callback(Box<dyn RpcCallback<Resp, Error>>),
}

impl<Resp, Error> ResponseHandler<Resp, Error> {
    fn invoke(self, result: Result<Resp, Error>) {
        match self {
            ResponseHandler::Chan(tx) => {
                let _ = tx.send(result);
            }
            ResponseHandler::Callback(f) => f.call(result),
        }
    }
}

pub trait ClonableCallback:
    FnOnce(PluginId, Result<Value, RpcError>) + Send + DynClone
{
}

impl<F: Send + FnOnce(PluginId, Result<Value, RpcError>) + DynClone> ClonableCallback
    for F
{
}

pub trait RpcCallback<Resp, Error>: Send {
    fn call(self: Box<Self>, result: Result<Resp, Error>);
}

impl<Resp, Error, F: Send + FnOnce(Result<Resp, Error>)> RpcCallback<Resp, Error>
    for F
{
    fn call(self: Box<F>, result: Result<Resp, Error>) {
        (*self)(result)
    }
}

pub enum PluginHandlerNotification {
    Initialize,
    Shutdown,
}

pub enum PluginServerRpc {
    Shutdown,
    Handler(PluginHandlerNotification),
    ServerRequest {
        id: Id,
        method: &'static str,
        params: Params,
        language_id: Option<String>,
        path: Option<PathBuf>,
        rh: ResponseHandler<Value, RpcError>,
    },
    ServerNotification {
        method: &'static str,
        params: Params,
        language_id: Option<String>,
        path: Option<PathBuf>,
    },
    HostRequest {
        id: Id,
        method: String,
        params: Params,
        chan: Sender<Result<Value, RpcError>>,
    },
    HostNotification {
        method: String,
        params: Params,
    },
    DidSaveTextDocument {
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: Rope,
    },
    DidChangeTextDocument {
        language_id: String,
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
        change: Arc<
            Mutex<(
                Option<TextDocumentContentChangeEvent>,
                Option<TextDocumentContentChangeEvent>,
            )>,
        >,
    },
    FormatSemanticTokens {
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    },
}

#[derive(Clone)]
pub struct PluginServerRpcHandler {
    pub plugin_id: PluginId,
    pub volt_id: VoltID,
    rpc_tx: Sender<PluginServerRpc>,
    rpc_rx: Receiver<PluginServerRpc>,
    io_tx: Sender<JsonRpc>,
    id: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<Id, ResponseHandler<Value, RpcError>>>>,
}

pub trait PluginServerHandler {
    fn document_supported(
        &self,
        language_id: Option<&str>,
        path: Option<&Path>,
    ) -> bool;
    fn method_registered(&mut self, method: &'static str) -> bool;
    fn handle_host_notification(&mut self, method: String, params: Params);
    fn handle_host_request(
        &mut self,
        id: Id,
        method: String,
        params: Params,
        chan: Sender<Result<Value, RpcError>>,
    );
    fn handle_handler_notification(
        &mut self,
        notification: PluginHandlerNotification,
    );
    fn handle_did_save_text_document(
        &self,
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: Rope,
    );
    fn handle_did_change_text_document(
        &mut self,
        language_id: String,
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
        change: Arc<
            Mutex<(
                Option<TextDocumentContentChangeEvent>,
                Option<TextDocumentContentChangeEvent>,
            )>,
        >,
    );
    fn format_semantic_tokens(
        &self,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    );
}

impl PluginServerRpcHandler {
    pub fn new(volt_id: VoltID, io_tx: Sender<JsonRpc>) -> Self {
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded();

        let rpc = Self {
            volt_id,
            plugin_id: PluginId::next(),
            rpc_tx,
            rpc_rx,
            io_tx,
            id: Arc::new(AtomicU64::new(0)),
            server_pending: Arc::new(Mutex::new(HashMap::new())),
        };

        rpc.initialize();
        rpc
    }

    fn initialize(&self) {
        self.handle_rpc(PluginServerRpc::Handler(
            PluginHandlerNotification::Initialize,
        ));
    }

    fn send_server_request(
        &self,
        id: Id,
        method: &str,
        params: Params,
        rh: ResponseHandler<Value, RpcError>,
    ) {
        {
            let mut pending = self.server_pending.lock();
            pending.insert(id.clone(), rh);
        }
        let msg = JsonRpc::request_with_params(id, method, params);
        self.send_server_rpc(msg);
    }

    fn send_server_notification(&self, method: &str, params: Params) {
        let msg = JsonRpc::notification_with_params(method, params);
        self.send_server_rpc(msg);
    }

    fn send_server_rpc(&self, msg: JsonRpc) {
        let _ = self.io_tx.send(msg);
    }

    pub fn handle_rpc(&self, rpc: PluginServerRpc) {
        let _ = self.rpc_tx.send(rpc);
    }

    pub fn server_notification<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        path: Option<PathBuf>,
        check: bool,
    ) {
        let params = Params::from(serde_json::to_value(params).unwrap());

        if check {
            let _ = self.rpc_tx.send(PluginServerRpc::ServerNotification {
                method,
                params,
                language_id,
                path,
            });
        } else {
            self.send_server_notification(method, params);
        }
    }

    /// Make a request to plugin/language server and get the response
    /// when check is true, the request will be in the handler mainloop to
    /// do checks like if the server has the capability of the request
    /// when check is false, the request will be sent out straight away
    pub fn server_request<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        path: Option<PathBuf>,
        check: bool,
    ) -> Result<Value, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.server_request_common(
            method,
            params,
            language_id,
            path,
            check,
            ResponseHandler::Chan(tx),
        );
        rx.recv().unwrap_or_else(|_| {
            Err(RpcError {
                code: 0,
                message: "io error".to_string(),
            })
        })
    }

    pub fn server_request_async<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        path: Option<PathBuf>,
        check: bool,
        f: impl RpcCallback<Value, RpcError> + 'static,
    ) {
        self.server_request_common(
            method,
            params,
            language_id,
            path,
            check,
            ResponseHandler::Callback(Box::new(f)),
        );
    }

    fn server_request_common<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        path: Option<PathBuf>,
        check: bool,
        rh: ResponseHandler<Value, RpcError>,
    ) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        let params = Params::from(serde_json::to_value(params).unwrap());
        if check {
            let _ = self.rpc_tx.send(PluginServerRpc::ServerRequest {
                id: Id::Num(id as i64),
                method,
                params,
                language_id,
                path,
                rh,
            });
        } else {
            self.send_server_request(Id::Num(id as i64), method, params, rh);
        }
    }

    pub fn handle_server_response(&self, id: Id, result: Result<Value, RpcError>) {
        if let Some(handler) = { self.server_pending.lock().remove(&id) } {
            handler.invoke(result);
        }
    }

    pub fn shutdown(&self) {
        self.handle_rpc(PluginServerRpc::Handler(
            PluginHandlerNotification::Shutdown,
        ));
        self.handle_rpc(PluginServerRpc::Shutdown);
    }

    pub fn mainloop<H>(&self, handler: &mut H)
    where
        H: PluginServerHandler,
    {
        for msg in &self.rpc_rx {
            match msg {
                PluginServerRpc::ServerRequest {
                    id,
                    method,
                    params,
                    language_id,
                    path,
                    rh,
                } => {
                    if handler
                        .document_supported(language_id.as_deref(), path.as_deref())
                        && handler.method_registered(method)
                    {
                        self.send_server_request(id, method, params, rh);
                    } else {
                        rh.invoke(Err(RpcError {
                            code: 0,
                            message: "server not capable".to_string(),
                        }));
                    }
                }
                PluginServerRpc::ServerNotification {
                    method,
                    params,
                    language_id,
                    path,
                } => {
                    if handler
                        .document_supported(language_id.as_deref(), path.as_deref())
                        && handler.method_registered(method)
                    {
                        self.send_server_notification(method, params);
                    }
                }
                PluginServerRpc::HostRequest {
                    id,
                    method,
                    params,
                    chan,
                } => {
                    handler.handle_host_request(id, method, params, chan);
                }
                PluginServerRpc::HostNotification { method, params } => {
                    handler.handle_host_notification(method, params);
                }
                PluginServerRpc::DidSaveTextDocument {
                    language_id,
                    path,
                    text_document,
                    text,
                } => {
                    if handler
                        .document_supported(Some(language_id.as_str()), Some(&path))
                    {
                        handler.handle_did_save_text_document(
                            language_id,
                            path,
                            text_document,
                            text,
                        );
                    }
                }
                PluginServerRpc::DidChangeTextDocument {
                    language_id,
                    document,
                    delta,
                    text,
                    new_text,
                    change,
                } => {
                    if handler.document_supported(Some(language_id.as_str()), None) {
                        handler.handle_did_change_text_document(
                            language_id,
                            document,
                            delta,
                            text,
                            new_text,
                            change,
                        );
                    }
                }
                PluginServerRpc::FormatSemanticTokens { tokens, text, f } => {
                    handler.format_semantic_tokens(tokens, text, f);
                }
                PluginServerRpc::Handler(notification) => {
                    handler.handle_handler_notification(notification)
                }
                PluginServerRpc::Shutdown => {
                    return;
                }
            }
        }
    }
}

pub fn handle_plugin_server_message(
    server_rpc: &PluginServerRpcHandler,
    message: &str,
) -> Option<JsonRpc> {
    match JsonRpc::parse(message) {
        Ok(value @ JsonRpc::Request(_)) => {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let id = value.get_id().unwrap();
            let rpc = PluginServerRpc::HostRequest {
                id: id.clone(),
                method: value.get_method().unwrap().to_string(),
                params: value.get_params().unwrap(),
                chan: tx,
            };
            server_rpc.handle_rpc(rpc);
            let result = rx.recv().unwrap();
            let resp = match result {
                Ok(v) => JsonRpc::success(id, &v),
                Err(e) => JsonRpc::error(
                    id,
                    jsonrpc_lite::Error {
                        code: e.code,
                        message: e.message,
                        data: None,
                    },
                ),
            };
            Some(resp)
        }
        Ok(value @ JsonRpc::Notification(_)) => {
            let rpc = PluginServerRpc::HostNotification {
                method: value.get_method().unwrap().to_string(),
                params: value.get_params().unwrap(),
            };
            server_rpc.handle_rpc(rpc);
            None
        }
        Ok(value @ JsonRpc::Success(_)) => {
            let result = value.get_result().unwrap().clone();
            server_rpc.handle_server_response(value.get_id().unwrap(), Ok(result));
            None
        }
        Ok(value @ JsonRpc::Error(_)) => {
            let error = value.get_error().unwrap();
            server_rpc.handle_server_response(
                value.get_id().unwrap(),
                Err(RpcError {
                    code: error.code,
                    message: error.message.clone(),
                }),
            );
            None
        }
        Err(err) => {
            eprintln!("parse error {err} message {message}");
            None
        }
    }
}

struct SaveRegistration {
    include_text: bool,
    filters: Vec<DocumentFilter>,
}

#[derive(Default)]
struct ServerRegistrations {
    save: Option<SaveRegistration>,
    did_change: Vec<(WatchKind, DocumentFilter)>,
}

pub struct PluginHostHandler {
    volt_id: VoltID,
    volt_display_name: String,
    pwd: Option<PathBuf>,
    pub(crate) workspace: Option<PathBuf>,
    document_selector: Vec<DocumentFilter>,
    catalog_rpc: PluginCatalogRpcHandler,
    pub server_rpc: PluginServerRpcHandler,
    pub server_capabilities: ServerCapabilities,
    server_registrations: ServerRegistrations,
}

impl PluginHostHandler {
    pub fn new(
        workspace: Option<PathBuf>,
        pwd: Option<PathBuf>,
        volt_id: VoltID,
        volt_display_name: String,
        document_selector: DocumentSelector,
        server_rpc: PluginServerRpcHandler,
        catalog_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        let document_selector = document_selector
            .iter()
            .flat_map(DocumentFilter::from_lsp_filter_loose)
            .collect();
        Self {
            pwd,
            workspace,
            volt_id,
            volt_display_name,
            document_selector,
            catalog_rpc,
            server_rpc,
            server_capabilities: ServerCapabilities::default(),
            server_registrations: ServerRegistrations::default(),
        }
    }

    pub fn document_supported(
        &self,
        language_id: Option<&str>,
        path: Option<&Path>,
    ) -> bool {
        check_document_filters(&self.document_selector, language_id, path)
    }

    pub fn method_registered(&mut self, method: &'static str) -> bool {
        match method {
            Initialize::METHOD => true,
            Initialized::METHOD => true,
            Completion::METHOD => {
                self.server_capabilities.completion_provider.is_some()
            }
            ResolveCompletionItem::METHOD => self
                .server_capabilities
                .completion_provider
                .as_ref()
                .and_then(|c| c.resolve_provider)
                .unwrap_or(false),
            DidOpenTextDocument::METHOD => {
                match &self.server_capabilities.text_document_sync {
                    Some(TextDocumentSyncCapability::Kind(kind)) => {
                        kind != &TextDocumentSyncKind::NONE
                    }
                    Some(TextDocumentSyncCapability::Options(options)) => options
                        .open_close
                        .or_else(|| {
                            options
                                .change
                                .map(|kind| kind != TextDocumentSyncKind::NONE)
                        })
                        .unwrap_or(false),
                    None => false,
                }
            }
            DidChangeTextDocument::METHOD => {
                match &self.server_capabilities.text_document_sync {
                    Some(TextDocumentSyncCapability::Kind(kind)) => {
                        kind != &TextDocumentSyncKind::NONE
                    }
                    Some(TextDocumentSyncCapability::Options(options)) => options
                        .change
                        .map(|kind| kind != TextDocumentSyncKind::NONE)
                        .unwrap_or(false),
                    None => false,
                }
            }
            SignatureHelpRequest::METHOD => {
                self.server_capabilities.signature_help_provider.is_some()
            }
            HoverRequest::METHOD => self
                .server_capabilities
                .hover_provider
                .as_ref()
                .map(|c| match c {
                    HoverProviderCapability::Simple(is_capable) => *is_capable,
                    HoverProviderCapability::Options(_) => true,
                })
                .unwrap_or(false),
            GotoDefinition::METHOD => self
                .server_capabilities
                .definition_provider
                .as_ref()
                .map(|d| match d {
                    OneOf::Left(is_capable) => *is_capable,
                    OneOf::Right(_) => true,
                })
                .unwrap_or(false),
            GotoTypeDefinition::METHOD => {
                self.server_capabilities.type_definition_provider.is_some()
            }
            References::METHOD => self
                .server_capabilities
                .references_provider
                .as_ref()
                .map(|r| match r {
                    OneOf::Left(is_capable) => *is_capable,
                    OneOf::Right(_) => true,
                })
                .unwrap_or(false),
            CodeActionRequest::METHOD => self
                .server_capabilities
                .code_action_provider
                .as_ref()
                .map(|a| match a {
                    CodeActionProviderCapability::Simple(is_capable) => *is_capable,
                    CodeActionProviderCapability::Options(_) => true,
                })
                .unwrap_or(false),
            Formatting::METHOD => self
                .server_capabilities
                .document_formatting_provider
                .as_ref()
                .map(|f| match f {
                    OneOf::Left(is_capable) => *is_capable,
                    OneOf::Right(_) => true,
                })
                .unwrap_or(false),
            SemanticTokensFullRequest::METHOD => {
                self.server_capabilities.semantic_tokens_provider.is_some()
            }
            InlayHintRequest::METHOD => {
                self.server_capabilities.inlay_hint_provider.is_some()
            }
            DocumentSymbolRequest::METHOD => {
                self.server_capabilities.document_symbol_provider.is_some()
            }
            WorkspaceSymbol::METHOD => {
                self.server_capabilities.workspace_symbol_provider.is_some()
            }
            PrepareRenameRequest::METHOD => {
                self.server_capabilities.rename_provider.is_some()
            }
            Rename::METHOD => self.server_capabilities.rename_provider.is_some(),
            SelectionRangeRequest::METHOD => {
                self.server_capabilities.selection_range_provider.is_some()
            }
            CodeActionResolveRequest::METHOD => {
                self.server_capabilities.code_action_provider.is_some()
            }
            _ => false,
        }
    }

    /// Check if the server is capable of handling the given text document sync event specified by `tds_kind`.
    /// See https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_synchronization
    fn check_server_capability_for_text_document_sync(&self, tds_kind: Tds) -> bool {
        if let Some(server_tds_capability) =
            self.server_capabilities.text_document_sync.as_ref()
        {
            match server_tds_capability {
                TextDocumentSyncCapability::Kind(k) => match *k {
                    TextDocumentSyncKind::NONE => return false,
                    _ => return true,
                },
                TextDocumentSyncCapability::Options(o) => match tds_kind {
                    Tds::Change => {
                        if let Some(o) = o.change {
                            match o {
                                TextDocumentSyncKind::NONE => return false,
                                _ => return true,
                            }
                        }
                    }
                    Tds::Save => {
                        if let Some(o) = o.save.as_ref() {
                            match o {
                                TextDocumentSyncSaveOptions::Supported(s) => {
                                    return *s
                                }
                                TextDocumentSyncSaveOptions::SaveOptions(_) => {
                                    return true
                                }
                            }
                        }
                    }
                },
            }
        }

        false
    }

    /// Check if the server is interested in the given `language_id` or `path` given the text document sync event in `tds`.
    /// Sometimes an LSP server will dynamically register for files it is interested in for a given text document sync events.
    /// This will check that.
    fn check_server_file_filter(
        &self,
        language_id: Option<&str>,
        path: Option<&Path>,
        tds: Tds,
    ) -> bool {
        match tds {
            Tds::Change => {
                return self.server_registrations.did_change.iter().any(
                    |(kind, doc)| {
                        kind.contains(WatchKind::Change)
                            && doc.matches_any(language_id, path)
                    },
                )
            }
            Tds::Save => {
                if let Some(options) = self.server_registrations.save.as_ref() {
                    return check_document_filters(
                        &options.filters,
                        language_id,
                        path,
                    );
                }
            }
        }

        false
    }

    fn register_capabilities(&mut self, registrations: Vec<Registration>) {
        for registration in registrations {
            let _ = self.register_capability(registration);
        }
    }

    fn register_capability(&mut self, registration: Registration) -> Result<()> {
        match registration.method.as_str() {
            // https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_didSave
            // Tells us that the LSP is interested if we are saving the files that match the pattern
            DidSaveTextDocument::METHOD => {
                let options = registration
                    .register_options
                    .ok_or_else(|| anyhow!("don't have options"))?;
                let options: TextDocumentSaveRegistrationOptions =
                    serde_json::from_value(options)?;
                self.server_registrations.save = Some(SaveRegistration {
                    include_text: options.include_text.unwrap_or(false),
                    filters: options
                        .text_document_registration_options
                        .document_selector
                        .map(|s| {
                            s.iter()
                                .flat_map(DocumentFilter::from_lsp_filter_loose)
                                .collect()
                        })
                        .unwrap_or_default(),
                });
            }
            // https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#workspace_didChangeWatchedFiles
            // Tells us that the LSP is interested in any changes to the files/folders that match
            DidChangeWatchedFiles::METHOD => {
                let options = registration
                    .register_options
                    .ok_or_else(|| anyhow!("don't have options"))?;
                let options: DidChangeWatchedFilesRegistrationOptions =
                    serde_json::from_value(options)?;
                self.server_registrations.did_change = options
                    .watchers
                    .iter()
                    .filter_map(|fsw| {
                        Glob::new(fsw.glob_pattern.as_str()).ok().map(|ok| {
                            (
                                fsw.kind.unwrap_or(WatchKind::all()),
                                DocumentFilter::Pattern(ok.compile_matcher()),
                            )
                        })
                    })
                    .collect();
            }
            _ => {
                eprintln!(
                    "don't handle register capability for {}",
                    registration.method
                );
            }
        }
        Ok(())
    }

    pub fn handle_request(
        &mut self,
        _id: Id,
        method: String,
        params: Params,
        chan: Sender<Result<Value, RpcError>>,
    ) {
        let result = self.process_request(method, params);
        let _ = chan.send(result.map_err(|e| RpcError {
            code: 0,
            message: e.to_string(),
        }));
    }

    pub fn process_request(
        &mut self,
        method: String,
        params: Params,
    ) -> Result<Value> {
        match method.as_str() {
            WorkDoneProgressCreate::METHOD => Ok(Value::Null),
            RegisterCapability::METHOD => {
                let params: RegistrationParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                self.register_capabilities(params.registrations);
                Ok(Value::Null)
            }
            ExecuteProcess::METHOD => {
                let params: ExecuteProcessParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                let output = std::process::Command::new(params.program)
                    .args(params.args)
                    .output()?;
                Ok(serde_json::to_value(ExecuteProcessResult {
                    success: output.status.success(),
                })?)
            }
            _ => Err(anyhow!("request not supported")),
        }
    }

    pub fn handle_notification(
        &mut self,
        method: String,
        params: Params,
    ) -> Result<()> {
        match method.as_str() {
            StartLspServer::METHOD => {
                let params: StartLspServerParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                let workspace = self.workspace.clone();
                let pwd = self.pwd.clone();
                let catalog_rpc = self.catalog_rpc.clone();
                let volt_id = self.volt_id.clone();
                let volt_display_name = self.volt_display_name.clone();
                thread::spawn(move || {
                    let _ = LspClient::start(
                        catalog_rpc,
                        params.document_selector,
                        workspace,
                        volt_id,
                        volt_display_name,
                        pwd,
                        params.server_uri,
                        params.server_args,
                        params.options,
                    );
                });
            }
            PublishDiagnostics::METHOD => {
                let diagnostics: PublishDiagnosticsParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                self.catalog_rpc.core_rpc.publish_diagnostics(diagnostics);
            }
            Progress::METHOD => {
                let progress: ProgressParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                self.catalog_rpc.core_rpc.work_done_progress(progress);
            }
            ShowMessage::METHOD => {
                let message: ShowMessageParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                let title = format!("Plugin: {}", self.volt_display_name);
                self.catalog_rpc.core_rpc.show_message(title, message);
            }
            LogMessage::METHOD => {
                let message: LogMessageParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                self.catalog_rpc.core_rpc.log_message(message);
            }
            _ => {
                eprintln!("host notificaton {method} not handled");
            }
        }
        Ok(())
    }

    pub fn handle_did_save_text_document(
        &self,
        language_id: Option<String>,
        path: Option<PathBuf>,
        text_document: TextDocumentIdentifier,
        text: Rope,
    ) {
        if !self.check_server_capability_for_text_document_sync(Tds::Save)
            || !self.check_server_file_filter(
                language_id.as_deref(),
                path.as_deref(),
                Tds::Save,
            )
        {
            return;
        }

        let include_text = self.server_registrations.save.iter().any(|d| {
            check_document_filters(
                &d.filters,
                language_id.as_deref(),
                path.as_deref(),
            ) && d.include_text
        });

        let params = DidSaveTextDocumentParams {
            text_document,
            text: if include_text {
                Some(text.to_string())
            } else {
                None
            },
        };
        self.server_rpc.server_notification(
            DidSaveTextDocument::METHOD,
            params,
            language_id,
            path,
            false,
        );
    }

    pub fn handle_did_change_text_document(
        &mut self,
        language_id: Option<String>,
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
        change: Arc<
            Mutex<(
                Option<TextDocumentContentChangeEvent>,
                Option<TextDocumentContentChangeEvent>,
            )>,
        >,
    ) {
        let path = document.uri.to_file_path().ok();

        if !self.check_server_capability_for_text_document_sync(Tds::Change)
            || !self.check_server_file_filter(
                language_id.as_deref(),
                path.as_deref(),
                Tds::Change,
            )
        {
            return;
        }

        let kind = match &self.server_capabilities.text_document_sync {
            Some(TextDocumentSyncCapability::Kind(kind)) => *kind,
            Some(TextDocumentSyncCapability::Options(options)) => {
                options.change.unwrap_or(TextDocumentSyncKind::NONE)
            }
            None => TextDocumentSyncKind::NONE,
        };

        let mut existing = change.lock();
        let change = match kind {
            TextDocumentSyncKind::FULL => {
                if let Some(c) = existing.0.as_ref() {
                    c.clone()
                } else {
                    let change = TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: new_text.to_string(),
                    };
                    existing.0 = Some(change.clone());
                    change
                }
            }
            TextDocumentSyncKind::INCREMENTAL => {
                if let Some(c) = existing.1.as_ref() {
                    c.clone()
                } else {
                    let change = get_document_content_change(&text, &delta)
                        .unwrap_or_else(|| TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: new_text.to_string(),
                        });
                    existing.1 = Some(change.clone());
                    change
                }
            }
            TextDocumentSyncKind::NONE => return,
            _ => return,
        };

        let params = DidChangeTextDocumentParams {
            text_document: document,
            content_changes: vec![change],
        };

        self.server_rpc.server_notification(
            DidChangeTextDocument::METHOD,
            params,
            language_id,
            path,
            false,
        );
    }

    pub fn format_semantic_tokens(
        &self,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    ) {
        let result = format_semantic_styles(
            &text,
            self.server_capabilities.semantic_tokens_provider.as_ref(),
            &tokens,
        )
        .ok_or_else(|| RpcError {
            code: 0,
            message: "can't get styles".to_string(),
        });
        f.call(result);
    }
}

fn get_document_content_change(
    text: &Rope,
    delta: &RopeDelta,
) -> Option<TextDocumentContentChangeEvent> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    let text = RopeText::new(text);

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let (start, end) = interval.start_end();
        let start = text.offset_to_position(start);

        let end = text.offset_to_position(end);

        let text = String::from(node);
        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text,
        };

        return Some(text_document_content_change_event);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let end_position = text.offset_to_position(end);

        let start = text.offset_to_position(start);

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start,
                end: end_position,
            }),
            range_length: None,
            text: String::new(),
        };

        return Some(text_document_content_change_event);
    }

    None
}

fn format_semantic_styles(
    text: &Rope,
    semantic_tokens_provider: Option<&SemanticTokensServerCapabilities>,
    tokens: &SemanticTokens,
) -> Option<Vec<LineStyle>> {
    let semantic_tokens_provider = semantic_tokens_provider?;
    let semantic_legends = semantic_tokens_legend(semantic_tokens_provider);

    let text = RopeText::new(text);
    let mut highlights = Vec::new();
    let mut line = 0;
    let mut start = 0;
    let mut last_start = 0;
    for semantic_token in &tokens.data {
        if semantic_token.delta_line > 0 {
            line += semantic_token.delta_line as usize;
            start = text.offset_of_line(line);
        }

        let sub_text = text.char_indices_iter(start..);
        start += offset_utf16_to_utf8(sub_text, semantic_token.delta_start as usize);

        let sub_text = text.char_indices_iter(start..);
        let end =
            start + offset_utf16_to_utf8(sub_text, semantic_token.length as usize);

        let kind = semantic_legends.token_types[semantic_token.token_type as usize]
            .as_str()
            .to_string();
        if start < last_start {
            continue;
        }
        last_start = start;
        highlights.push(LineStyle {
            start,
            end,
            style: Style {
                fg_color: Some(kind),
            },
        });
    }

    Some(highlights)
}

fn semantic_tokens_legend(
    semantic_tokens_provider: &SemanticTokensServerCapabilities,
) -> &SemanticTokensLegend {
    match semantic_tokens_provider {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => {
            &options.legend
        }
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
            options,
        ) => &options.semantic_tokens_options.legend,
    }
}

/// The kind of text document sync.
/// Correspond to each key of this object:
/// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocumentSyncOptions
enum Tds {
    Change,
    Save,
    // TODO: Implement the following
    // OpenClose,
    // WillSave,
    // WillSaveWaitUntil,
}

// TODO: Add support for scheme
pub fn check_document_filters(
    document_filters: &[DocumentFilter],
    language_id: Option<&str>,
    path: Option<&Path>,
) -> bool {
    document_filters
        .iter()
        .any(|d| d.matches_any(language_id, path))
}
