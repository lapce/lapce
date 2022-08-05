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
use jsonrpc_lite::{JsonRpc, Params};
use lapce_core::{buffer::RopeText, encoding::offset_utf16_to_utf8};
use lapce_rpc::{
    plugin::PluginId,
    style::{LineStyle, SemanticStyles, Style},
    RpcError,
};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument,
        Initialized, Notification, Progress, PublishDiagnostics,
    },
    request::{
        CodeActionRequest, Completion, DocumentSymbolRequest, Formatting,
        GotoDefinition, GotoTypeDefinition, HoverRequest, Initialize,
        InlayHintRequest, References, RegisterCapability, ResolveCompletionItem,
        SemanticTokensFullRequest, WorkDoneProgressCreate, WorkspaceSymbol,
    },
    CodeActionProviderCapability, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbolResponse,
    HoverProviderCapability, InitializeResult, OneOf, ProgressParams,
    PublishDiagnosticsParams, Range, Registration, RegistrationParams,
    SemanticTokens, SemanticTokensLegend, SemanticTokensServerCapabilities,
    ServerCapabilities, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentSaveRegistrationOptions, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions,
    VersionedTextDocumentIdentifier, WorkDoneProgress, WorkDoneProgressReport,
};
use parking_lot::Mutex;
use psp_types::{Request, StartLspServer, StartLspServerParams};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xi_rope::{Rope, RopeDelta};

use super::{
    lsp::{DocumentFilter, NewLspClient},
    number_from_id, PluginCatalogRpcHandler,
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
    Initilize,
}

