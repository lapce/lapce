pub mod catalog;
pub mod lsp;
pub mod psp;
pub mod wasi;

use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use hotwatch::Hotwatch;
use jsonrpc_lite::{Id, JsonRpc};
use lapce_rpc::counter::Counter;
use lapce_rpc::plugin::{PluginDescription, PluginId};
use lapce_rpc::proxy::ProxyRpcHandler;
use lapce_rpc::style::{LineStyle, SemanticStyles};
use lapce_rpc::{RequestId, RpcError, RpcMessage};
use lsp_types::notification::{DidOpenTextDocument, Notification};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentSymbolRequest, Formatting,
    GotoDefinition, GotoTypeDefinition, GotoTypeDefinitionParams,
    GotoTypeDefinitionResponse, HoverRequest, InlayHintRequest, References, Request,
    ResolveCompletionItem, SemanticTokensFullRequest, WorkspaceSymbol,
};
use lsp_types::{
    CodeActionContext, CodeActionParams, CodeActionResponse, CompletionItem,
    CompletionParams, CompletionResponse, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentSymbolParams, DocumentSymbolResponse,
    FormattingOptions, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverParams, InlayHint, InlayHintParams, Location, PartialResultParams,
    Position, Range, ReferenceContext, ReferenceParams, SemanticToken,
    SemanticTokens, SemanticTokensParams, SemanticTokensResult, SymbolInformation,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, TextEdit,
    Url, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceSymbolParams,
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use toml_edit::easy as toml;
use wasmer::Store;
use wasmer::WasmerEnv;
use wasmer_wasi::WasiEnv;
use xi_rope::{Rope, RopeDelta};

use crate::buffer::language_id_from_path;
use crate::dispatch::Dispatcher;
use crate::lsp::{LspRpcHandler, NewLspClient};
use crate::lsp::{
    LspRpcHandler, NewLspClient, PluginHandlerNotification, PluginServerHandler,
    PluginServerRpc, PluginServerRpcHandler,
};
use crate::{dispatch::Dispatcher, APPLICATION_NAME};

pub type PluginName = String;

pub enum PluginCatalogRpc {
    ServerRequest {
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        method: &'static str,
        params: Value,
        language_id: Option<String>,
        f: Box<dyn ClonableCallback>,
    },
    ServerNotification {
        method: &'static str,
        params: Value,
        language_id: Option<String>,
    },
    FormatSemanticTokens {
        plugin_id: PluginId,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
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
}

pub enum PluginCatalogNotification {
    PluginServerLoaded(PluginServerRpcHandler),
}

#[derive(WasmerEnv, Clone)]
pub(crate) struct PluginEnv {
    wasi_env: WasiEnv,
    desc: PluginDescription,
    dispatcher: Dispatcher,
}

#[derive(Clone)]
pub(crate) struct Plugin {
    instance: wasmer::Instance,
    env: PluginEnv,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
struct PluginConfig {
    disabled: Vec<String>,
}

#[derive(Clone)]
pub struct PluginCatalogRpcHandler {
    core_rpc: CoreRpcHandler,
    proxy_rpc: ProxyRpcHandler,
    plugin_tx: Sender<PluginCatalogRpc>,
    plugin_rx: Receiver<PluginCatalogRpc>,
    id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, Sender<Result<Value, RpcError>>>>>,
}

impl PluginCatalogRpcHandler {
    pub fn new(core_rpc: CoreRpcHandler, proxy_rpc: ProxyRpcHandler) -> Self {
        let (plugin_tx, plugin_rx) = crossbeam_channel::unbounded();
        Self {
            core_rpc,
            proxy_rpc,
            plugin_tx,
            plugin_rx,
            id: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn handle_response(&self, id: RequestId, result: Result<Value, RpcError>) {
        if let Some(chan) = { self.pending.lock().remove(&id) } {
            chan.send(result);
        }
    }

    pub fn mainloop(&self, plugin: &mut NewPluginCatalog) {
        for msg in &self.plugin_rx {
            match msg {
                PluginCatalogRpc::ServerRequest {
                    plugin_id,
                    request_sent,
                    method,
                    params,
                    language_id,
                    f,
                } => {
                    plugin.handle_server_request(
                        plugin_id,
                        request_sent,
                        method,
                        params,
                        language_id,
                        f,
                    );
                }
                PluginCatalogRpc::ServerNotification {
                    method,
                    params,
                    language_id,
                } => {
                    plugin.handle_server_notification(method, params, language_id);
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
            }
        }
    }

    fn catalog_notification(&self, notification: PluginCatalogNotification) {
        let _ = self.plugin_tx.send(PluginCatalogRpc::Handler(notification));
    }

    fn server_notification<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
    ) {
        let params = serde_json::to_value(params).unwrap();
        let rpc = PluginCatalogRpc::ServerNotification {
            method,
            params,
            language_id,
        };
        let _ = self.plugin_tx.send(rpc);
    }

    fn send_request_to_all_plugins<P, Resp>(
        &self,
        method: &'static str,
        params: P,
        language_id: Option<String>,
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

    fn send_request<P: Serialize>(
        &self,
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        method: &'static str,
        params: P,
        language_id: Option<String>,
        f: impl FnOnce(PluginId, Result<Value, RpcError>) + Send + DynClone + 'static,
    ) {
        let params = serde_json::to_value(params).unwrap();
        let rpc = PluginCatalogRpc::ServerRequest {
            plugin_id,
            request_sent,
            method,
            params,
            language_id,
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, None, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
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
        self.send_request_to_all_plugins(method, params, language_id, cb);
    }

    pub fn completion(
        &self,
        request_id: usize,
        path: &Path,
        input: String,
        position: Position,
    ) {
        eprintln!("send completion {input} {position:?}");
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

    pub fn document_did_open(
        &self,
        path: &Path,
        language_id: String,
        version: i32,
        text: String,
    ) {
        let method = DidOpenTextDocument::METHOD;
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(
                Url::from_file_path(path).unwrap(),
                language_id.clone(),
                version,
                text,
            ),
        };
        self.server_notification(method, params, Some(language_id));
    }

    pub fn plugin_server_loaded(&self, plugin: PluginServerRpcHandler) {
        self.catalog_notification(PluginCatalogNotification::PluginServerLoaded(
            plugin,
        ));
    }
}

pub struct PluginCatalog {
    id_counter: Counter,
    pub items: HashMap<PluginName, PluginDescription>,
    plugins: HashMap<PluginName, Plugin>,
    pub disabled: HashMap<PluginName, PluginDescription>,
    store: Store,
    senders: HashMap<PluginName, Sender<PluginTransmissionMessage>>,
}

enum PluginTransmissionMessage {
    Initialize,
    Stop,
}

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog {
            id_counter: Counter::new(),
            items: HashMap::new(),
            plugins: HashMap::new(),
            disabled: HashMap::new(),
            store: Store::default(),
            senders: HashMap::new(),
        }
    }

    pub fn stop(&mut self) {
        self.items.clear();
        self.plugins.clear();
    }

    pub fn reload(&mut self) {
        self.items.clear();
        self.plugins.clear();
        self.disabled.clear();
        // let _ = self.load();
    }

    pub fn load(&mut self) -> Result<()> {
        let all_plugins = find_all_plugins();
        for plugin_path in &all_plugins {
            match load_plugin(plugin_path) {
                Err(_e) => (),
                Ok(plugin) => {
                    self.items.insert(plugin.name.clone(), plugin.clone());
                }
            }
        }
        let path = config_directory()
            .expect("couldn't obtain config dir")
            .join("plugins.toml");
        let mut file = fs::File::open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        let plugin_config: PluginConfig = toml::from_str(&content)?;
        let mut disabled = HashMap::new();
        for plugin_name in plugin_config.disabled.iter() {
            if let Some(plugin) = self.items.get(plugin_name) {
                disabled.insert(plugin_name.clone(), plugin.clone());
            }
        }
        self.disabled = disabled;
        Ok(())
    }

    pub fn install_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin: PluginDescription,
    ) -> Result<()> {
        let path = plugins_directory()
            .expect("Couldn't obtain plugins dir")
            .join(&plugin.name);
        let _ = fs::remove_dir_all(&path);

    //     fs::create_dir_all(&path)?;

    //     {
    //         let mut file = fs::OpenOptions::new()
    //             .create(true)
    //             .truncate(true)
    //             .write(true)
    //             .open(path.join("plugin.toml"))?;
    //         file.write_all(&toml::to_vec(&plugin)?)?;
    //     }

    //     let mut plugin = plugin;
    //     if let Some(wasm) = plugin.wasm.clone() {
    //         {
    //             let url = format!(
    //                 "https://raw.githubusercontent.com/{}/master/{}",
    //                 plugin.repository, wasm
    //             );
    //             let mut resp = reqwest::blocking::get(url)?;
    //             let mut file = fs::OpenOptions::new()
    //                 .create(true)
    //                 .truncate(true)
    //                 .write(true)
    //                 .open(path.join(&wasm))?;
    //             std::io::copy(&mut resp, &mut file)?;
    //         }

    //         plugin.dir = Some(path.clone());
    //         plugin.wasm = Some(
    //             path.join(&wasm)
    //                 .to_str()
    //                 .ok_or_else(|| anyhow!("path can't to string"))?
    //                 .to_string(),
    //         );

            if let Ok((p, tx)) = self.start_plugin(dispatcher, plugin.clone()) {
                self.plugins.insert(plugin.name.clone(), p);
                self.senders.insert(plugin.name.clone(), tx);
            }
        }
        if let Some(themes) = plugin.themes.as_ref() {
            for theme in themes {
                {
                    let url = format!(
                        "https://raw.githubusercontent.com/{}/HEAD/{}",
                        plugin.repository, theme
                    );
                    let mut resp = reqwest::blocking::get(url)?;
                    let mut file = fs::OpenOptions::new()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(path.join(theme))?;
                    std::io::copy(&mut resp, &mut file)?;
                }
            }
        }
        self.items.insert(plugin.name.clone(), plugin);
        Ok(())
    }

    pub fn remove_plugin(
        &mut self,
        dispatcher: Dispatcher,
        plugin: PluginDescription,
    ) -> Result<()> {
        self.disable_plugin(dispatcher, plugin.clone())?;
        let path = plugins_directory()
            .expect("Couldn't obtain plugins dir")
            .join(&plugin.name);
        fs::remove_dir_all(&path)?;

    //     let _ = self.items.remove(&plugin.name);
    //     let _ = self.plugins.remove(&plugin.name);
    //     let _ = self.disabled.remove(&plugin.name);
    //     Ok(())
    // }

    // pub fn start_all(&mut self, dispatcher: Dispatcher) {
    //     for (_, plugin) in self.items.clone().iter() {
    //         if !self.disabled.contains_key(&plugin.name) {
    //             if let Ok((p, _tx)) =
    //                 self.start_plugin(dispatcher.clone(), plugin.clone())
    //             {
    //                 self.plugins.insert(plugin.name.clone(), p);
    //             }
    //         }
    //     }
    // }

    // pub fn disable_plugin(
    //     &mut self,
    //     _dispatcher: Dispatcher,
    //     plugin_desc: PluginDescription,
    // ) -> Result<()> {
    //     let plugin_tx = self.senders.get(&plugin_desc.name);
    //     if let Some(tx) = plugin_tx {
    //         let local_tx = tx.clone();
    //         thread::spawn(move || {
    //             let _ = local_tx.send(PluginTransmissionMessage::Stop);
    //         });
    //     }
    //     self.senders.remove(&plugin_desc.name);
    //     let plugin = plugin_desc.clone();
    //     self.disabled.insert(plugin_desc.name.clone(), plugin);
    //     let disabled_plugin_list =
    //         self.disabled.clone().into_keys().collect::<Vec<String>>();
    //     let plugin_config = PluginConfig {
    //         disabled: disabled_plugin_list,
    //     };
    //     let home = home_dir().unwrap();
    //     let path = home.join(".lapce").join("config");
    //     fs::create_dir_all(&path)?;
    //     {
    //         let mut file = fs::OpenOptions::new()
    //             .create(true)
    //             .truncate(true)
    //             .write(true)
    //             .open(path.join("plugins.toml"))?;
    //         file.write_all(&toml::to_vec(&plugin_config)?)?;
    //     }

    //     Ok(())
    // }

    //     let local_plugin = plugin.clone();
    //     let (tx, rx) = mpsc::channel();

    //     thread::spawn(move || loop {
    //         match rx.recv() {
    //             Ok(PluginTransmissionMessage::Initialize) => {
    //                 let initialize = local_plugin
    //                     .instance
    //                     .exports
    //                     .get_function("initialize")
    //                     .unwrap();
    //                 wasi_write_object(
    //                     &local_plugin.env.wasi_env,
    //                     &PluginInfo {
    //                         os: std::env::consts::OS.to_string(),
    //                         arch: std::env::consts::ARCH.to_string(),
    //                         configuration: plugin_desc.clone().configuration,
    //                     },
    //                 );
    //                 initialize.call(&[]).unwrap();
    //             }
    //             Ok(PluginTransmissionMessage::Stop) => {
    //                 let stop = local_plugin.instance.exports.get_function("stop");
    //                 if let Ok(stop_func) = stop {
    //                     stop_func.call(&[]).unwrap();
    //                 } else if let Some(Value::Object(conf)) =
    //                     &plugin_desc.configuration
    //                 {
    //                     if let Some(Value::String(lang)) = conf.get("language_id") {
    //                         local_plugin
    //                             .env
    //                             .dispatcher
    //                             .lsp
    //                             .lock()
    //                             .stop_language_lsp(lang);
    //                     }
    //                 }
    //                 break;
    //             }
    //             // There was an error when receiving, which means that the other end was closed.
    //             // So we simply shutdown this thread by breaking out of the loop
    //             Err(_) => break,
    //         }
    //     });
    //     tx.send(PluginTransmissionMessage::Initialize)?;
    //     Ok((plugin, tx))
    // }

    pub fn disable_plugin(
        &mut self,
        _dispatcher: Dispatcher,
        plugin_desc: PluginDescription,
    ) -> Result<()> {
        let plugin_tx = self.senders.get(&plugin_desc.name);
        if let Some(tx) = plugin_tx {
            let local_tx = tx.clone();
            thread::spawn(move || {
                let _ = local_tx.send(PluginTransmissionMessage::Stop);
            });
        }
        self.senders.remove(&plugin_desc.name);
        let plugin = plugin_desc.clone();
        self.disabled.insert(plugin_desc.name.clone(), plugin);
        let disabled_plugin_list =
            self.disabled.clone().into_keys().collect::<Vec<String>>();
        let plugin_config = PluginConfig {
            disabled: disabled_plugin_list,
        };
        let path = config_directory().expect("couldn't obtain config dir");
        fs::create_dir_all(&path)?;
        {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path.join("plugins.toml"))?;
            file.write_all(&toml::to_vec(&plugin_config)?)?;
        }

        Ok(())
    }

    // pub fn enable_plugin(
    //     &mut self,
    //     dispatcher: Dispatcher,
    //     plugin_desc: PluginDescription,
    // ) -> Result<()> {
    //     let mut plugin = plugin_desc.clone();
    //     let home = home_dir().unwrap();
    //     let path = home.join(".lapce").join("plugins").join(&plugin.name);
    //     plugin.dir = Some(path.clone());
    //     if let Some(wasm) = plugin.wasm {
    //         plugin.wasm = Some(
    //             path.join(&wasm)
    //                 .to_str()
    //                 .ok_or_else(|| anyhow!("path can't to string"))?
    //                 .to_string(),
    //         );
    //         self.start_plugin(dispatcher, plugin.clone())?;
    //         self.disabled.remove(&plugin_desc.name);
    //         let config_path = home.join(".lapce").join("config");
    //         let disabled_plugin_list =
    //             self.disabled.clone().into_keys().collect::<Vec<String>>();
    //         let plugin_config = PluginConfig {
    //             disabled: disabled_plugin_list,
    //         };
    //         {
    //             let mut file = fs::OpenOptions::new()
    //                 .create(true)
    //                 .truncate(true)
    //                 .write(true)
    //                 .open(config_path.join("plugins.toml"))?;
    //             file.write_all(&toml::to_vec(&plugin_config)?)?;
    //         }
    //         Ok(())
    //     } else {
    //         Err(anyhow!("no wasm in plugin"))
    //     }
    // }

    pub fn next_plugin_id(&mut self) -> PluginId {
        PluginId(self.id_counter.next())
    }
}

impl Default for PluginCatalog {
    fn default() -> Self {
        Self::new()
    }
}

// pub(crate) fn lapce_exports(store: &Store, plugin_env: &PluginEnv) -> ImportObject {
//     macro_rules! lapce_export {
//         ($($host_function:ident),+ $(,)?) => {
//             wasmer::imports! {
//                 "lapce" => {
//                     $(stringify!($host_function) =>
//                         wasmer::Function::new_native_with_env(store, plugin_env.clone(), $host_function),)+
//                 }
//             }
//         }
//     }

//     lapce_export! {
//         host_handle_notification,
//     }
// }

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

fn number_from_id(id: &Id) -> u64 {
    match *id {
        Id::Num(n) => n as u64,
        Id::Str(ref s) => s
            .parse::<u64>()
            .expect("failed to convert string id to u64"),
        _ => panic!("unexpected value for id: None"),
    }
}

// fn host_handle_notification(plugin_env: &PluginEnv) {
//     let notification: Result<PluginNotification> =
//         wasi_read_object(&plugin_env.wasi_env);
//     if let Ok(notification) = notification {
//         match notification {
//             PluginNotification::StartLspServer {
//                 exec_path,
//                 language_id,
//                 options,
//                 system_lsp,
//             } => {
//                 let exec_path = if system_lsp.unwrap_or(false) {
//                     // System LSP should be handled by PATH during
//                     // process creation, so we forbid anything that
//                     // is not just an executable name
//                     match PathBuf::from(&exec_path).file_name() {
//                         Some(v) => v.to_str().unwrap().to_string(),
//                         None => return,
//                     }
//                 } else {
//                     plugin_env
//                         .desc
//                         .dir
//                         .clone()
//                         .unwrap()
//                         .join(&exec_path)
//                         .to_str()
//                         .unwrap()
//                         .to_string()
//                 };
//                 plugin_env.dispatcher.lsp.lock().start_server(
//                     &exec_path,
//                     &language_id,
//                     options,
//                 );
//             }
//             PluginNotification::DownloadFile { url, path } => {
//                 let mut resp = reqwest::blocking::get(url).expect("request failed");
//                 let mut out = fs::File::create(
//                     plugin_env.desc.dir.clone().unwrap().join(path),
//                 )
//                 .expect("failed to create file");
//                 std::io::copy(&mut resp, &mut out).expect("failed to copy content");
//             }
//             PluginNotification::LockFile { path } => {
//                 let path = plugin_env.desc.dir.clone().unwrap().join(path);
//                 let mut n = 0;
//                 loop {
//                     if let Ok(_file) = fs::OpenOptions::new()
//                         .write(true)
//                         .create_new(true)
//                         .open(&path)
//                     {
//                         return;
//                     }
//                     if n > 10 {
//                         return;
//                     }
//                     n += 1;
//                     let mut hotwatch =
//                         Hotwatch::new().expect("hotwatch failed to initialize!");
//                     let (tx, rx) = crossbeam_channel::bounded(1);
//                     let _ = hotwatch.watch(&path, move |_event| {
//                         #[allow(deprecated)]
//                         let _ = tx.send(0);
//                     });
//                     let _ = rx.recv_timeout(Duration::from_secs(10));
//                 }
//             }
//             PluginNotification::MakeFileExecutable { path } => {
//                 let _ = Command::new("chmod")
//                     .arg("+x")
//                     .arg(&plugin_env.desc.dir.clone().unwrap().join(path))
//                     .output();
//             }
//         }
//     }
// }

pub fn wasi_read_string(wasi_env: &WasiEnv) -> Result<String> {
    let mut state = wasi_env.state();
    let wasi_file = state
        .fs
        .stdout_mut()?
        .as_mut()
        .ok_or_else(|| anyhow!("can't get stdout"))?;
    let mut buf = String::new();
    wasi_file.read_to_string(&mut buf)?;
    Ok(buf)
}

pub fn wasi_read_object<T: DeserializeOwned>(wasi_env: &WasiEnv) -> Result<T> {
    let json = wasi_read_string(wasi_env)?;
    Ok(serde_json::from_str(&json)?)
}

pub fn wasi_write_string(wasi_env: &WasiEnv, buf: &str) {
    let mut state = wasi_env.state();
    let wasi_file = state.fs.stdin_mut().unwrap().as_mut().unwrap();
    writeln!(wasi_file, "{}\r", buf).unwrap();
}

pub fn wasi_write_object(wasi_env: &WasiEnv, object: &(impl Serialize + ?Sized)) {
    wasi_write_string(wasi_env, &serde_json::to_string(&object).unwrap());
}

pub struct PluginHandler {}

fn find_all_plugins() -> Vec<PathBuf> {
    let mut plugin_paths = Vec::new();
    let path = plugins_directory().expect("Couldn't obtain plugin dirs");
    let _ = path.read_dir().map(|dir| {
        dir.flat_map(|item| item.map(|p| p.path()).ok())
            .map(|dir| dir.join("plugin.toml"))
            .filter(|f| f.exists())
            .for_each(|f| plugin_paths.push(f))
    });
    plugin_paths
}

fn load_plugin(path: &Path) -> Result<PluginDescription> {
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut plugin: PluginDescription = toml::from_str(&contents)?;
    plugin.dir = Some(path.parent().unwrap().canonicalize()?);
    plugin.wasm = plugin.wasm.as_ref().and_then(|wasm| {
        Some(
            path.parent()?
                .join(wasm)
                .canonicalize()
                .ok()?
                .to_str()?
                .to_string(),
        )
    });
    plugin.themes = plugin.themes.as_ref().map(|themes| {
        themes
            .iter()
            .filter_map(|theme| {
                Some(
                    path.parent()?
                        .join(theme)
                        .canonicalize()
                        .ok()?
                        .to_str()?
                        .to_string(),
                )
            })
            .collect()
    });
    Ok(plugin)
}

pub fn plugins_directory() -> Option<PathBuf> {
    match ProjectDirs::from("dev", "lapce", APPLICATION_NAME) {
        Some(dir) => {
            if !dir.data_local_dir().exists() {
                match std::fs::create_dir_all(dir.data_local_dir()) {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!(target: "lapce_proxy::plugin::plugins_directory", "{e}")
                    }
                };
            }
            Some(dir.data_local_dir().join("plugins"))
        }
        None => None,
    }
}

pub fn config_directory() -> Option<PathBuf> {
    match ProjectDirs::from("dev", "lapce", APPLICATION_NAME) {
        Some(dir) => {
            if !dir.config_dir().exists() {
                match std::fs::create_dir_all(dir.config_dir()) {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!(target: "lapce_proxy::plugin::config_directory", "{e}")
                    }
                };
            }
            Some(dir.config_dir().to_path_buf())
        }
        None => None,
    }
}

pub fn send_plugin_notification(
    plugin_sender: &Sender<PluginRpcMessage>,
    notification: NewPluginNotification,
) {
    let _ = plugin_sender.send(RpcMessage::Notification(notification));
}