pub enum PluginServerRpc {
    Handler(PluginHandlerNotification),
    ServerRequest {
        id: u64,
        method: &'static str,
        params: Params,
        language_id: Option<String>,
        rh: ResponseHandler<Value, RpcError>,
    },
    ServerNotification {
        method: &'static str,
        params: Params,
        language_id: Option<String>,
    },
    HostRequest {
        id: u64,
        method: String,
        params: Params,
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
    rpc_tx: Sender<PluginServerRpc>,
    rpc_rx: Receiver<PluginServerRpc>,
    io_tx: Sender<String>,
    id: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<u64, ResponseHandler<Value, RpcError>>>>,
}

pub trait PluginServerHandler {
    fn language_supported(&mut self, language_id: Option<&str>) -> bool;
    fn method_registered(&mut self, method: &'static str) -> bool;
    fn handle_host_notification(&mut self, method: String, params: Params);
    fn handle_host_request(&mut self, id: u64, method: String, params: Params);
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
    pub fn new(io_tx: Sender<String>) -> Self {
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded();

        let rpc = Self {
            plugin_id: PluginId::next(),
            rpc_tx,
            rpc_rx,
            io_tx,
            id: Arc::new(AtomicU64::new(0)),
            server_pending: Arc::new(Mutex::new(HashMap::new())),
        };

        rpc.initilize();
        rpc
    }

    fn initilize(&self) {
        self.handle_rpc(PluginServerRpc::Handler(
            PluginHandlerNotification::Initilize,
        ));
    }

    fn send_server_request(
        &self,
        id: u64,
        method: &str,
        params: Params,
        rh: ResponseHandler<Value, RpcError>,
    ) {
        {
            let mut pending = self.server_pending.lock();
            pending.insert(id, rh);
        }
        let msg = JsonRpc::request_with_params(id as i64, method, params);
        let msg = serde_json::to_string(&msg).unwrap();
        self.send_server_rpc(msg);
    }

    fn send_server_notification(&self, method: &str, params: Params) {
        let msg = JsonRpc::notification_with_params(method, params);
        let msg = serde_json::to_string(&msg).unwrap();
        self.send_server_rpc(msg);
    }

    fn send_host_error(&self, id: u64, err: RpcError) {
        self.send_host_response::<Value>(id, Err(err));
    }

    fn send_host_success<V: Serialize>(&self, id: u64, v: V) {
        self.send_host_response(id, Ok(v));
    }

    fn send_host_response<V: Serialize>(
        &self,
        id: u64,
        result: Result<V, RpcError>,
    ) {
        let msg = match result {
            Ok(v) => JsonRpc::success(id as i64, &serde_json::to_value(v).unwrap()),
            Err(e) => JsonRpc::error(
                id as i64,
                jsonrpc_lite::Error {
                    code: e.code,
                    message: e.message,
                    data: None,
                },
            ),
        };
        let msg = serde_json::to_string(&msg).unwrap();
        self.send_server_rpc(msg);
    }

    fn send_server_rpc(&self, msg: String) {
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
        check: bool,
    ) {
        let params = Params::from(serde_json::to_value(params).unwrap());

        if check {
            let _ = self.rpc_tx.send(PluginServerRpc::ServerNotification {
                method,
                params,
                language_id,
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
        check: bool,
    ) -> Result<Value, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.server_request_common(
            method,
            params,
            language_id,
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
        check: bool,
        f: impl RpcCallback<Value, RpcError> + 'static,
    ) {
        self.server_request_common(
            method,
            params,
            language_id,
            check,
            ResponseHandler::Callback(Box::new(f)),
        );
    }

    fn server_request_common<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        check: bool,
        rh: ResponseHandler<Value, RpcError>,
    ) {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        let params = Params::from(serde_json::to_value(params).unwrap());
        if check {
            let _ = self.rpc_tx.send(PluginServerRpc::ServerRequest {
                id,
                method,
                params,
                language_id,
                rh,
            });
        } else {
            self.send_server_request(id, method, params, rh);
        }
    }

    pub fn handle_server_response(&self, id: u64, result: Result<Value, RpcError>) {
        if let Some(handler) = { self.server_pending.lock().remove(&id) } {
            handler.invoke(result);
        }
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
                    rh,
                } => {
                    if handler.language_supported(language_id.as_deref())
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
                } => {
                    if handler.language_supported(language_id.as_deref())
                        && handler.method_registered(method)
                    {
                        self.send_server_notification(method, params);
                    }
                }
                PluginServerRpc::HostRequest { id, method, params } => {
                    handler.handle_host_request(id, method, params);
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
                    handler.handle_did_save_text_document(
                        language_id,
                        path,
                        text_document,
                        text,
                    );
                }
                PluginServerRpc::DidChangeTextDocument {
                    language_id,
                    document,
                    delta,
                    text,
                    new_text,
                    change,
                } => {
                    handler.handle_did_change_text_document(
                        language_id,
                        document,
                        delta,
                        text,
                        new_text,
                        change,
                    );
                }
                PluginServerRpc::FormatSemanticTokens { tokens, text, f } => {
                    handler.format_semantic_tokens(tokens, text, f);
                }
                PluginServerRpc::Handler(notification) => {
                    handler.handle_handler_notification(notification)
                }
            }
        }
    }
}

pub fn handle_plugin_server_message(
    server_rpc: &PluginServerRpcHandler,
    message: &str,
) {
    match JsonRpc::parse(message) {
        Ok(value @ JsonRpc::Request(_)) => {
            let id = number_from_id(&value.get_id().unwrap());
            let rpc = PluginServerRpc::HostRequest {
                id,
                method: value.get_method().unwrap().to_string(),
                params: value.get_params().unwrap(),
            };
            server_rpc.handle_rpc(rpc);
        }
        Ok(value @ JsonRpc::Notification(_)) => {
            let rpc = PluginServerRpc::HostNotification {
                method: value.get_method().unwrap().to_string(),
                params: value.get_params().unwrap(),
            };
            server_rpc.handle_rpc(rpc);
        }
        Ok(value @ JsonRpc::Success(_)) => {
            let id = number_from_id(&value.get_id().unwrap());
            let result = value.get_result().unwrap().clone();
            server_rpc.handle_server_response(id, Ok(result));
        }
        Ok(value @ JsonRpc::Error(_)) => {
            let id = number_from_id(&value.get_id().unwrap());
            let error = value.get_error().unwrap();
            server_rpc.handle_server_response(
                id,
                Err(RpcError {
                    code: error.code,
                    message: error.message.clone(),
                }),
            );
        }
        Err(err) => {
            eprintln!("parse error {err}");
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
}

pub struct PluginHostHandler {
    pwd: Option<PathBuf>,
    pub(crate) workspace: Option<PathBuf>,
    lanaguage_id: Option<String>,
    catalog_rpc: PluginCatalogRpcHandler,
    pub server_rpc: PluginServerRpcHandler,
    pub server_capabilities: ServerCapabilities,
    server_registrations: ServerRegistrations,
}

impl PluginHostHandler {
    pub fn new(
        workspace: Option<PathBuf>,
        pwd: Option<PathBuf>,
        lanaguage_id: Option<String>,
        server_rpc: PluginServerRpcHandler,
        catalog_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        Self {
            pwd,
            workspace,
            lanaguage_id,
            catalog_rpc,
            server_rpc,
            server_capabilities: ServerCapabilities::default(),
            server_registrations: ServerRegistrations::default(),
        }
    }

    pub fn language_supported(&mut self, language_id: Option<&str>) -> bool {
        match language_id {
            Some(language_id) => match self.lanaguage_id.as_ref() {
                Some(l) => l.as_str() == language_id,
                None => true,
            },
            None => true,
        }
    }

    pub fn method_registered(&mut self, method: &'static str) -> bool {
        match method {
            Initialize::METHOD => true,
            Initialized::METHOD => true,
            Completion::METHOD => {
                eprintln!("check completion registered");
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
            _ => false,
        }
    }

    fn check_save_capability(&self, language_id: &str, path: &Path) -> (bool, bool) {
        if self.lanaguage_id.is_none()
            || self.lanaguage_id.as_deref() == Some(language_id)
        {
            let (should_send, include_text) = self
                .server_capabilities
                .text_document_sync
                .as_ref()
                .and_then(|sync| match sync {
                    TextDocumentSyncCapability::Kind(_) => None,
                    TextDocumentSyncCapability::Options(options) => Some(options),
                })
                .and_then(|o| o.save.as_ref())
                .map(|o| match o {
                    TextDocumentSyncSaveOptions::Supported(is_supported) => {
                        (*is_supported, true)
                    }
                    TextDocumentSyncSaveOptions::SaveOptions(options) => {
                        (true, options.include_text.unwrap_or(false))
                    }
                })
                .unwrap_or((false, false));
            return (should_send, include_text);
        }

        if let Some(options) = self.server_registrations.save.as_ref() {
            for filter in options.filters.iter() {
                if (filter.language_id.is_none()
                    || filter.language_id.as_deref() == Some(language_id))
                    && (filter.pattern.is_none()
                        || filter.pattern.as_ref().unwrap().is_match(path))
                {
                    return (true, options.include_text);
                }
            }
        }

        (false, false)
    }

    fn register_capabilities(&mut self, registrations: Vec<Registration>) {
        for registration in registrations {
            let _ = self.register_capability(registration);
        }
    }

    fn register_capability(&mut self, registration: Registration) -> Result<()> {
        match registration.method.as_str() {
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
                                .map(DocumentFilter::from_lsp_filter_loose)
                                .collect()
                        })
                        .unwrap_or_default(),
                });
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
        id: u64,
        method: String,
        params: Params,
    ) -> Result<()> {
        match method.as_str() {
            WorkDoneProgressCreate::METHOD => {
                self.server_rpc.send_host_success(id, Value::Null);
            }
            RegisterCapability::METHOD => {
                let params: RegistrationParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                self.register_capabilities(params.registrations);
            }
            _ => {
                self.server_rpc.send_host_error(
                    id,
                    RpcError {
                        code: 0,
                        message: "request not handled".to_string(),
                    },
                );
            }
        }
        Ok(())
    }

    pub fn handle_notification(
        &mut self,
        method: String,
        params: Params,
    ) -> Result<()> {
        match method.as_str() {
            StartLspServer::METHOD => {
                eprintln!("start lsp server");
                let params: StartLspServerParams =
                    serde_json::from_value(serde_json::to_value(params)?)?;
                let workspace = self.workspace.clone();
                let pwd = self.pwd.clone();
                let catalog_rpc = self.catalog_rpc.clone();
                thread::spawn(move || {
                    match NewLspClient::start(
                        catalog_rpc,
                        params.language_id,
                        workspace,
                        pwd,
                        params.exec_path,
                        Vec::new(),
                        params.options,
                    ) {
                        Ok(_) => eprintln!("lsp started"),
                        Err(e) => eprintln!("lsp start error {e}"),
                    }
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
            _ => {
                eprintln!("host notificaton {method} not handled");
            }
        }
        Ok(())
    }

    pub fn handle_did_save_text_document(
        &self,
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: Rope,
    ) {
        let (should_send, include_text) =
            self.check_save_capability(language_id.as_str(), &path);
        if !should_send {
            eprintln!("did save not sent for {path:?} {language_id}");
            return;
        }
        eprintln!(
            "did save sent for {path:?} {language_id} inclue_text: {include_text}"
        );
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
            Some(language_id),
            false,
        );
    }

    pub fn handle_did_change_text_document(
        &mut self,
        lanaguage_id: String,
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
            Some(lanaguage_id),
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
        let start = if let Some(start) = text.offset_to_position(start) {
            start
        } else {
            log::error!("Failed to convert start offset to Position in document content change insert");
            return None;
        };

        let end = if let Some(end) = text.offset_to_position(end) {
            end
        } else {
            log::error!("Failed to convert end offset to Position in document content change insert");
            return None;
        };

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
        let end_position = if let Some(end) = text.offset_to_position(end) {
            end
        } else {
            log::error!("Failed to convert end offset to Position in document content change delete");
            return None;
        };

        let start = if let Some(start) = text.offset_to_position(start) {
            start
        } else {
            log::error!("Failed to convert start offset to Position in document content change delete");
            return None;
        };

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
        if let Some(utf8_delta_start) =
            offset_utf16_to_utf8(sub_text, semantic_token.delta_start as usize)
        {
            start += utf8_delta_start;
        } else {
            // Bad semantic token offsets
            log::error!("Bad semantic token starty {semantic_token:?}");
            break;
        };

        let sub_text = text.char_indices_iter(start..);
        let end = if let Some(utf8_length) =
            offset_utf16_to_utf8(sub_text, semantic_token.length as usize)
        {
            start + utf8_length
        } else {
            log::error!("Bad semantic token end {semantic_token:?}");
            break;
        };

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
