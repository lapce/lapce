#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{self, Child, ChildStderr, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use jsonrpc_lite::{Id, JsonRpc, Params};
use lapce_core::encoding::offset_utf16_to_utf8;
use lapce_rpc::{
    buffer::BufferId,
    lsp::{LspNotification, LspRequest, LspResponse, LspRpcMessage},
    proxy::{ProxyNotification, ProxyRequest, ProxyRpcMessage},
    style::{LineStyle, SemanticStyles, Style},
    NewHandler, NewRpcHandler, RequestId,
};
use lsp_types::{request::GotoTypeDefinitionParams, *};
use parking_lot::Mutex;
use serde_json::{json, to_value, Value};

use crate::{buffer::Buffer, dispatch::Dispatcher};

pub type Callback = Box<dyn Callable>;
const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

pub trait Callable: Send {
    fn call(self: Box<Self>, client: &LspClient, result: Result<Value>);
}

impl<F: Send + FnOnce(&LspClient, Result<Value>)> Callable for F {
    fn call(self: Box<F>, client: &LspClient, result: Result<Value>) {
        (*self)(client, result)
    }
}

pub struct NewLspCatalog {}

impl NewHandler<LspRequest, LspNotification, LspResponse> for NewLspCatalog {
    fn handle_notification(&mut self, rpc: LspNotification) {
        todo!()
    }

    fn handle_request(&mut self, rpc: LspRequest) {
        todo!()
    }
}

impl NewLspCatalog {
    pub fn mainloop(
        proxy_sender: Sender<ProxyRpcMessage>,
        lsp_receiver: Receiver<LspRpcMessage>,
    ) {
        let mut lsp = Self {};
        let mut handler = NewRpcHandler::new(proxy_sender);
        handler.mainloop(lsp_receiver, &mut lsp);
    }
}

pub struct LspCatalog {
    pub dispatcher: Option<Dispatcher>,
    clients: HashMap<String, Arc<LspClient>>,
}

pub struct LspState {
    next_id: u64,
    writer: Box<dyn Write + Send>,
    process: Child,
    pending: HashMap<u64, Callback>,
    pub server_capabilities: Option<ServerCapabilities>,
    pub opened_documents: HashMap<BufferId, Url>,
    pub is_initialized: bool,
    pub did_save_capabilities: Vec<DidSaveCapability>,
}

pub struct DocumentFilter {
    /// The document must have this language id, if it exists
    pub language_id: Option<String>,
    /// The document's path must match this glob, if it exists
    pub pattern: Option<globset::GlobMatcher>,
    // TODO: URI Scheme from lsp-types document filter
}
impl DocumentFilter {
    /// Constructs a document filter from the LSP version
    /// This ignores any fields that are badly constructed
    fn from_lsp_filter_loose(filter: lsp_types::DocumentFilter) -> DocumentFilter {
        DocumentFilter {
            language_id: filter.language,
            // TODO: clean this up
            pattern: filter
                .pattern
                .as_deref()
                .map(globset::Glob::new)
                .and_then(Result::ok)
                .map(|x| globset::Glob::compile_matcher(&x)),
        }
    }
}
pub struct DidSaveCapability {
    /// A filter on what documents this applies to
    filter: DocumentFilter,
    /// Whether we are supposed to include the text when sending a didSave event
    include_text: bool,
}

#[derive(Clone)]
pub struct LspClient {
    exec_path: String,
    args: Vec<String>,
    options: Option<Value>,
    state: Arc<Mutex<LspState>>,
    dispatcher: Dispatcher,
    active: Arc<AtomicBool>,
}

impl LspCatalog {
    pub fn new() -> LspCatalog {
        LspCatalog {
            dispatcher: None,
            clients: HashMap::new(),
        }
    }

    pub fn stop(&mut self) {
        for (_, client) in self.clients.iter() {
            client.stop();
        }
        self.clients.clear();
        self.dispatcher.take();
    }

    pub fn stop_language_lsp(&mut self, lang: &String) {
        if let Some(lsp) = self.clients.get(lang) {
            lsp.stop();
        }
    }

    pub fn start_server(
        &mut self,
        exec_path: &str,
        language_id: &str,
        options: Option<Value>,
    ) {
        let args = self
            .get_plugin_binary_args(options.clone())
            .unwrap_or_default();
        let client = LspClient::new(
            language_id.to_string(),
            exec_path,
            options,
            args,
            self.dispatcher.clone().unwrap(),
        );
        self.clients.insert(language_id.to_string(), client);
    }

    fn get_plugin_binary_args(
        &mut self,
        option: Option<Value>,
    ) -> Option<Vec<String>> {
        let option = option?;

        match option["binary"].as_object()?.get("args")?.as_array() {
            Some(options) => {
                return Some(
                    options
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                )
            }
            None => {
                log::warn!("args value should be of type [String].");
            }
        };

        None
    }

    pub fn new_buffer(
        &self,
        buffer_id: &BufferId,
        path: &str,
        language_id: &str,
        text: String,
    ) {
        if let Some(client) = self.clients.get(language_id) {
            {
                let state = client.state.lock();
                if !state.is_initialized {
                    return;
                }
            }

            let document_uri = Url::from_file_path(path).unwrap();
            client.send_did_open(buffer_id, document_uri, language_id, text);
        }
    }

    pub fn save_buffer(&self, buffer: &Buffer, workspace_path: &Path) {
        for (client_language_id, client) in self.clients.iter() {
            {
                let state = client.state.lock();
                if !state.is_initialized {
                    return;
                }
            }

            // Get rid of the workspace path prefix so that it can be used with the filters
            let buffer_path = buffer
                .path
                .strip_prefix(workspace_path)
                .unwrap_or(&buffer.path);

            let mut passed_filter = client_language_id == &buffer.language_id;
            let mut include_text = false;
            if !passed_filter {
                let lsp_state = client.state.lock();

                // TODO: Should we iterate in reverse order so that later capabilities
                // can overwrite old ones?
                // Find the first capability that wants this file, if any.
                for cap in &lsp_state.did_save_capabilities {
                    if let Some(language_id) = &cap.filter.language_id {
                        if language_id != &buffer.language_id {
                            continue;
                        }
                    }

                    if let Some(pattern) = &cap.filter.pattern {
                        if !pattern.is_match(buffer_path) {
                            continue;
                        }
                    }

                    passed_filter = true;
                    include_text = cap.include_text;
                    break;
                }

                // Get rid of the mutex guard
                drop(lsp_state);
            }

            if passed_filter {
                let uri = client.get_uri(buffer);
                let text = if include_text {
                    Some(buffer.get_document())
                } else {
                    None
                };
                client.send_did_save(uri, text);
            }
        }
    }

    pub fn get_semantic_tokens(&self, id: RequestId, buffer: &Buffer) {
        let buffer = buffer.clone();
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.semantic_tokens_provider.as_ref())
                    .map(|prov| match prov {
                        SemanticTokensServerCapabilities::SemanticTokensOptions(opts) => opts,
                        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(reg) => &reg.semantic_tokens_options,
                    }).map(|opts| {
                        // TODO: handle opts.full delta
                        opts.full.is_some()
                    }).unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(&buffer);
            let old_buffer = buffer;

            client.request_semantic_tokens(uri, move |lsp_client, result| {
                let buffers = lsp_client.dispatcher.buffers.lock();
                let buffer = buffers.get(&old_buffer.id).unwrap();
                // If the revision changed while we were requesting, then we refuse the request as it will have made another one
                if buffer.rev != old_buffer.rev {
                    lsp_client
                        .dispatcher
                        .respond(id, Err(anyhow!("revision changed")));
                    return;
                }

                let lsp_state = lsp_client.state.lock();
                let semantic_tokens_provider = &lsp_state
                    .server_capabilities
                    .as_ref()
                    .unwrap()
                    .semantic_tokens_provider;
                let result = result.and_then(|value| {
                    format_semantic_styles(buffer, semantic_tokens_provider, value)
                        .map(|styles| {
                            serde_json::to_value(SemanticStyles {
                                rev: buffer.rev,
                                buffer_id: buffer.id,
                                path: buffer.path.clone(),
                                styles,
                                len: buffer.len(),
                            })
                            .unwrap()
                        })
                        .ok_or_else(|| anyhow!("can't format semantic styles"))
                });

                lsp_client.dispatcher.respond(id, result);
            });
        }
    }

    pub fn get_document_symbols(&self, id: RequestId, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.document_symbol_provider.as_ref())
                    .map(|prov| prov != &OneOf::Left(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_document_symbols(uri, move |lsp_client, result| {
                lsp_client.dispatcher.respond(id, result);
            });
        }
    }

    pub fn get_workspace_symbols(
        &self,
        id: RequestId,
        buffer: &Buffer,
        query: String,
    ) {
        // TODO: We could collate workspace symbols from all the lsps?
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.workspace_symbol_provider.as_ref())
                    .map(|prov| prov != &OneOf::Left(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            client.request_workspace_symbols(query, move |lsp_client, result| {
                lsp_client.dispatcher.respond(id, result);
            });
        }
    }

    pub fn get_document_formatting(&self, id: RequestId, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.document_formatting_provider.as_ref())
                    .map(|prov| prov != &OneOf::Left(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_document_formatting(uri, move |lsp_client, result| {
                lsp_client.dispatcher.respond(id, result);
            });
        } else {
            self.dispatcher
                .as_ref()
                .unwrap()
                .respond(id, Err(anyhow!("no document formatting")));
        }
    }

    pub fn get_completion(
        &self,
        id: RequestId,
        _request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                // TODO: pay attention to trigger characters
                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .map(|cap| cap.completion_provider.is_some())
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_completion(uri, position, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}",e),
                        })
                    }
                }
                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn completion_resolve(
        &self,
        id: RequestId,
        buffer: &Buffer,
        completion_item: &CompletionItem,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.completion_resolve(completion_item, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}",e),
                        })
                    }
                }
                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_hover(
        &self,
        id: RequestId,
        _request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.hover_provider.as_ref())
                    .map(|prov| prov != &HoverProviderCapability::Simple(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_hover(uri, position, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}", e),
                        })
                    }
                }
                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_signature(&self, id: RequestId, buffer: &Buffer, position: Position) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                // TODO: use the trigger characters fields
                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .map(|cap| cap.signature_help_provider.is_some())
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_signature(uri, position, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}",e),
                        })
                    }
                }
                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_references(
        &self,
        id: RequestId,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.references_provider.as_ref())
                    .map(|prov| prov != &OneOf::Left(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_references(uri, position, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}",e),
                        })
                    }
                }
                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_inlay_hints(&self, id: RequestId, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.inlay_hint_provider.as_ref())
                    .map(|prov| prov != &OneOf::Left(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            // Range over the entire buffer
            let range = Range {
                start: Position::new(0, 0),
                end: buffer.offset_to_position(buffer.len()).unwrap(),
            };
            client.request_inlay_hints(uri, range, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}", e),
                        })
                    }
                }

                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_code_actions(
        &self,
        id: RequestId,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.code_action_provider.as_ref())
                    .map(|prov| prov != &CodeActionProviderCapability::Simple(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            let range = Range {
                start: position,
                end: position,
            };
            client.request_code_actions(uri, range, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}",e),
                        })
                    }
                }
                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_definition(
        &self,
        id: RequestId,
        _request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.definition_provider.as_ref())
                    .map(|prov| prov != &OneOf::Left(false))
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_definition(uri, position, move |lsp_client, result| {
                let mut resp = json!({ "id": id });
                match result {
                    Ok(v) => resp["result"] = v,
                    Err(e) => {
                        resp["error"] = json!({
                            "code": 0,
                            "message": format!("{}",e),
                        })
                    }
                }
                let _ = lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_type_definition(
        &self,
        id: RequestId,
        _request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            {
                let state = client.state.lock();

                if !state.is_initialized {
                    return;
                }

                let is_enabled = state
                    .server_capabilities
                    .as_ref()
                    .and_then(|cap| cap.type_definition_provider.as_ref())
                    .map(|prov| {
                        prov != &TypeDefinitionProviderCapability::Simple(false)
                    })
                    .unwrap_or(false);

                if !is_enabled {
                    return;
                }
            }

            let uri = client.get_uri(buffer);
            client.request_type_definition(
                uri,
                position,
                move |lsp_client, result| {
                    let mut resp = json!({ "id": id });
                    match result {
                        Ok(v) => resp["result"] = v,
                        Err(e) => {
                            resp["error"] = json!({
                                "code": 0,
                                "message": format!("{}", e),
                            })
                        }
                    }

                    let _ = lsp_client.dispatcher.sender.send(resp);
                },
            );
        }
    }

    pub fn update(
        &self,
        buffer: &Buffer,
        content_change: &TextDocumentContentChangeEvent,
        rev: u64,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.update(buffer, content_change, rev);
        }
    }
}

impl Default for LspCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl LspClient {
    pub fn new(
        language_id: String,
        exec_path: &str,
        options: Option<Value>,
        args: Vec<String>,
        dispatcher: Dispatcher,
    ) -> Arc<LspClient> {
        //TODO: better handling of binary args in plugin
        let workspace = dispatcher.workspace.lock().clone();
        let mut process = Self::process(workspace, exec_path, args.clone());
        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));
        let stdout = process.stdout.take().unwrap();
        let stderr = process.stderr.take().unwrap();

        let lsp_client = Arc::new(LspClient {
            dispatcher,
            exec_path: exec_path.to_string(),
            args,
            options,
            state: Arc::new(Mutex::new(LspState {
                next_id: 0,
                writer,
                process,
                pending: HashMap::new(),
                server_capabilities: None,
                opened_documents: HashMap::new(),
                is_initialized: false,
                did_save_capabilities: Vec::new(),
            })),
            active: Arc::new(AtomicBool::new(true)),
        });

        lsp_client.handle_stdout(stdout);
        lsp_client.handle_stderr(stderr, language_id);
        lsp_client.initialize();

        lsp_client
    }

    fn handle_stdout(&self, stdout: ChildStdout) {
        let local_lsp_client = self.clone();
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stdout));
            loop {
                match read_message(&mut reader) {
                    Ok(message_str) => {
                        local_lsp_client.handle_message(message_str.as_ref());
                    }
                    Err(_err) => {
                        if !local_lsp_client.active.load(Ordering::Acquire) {
                            return;
                        }
                        local_lsp_client.stop();
                        local_lsp_client.reload();
                        return;
                    }
                };
            }
        });
    }

    fn handle_stderr(&self, stderr: ChildStderr, language_id: String) {
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stderr));
            let mut buffer = String::new();

            loop {
                buffer.clear();
                match reader.read_line(&mut buffer) {
                    Ok(bytes) => {
                        if bytes == 0 {
                            return;
                        }
                    }
                    Err(_) => return,
                }
                if buffer.trim().is_empty() {
                    continue;
                }
                log::debug!("[LSP::{}] {}", language_id, buffer.trim())
            }
        });
    }

    fn process(
        workspace: Option<PathBuf>,
        exec_path: &str,
        args: Vec<String>,
    ) -> Child {
        let mut process = Command::new(exec_path);
        if let Some(workspace) = workspace {
            process.current_dir(&workspace);
        }

        process.args(args);

        #[cfg(target_os = "windows")]
        let process = process.creation_flags(0x08000000);
        process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Error Occurred")
    }

    fn reload(&self) {
        //TODO: avoid clone using a &[String] ?
        let mut process = Self::process(
            self.dispatcher.workspace.lock().clone(),
            &self.exec_path,
            self.args.clone(),
        );
        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));
        let stdout = process.stdout.take().unwrap();

        let mut state = self.state.lock();
        state.next_id = 0;
        state.pending.clear();
        state.opened_documents.clear();
        state.server_capabilities = None;
        state.is_initialized = false;
        state.writer = writer;
        state.process = process;

        self.handle_stdout(stdout);
        self.initialize();
    }

    fn stop(&self) {
        self.active.store(false, Ordering::Release);
        let _ = self.state.lock().process.kill();
        let _ = self.state.lock().process.wait();
    }

    pub fn get_uri(&self, buffer: &Buffer) -> Url {
        let exists = {
            let state = self.state.lock();
            state.opened_documents.contains_key(&buffer.id)
        };
        if !exists {
            let document_uri =
                Url::from_file_path(&buffer.path).unwrap_or_else(|_| {
                    panic!("Failed to create URL from path {:?}", buffer.path)
                });
            self.send_did_open(
                &buffer.id,
                document_uri,
                &buffer.language_id,
                buffer.get_document(),
            );
        }
        self.state
            .lock()
            .opened_documents
            .get(&buffer.id)
            .unwrap()
            .clone()
    }

    pub fn handle_message(&self, message: &str) {
        log::debug!(target: "lapce_proxy::dispatch::handle_message", "{message}");
        match JsonRpc::parse(message) {
            Ok(value @ JsonRpc::Request(_)) => {
                let id = value.get_id().unwrap();
                self.handle_request(
                    value.get_method().unwrap(),
                    id,
                    value.get_params().unwrap(),
                )
            }
            Ok(value @ JsonRpc::Notification(_)) => {
                self.handle_notification(
                    value.get_method().unwrap(),
                    value.get_params().unwrap(),
                );
            }
            Ok(value @ JsonRpc::Success(_)) => {
                let id = number_from_id(&value.get_id().unwrap());
                let result = value.get_result().unwrap();
                self.handle_response(id, Ok(result.clone()));
            }
            Ok(value @ JsonRpc::Error(_)) => {
                let id = number_from_id(&value.get_id().unwrap());
                let error = value.get_error().unwrap();
                self.handle_response(id, Err(anyhow!("{}", error)));
            }
            Err(_err) => {}
        }
    }

    pub fn handle_request(&self, method: &str, id: Id, params: Params) {
        match method {
            "window/workDoneProgress/create" => {
                // Token is ignored as the workProgress Widget is always working
                // In the future, for multiple workProgress Handling we should
                // probably store the token
                self.send_success_response(id, &Value::Null);
            }
            "client/registerCapability" => {
                if let Ok(registrations) =
                    serde_json::from_value::<RegistrationParams>(json!(params))
                {
                    for registration in registrations.registrations {
                        match registration.method.as_str() {
                            "textDocument/didSave" => {
                                if let Some(options) = registration.register_options {
                                    if let Ok(options) = serde_json::from_value::<TextDocumentSaveRegistrationOptions>(options) {
                                        if let Some(selectors) = options.text_document_registration_options.document_selector {
                                            // TODO: is false a reasonable default?
                                            let include_text = options.include_text.unwrap_or(false);

                                            let mut lsp_state = self.state.lock();

                                            // Add each selector our did save filtering
                                            for selector in selectors {
                                                let filter = DocumentFilter::from_lsp_filter_loose(selector);
                                                let cap = DidSaveCapability {
                                                    filter,
                                                    include_text,
                                                };

                                                lsp_state.did_save_capabilities.push(cap);
                                            }
                                        }
                                    }
                                }
                                // TODO: report error?
                            }
                            _ => println!("Received unhandled client/registerCapability request {}", registration.method),
                        }
                    }
                }
            }
            "workspace/configuration" => {
                if let Ok(config) =
                    serde_json::from_value::<ConfigurationParams>(json!(params))
                {
                    let items = config
                        .items
                        .into_iter()
                        .map(|_item| Value::Null)
                        .collect::<Vec<Value>>();

                    self.send_success_response(id, &Value::Array(items));
                }
            }
            method => {
                println!("Received unhandled request {method}");
            }
        }
    }

    pub fn handle_notification(&self, method: &str, params: Params) {
        match method {
            "textDocument/publishDiagnostics" => {
                self.dispatcher.send_notification(
                    "publish_diagnostics",
                    json!({
                        "diagnostics": params,
                    }),
                );
            }
            "$/progress" => {
                self.dispatcher.send_notification(
                    "work_done_progress",
                    json!({
                        "progress": params,
                    }),
                );
            }
            "window/showMessage" => {
                // TODO: send message to display
            }
            "window/logMessage" => {
                // TODO: We should log the message here. Waiting for
                // the discussion about handling plugins logs before doing anything
            }
            "experimental/serverStatus" => {
                //TODO: Logging of server status
            }
            method => {
                println!("Received unhandled notification {}", method);
            }
        }
    }

    pub fn handle_response(&self, id: u64, result: Result<Value>) {
        let callback =
            {
                self.state.lock().pending.remove(&id).unwrap_or_else(|| {
                    panic!("id {} missing from request table", id)
                })
            };
        callback.call(self, result);
    }

    pub fn write(&self, msg: &str) -> Result<()> {
        let mut state = self.state.lock();
        state.writer.write_all(msg.as_bytes())?;
        state.writer.flush()?;
        Ok(())
    }

    fn send_rpc(&self, value: &Value) {
        let rpc = match prepare_lsp_json(value) {
            Ok(r) => r,
            Err(err) => panic!("Encoding Error {:?}", err),
        };

        let _ = self.write(rpc.as_ref());
    }

    pub fn send_notification(&self, method: &str, params: Params) {
        let notification = JsonRpc::notification_with_params(method, params);
        let res = to_value(&notification).unwrap();
        self.send_rpc(&res);
    }

    pub fn send_request(&self, method: &str, params: Params, completion: Callback) {
        log::debug!(target: "lapce_proxy::dispatch::send_request", "{method} : {params:?}");
        let request = {
            let mut state = self.state.lock();
            let next_id = state.next_id;
            state.pending.insert(next_id, completion);
            state.next_id += 1;

            JsonRpc::request_with_params(Id::Num(next_id as i64), method, params)
        };

        self.send_rpc(&to_value(&request).unwrap());
    }

    pub fn send_success_response(&self, id: Id, result: &Value) {
        let response = JsonRpc::success(id, result);

        self.send_rpc(&to_value(&response).unwrap());
    }

    pub fn send_error_response(
        &self,
        id: jsonrpc_lite::Id,
        error: jsonrpc_lite::Error,
    ) {
        let response = JsonRpc::error(id, error);

        self.send_rpc(&to_value(&response).unwrap());
    }

    fn initialize(&self) {
        if let Some(workspace) = self.dispatcher.workspace.lock().clone() {
            let root_url = Url::from_directory_path(workspace).unwrap();
            let (sender, receiver) = channel();
            self.send_initialize(Some(root_url), move |lsp_client, result| {
                if let Ok(result) = result {
                    {
                        let init_result: InitializeResult =
                            serde_json::from_value(result).unwrap();
                        let mut state = lsp_client.state.lock();
                        state.server_capabilities = Some(init_result.capabilities);
                        state.is_initialized = true;
                    }
                    lsp_client.send_initialized();
                }
                let _ = sender.send(true);
            });
            let _ = receiver.recv_timeout(Duration::from_millis(1000));
        }
    }

    pub fn send_did_open(
        &self,
        buffer_id: &BufferId,
        document_uri: Url,
        language_id: &str,
        document_text: String,
    ) {
        let is_initialized = {
            let mut state = self.state.lock();
            state
                .opened_documents
                .insert(*buffer_id, document_uri.clone());
            state.is_initialized
        };

        if !is_initialized {
            self.initialize();
            return;
        }

        let text_document_did_open_params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                language_id: language_id.to_string(),
                uri: document_uri,
                version: 0,
                text: document_text,
            },
        };
        let params = Params::from(
            serde_json::to_value(text_document_did_open_params).unwrap(),
        );
        self.send_notification("textDocument/didOpen", params);
    }

    pub fn send_did_save(&self, uri: Url, text: Option<String>) {
        let params = DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
            text,
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_notification("textDocument/didSave", params);
    }

    pub fn send_initialized(&self) {
        self.send_notification("initialized", Params::from(json!({})));
    }

    pub fn send_initialize<CB>(&self, root_uri: Option<Url>, on_init: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let client_capabilities = ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                synchronization: Some(TextDocumentSyncClientCapabilities {
                    did_save: Some(true),
                    dynamic_registration: Some(true),
                    ..Default::default()
                }),
                completion: Some(CompletionClientCapabilities {
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: Some(true),
                        resolve_support: Some(
                            CompletionItemCapabilityResolveSupport {
                                properties: vec!["additionalTextEdits".to_string()],
                            },
                        ),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                // signature_help: Some(SignatureHelpCapability {
                //     signature_information: Some(SignatureInformationSettings {
                //         parameter_information: Some(ParameterInformationSettings {
                //             label_offset_support: Some(true),
                //         }),
                //         active_parameter_support: Some(true),
                //         documentation_format: Some(vec![
                //             MarkupKind::Markdown,
                //             MarkupKind::PlainText,
                //         ]),
                //     }),
                //     ..Default::default()
                // }),
                hover: Some(HoverClientCapabilities {
                    content_format: Some(vec![
                        MarkupKind::Markdown,
                        MarkupKind::PlainText,
                    ]),
                    ..Default::default()
                }),
                inlay_hint: Some(InlayHintClientCapabilities {
                    ..Default::default()
                }),
                code_action: Some(CodeActionClientCapabilities {
                    code_action_literal_support: Some(CodeActionLiteralSupport {
                        code_action_kind: CodeActionKindLiteralSupport {
                            value_set: vec![
                                CodeActionKind::EMPTY.as_str().to_string(),
                                CodeActionKind::QUICKFIX.as_str().to_string(),
                                CodeActionKind::REFACTOR.as_str().to_string(),
                                CodeActionKind::REFACTOR_EXTRACT
                                    .as_str()
                                    .to_string(),
                                CodeActionKind::REFACTOR_INLINE.as_str().to_string(),
                                CodeActionKind::REFACTOR_REWRITE
                                    .as_str()
                                    .to_string(),
                                CodeActionKind::SOURCE.as_str().to_string(),
                                CodeActionKind::SOURCE_ORGANIZE_IMPORTS
                                    .as_str()
                                    .to_string(),
                            ],
                        },
                    }),
                    ..Default::default()
                }),
                semantic_tokens: Some(SemanticTokensClientCapabilities {
                    ..Default::default()
                }),
                type_definition: Some(GotoCapability {
                    // Note: This is explicitly specified rather than left to the Default because
                    // of a bug in lsp-types https://github.com/gluon-lang/lsp-types/pull/244
                    link_support: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                show_message: Some(ShowMessageRequestClientCapabilities {
                    message_action_item: Some(MessageActionItemCapabilities {
                        additional_properties_support: Some(true),
                    }),
                }),
                ..Default::default()
            }),
            workspace: Some(WorkspaceClientCapabilities {
                symbol: Some(WorkspaceSymbolClientCapabilities {
                    ..Default::default()
                }),
                configuration: Some(true),
                ..Default::default()
            }),

            // TODO: We could support the extension for utf8 and the upcoming stable support for requesting utf8
            // so then we can just directly deal with (without translation) utf8 positions from the lsp.
            // However implementing that has complexities in terms of knowing what lsp is getting our data
            // before we send it, since we typically let the proxy decide.
            // So, currently, we only send/receive UTF16 positions/offsets from the LSP clients
            experimental: Some(json!({
                "serverStatusNotification": true,
            })),
            ..Default::default()
        };

        #[allow(deprecated)]
        let init_params = InitializeParams {
            process_id: Some(process::id()),
            root_uri: root_uri.clone(),
            initialization_options: self.options.clone(),
            capabilities: client_capabilities,
            trace: Some(TraceValue::Verbose),
            workspace_folders: root_uri.map(|uri| {
                vec![WorkspaceFolder {
                    name: uri.as_str().to_string(),
                    uri,
                }]
            }),
            client_info: None,
            root_path: None,
            locale: None,
        };

        let params = Params::from(serde_json::to_value(init_params).unwrap());
        self.send_request("initialize", params, Box::new(on_init));
    }

    pub fn request_document_symbols<CB>(&self, document_uri: Url, cb: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: document_uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/documentSymbol", params, Box::new(cb));
    }

    pub fn request_workspace_symbols<CB>(&self, query: String, cb: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = WorkspaceSymbolParams {
            query,
            ..Default::default()
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("workspace/symbol", params, Box::new(cb));
    }

    pub fn request_document_formatting<CB>(&self, document_uri: Url, cb: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri: document_uri },
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                ..Default::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/formatting", params, Box::new(cb));
    }

    pub fn request_semantic_tokens<CB>(&self, document_uri: Url, cb: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: document_uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/semanticTokens/full", params, Box::new(cb));
    }

    pub fn request_inlay_hints<CB>(&self, document_uri: Url, range: Range, cb: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = InlayHintParams {
            text_document: TextDocumentIdentifier { uri: document_uri },
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/inlayHint", params, Box::new(cb));
    }

    pub fn request_code_actions<CB>(&self, document_uri: Url, range: Range, cb: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri: document_uri },
            range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/codeAction", params, Box::new(cb));
    }

    pub fn request_references<CB>(
        &self,
        document_uri: Url,
        position: Position,
        cb: CB,
    ) where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: document_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: false,
            },
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/references", params, Box::new(cb));
    }

    pub fn request_definition<CB>(
        &self,
        document_uri: Url,
        position: Position,
        cb: CB,
    ) where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: document_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/definition", params, Box::new(cb));
    }

    pub fn request_type_definition<CB>(
        &self,
        document_uri: Url,
        position: Position,
        cb: CB,
    ) where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = GotoTypeDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: document_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/typeDefinition", params, Box::new(cb));
    }

    pub fn request_completion<CB>(
        &self,
        document_uri: Url,
        position: Position,
        on_completion: CB,
    ) where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let completion_params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: document_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        let params = Params::from(serde_json::to_value(completion_params).unwrap());
        self.send_request(
            "textDocument/completion",
            params,
            Box::new(on_completion),
        );
    }

    pub fn completion_resolve<CB>(
        &self,
        completion_item: &CompletionItem,
        on_result: CB,
    ) where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = Params::from(serde_json::to_value(completion_item).unwrap());
        self.send_request("completionItem/resolve", params, Box::new(on_result));
    }

    pub fn request_hover<CB>(&self, document_uri: Url, position: Position, cb: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let hover_params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: document_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let params = Params::from(serde_json::to_value(hover_params).unwrap());
        self.send_request("textDocument/hover", params, Box::new(cb));
    }

    pub fn request_signature<CB>(
        &self,
        document_uri: Url,
        position: Position,
        cb: CB,
    ) where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let params = SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: document_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/signatureHelp", params, Box::new(cb));
    }

    pub fn send_did_change(
        &self,
        buffer: &Buffer,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: u64,
    ) {
        let uri = self.get_uri(buffer);
        let text_document_did_change_params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri,
                version: version as i32,
            },
            content_changes: changes,
        };

        let params = Params::from(
            serde_json::to_value(text_document_did_change_params).unwrap(),
        );
        self.send_notification("textDocument/didChange", params);
    }

    pub fn get_sync_kind(&self) -> Option<TextDocumentSyncKind> {
        let state = self.state.lock();
        let text_document_sync = state
            .server_capabilities
            .as_ref()
            .and_then(|c| c.text_document_sync.as_ref())?;
        match text_document_sync {
            TextDocumentSyncCapability::Kind(kind) => Some(*kind),
            TextDocumentSyncCapability::Options(options) => options.clone().change,
        }
    }

    pub fn update(
        &self,
        buffer: &Buffer,
        content_change: &TextDocumentContentChangeEvent,
        rev: u64,
    ) {
        let sync_kind = self.get_sync_kind().unwrap_or(TextDocumentSyncKind::FULL);
        let changes = get_change_for_sync_kind(sync_kind, buffer, content_change);
        if let Some(changes) = changes {
            self.send_did_change(buffer, changes, rev);
        }
    }
}

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
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

fn prepare_lsp_json(msg: &Value) -> Result<String> {
    let request = serde_json::to_string(&msg)?;
    Ok(format!(
        "Content-Length: {}\r\n\r\n{}",
        request.len(),
        request
    ))
}

fn parse_header(s: &str) -> Result<LspHeader> {
    let split: Vec<String> =
        s.splitn(2, ": ").map(|s| s.trim().to_lowercase()).collect();
    if split.len() != 2 {
        return Err(anyhow!("Malformed"));
    };
    match split[0].as_ref() {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => {
            Ok(LspHeader::ContentLength(split[1].parse::<usize>()?))
        }
        _ => Err(anyhow!("Unknown parse error occurred")),
    }
}

pub fn read_message<T: BufRead>(reader: &mut T) -> Result<String> {
    let mut buffer = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buffer.clear();
        let _result = reader.read_line(&mut buffer);
        // eprin
        match &buffer {
            s if s.trim().is_empty() => break,
            s => {
                match parse_header(s)? {
                    LspHeader::ContentLength(len) => content_length = Some(len),
                    LspHeader::ContentType => (),
                };
            }
        };
    }

    let content_length = content_length
        .ok_or_else(|| anyhow!("missing content-length header: {}", buffer))?;

    let mut body_buffer = vec![0; content_length];
    reader.read_exact(&mut body_buffer)?;

    let body = String::from_utf8(body_buffer)?;
    Ok(body)
}

pub fn get_change_for_sync_kind(
    sync_kind: TextDocumentSyncKind,
    buffer: &Buffer,
    content_change: &TextDocumentContentChangeEvent,
) -> Option<Vec<TextDocumentContentChangeEvent>> {
    match sync_kind {
        TextDocumentSyncKind::NONE => None,
        TextDocumentSyncKind::FULL => {
            let text_document_content_change_event =
                TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: buffer.get_document(),
                };
            Some(vec![text_document_content_change_event])
        }
        TextDocumentSyncKind::INCREMENTAL => Some(vec![content_change.clone()]),
        _ => None,
    }
}

fn format_semantic_styles(
    buffer: &Buffer,
    semantic_tokens_provider: &Option<SemanticTokensServerCapabilities>,
    value: Value,
) -> Option<Vec<LineStyle>> {
    let semantic_tokens: SemanticTokens = serde_json::from_value(value).ok()?;
    let semantic_tokens_provider = semantic_tokens_provider.as_ref()?;
    let semantic_legends = semantic_tokens_legend(semantic_tokens_provider);

    let mut highlights = Vec::new();
    let mut line = 0;
    let mut start = 0;
    let mut last_start = 0;
    for semantic_token in &semantic_tokens.data {
        if semantic_token.delta_line > 0 {
            line += semantic_token.delta_line as usize;
            start = buffer.offset_of_line(line);
        }

        let sub_text = buffer.char_indices_iter(start..);
        if let Some(utf8_delta_start) =
            offset_utf16_to_utf8(sub_text, semantic_token.delta_start as usize)
        {
            start += utf8_delta_start;
        } else {
            // Bad semantic token offsets
            log::error!("Bad semantic token starty {semantic_token:?}");
            break;
        };

        let sub_text = buffer.char_indices_iter(start..);
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
) -> SemanticTokensLegend {
    match semantic_tokens_provider {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => {
            options.legend.clone()
        }
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
            options,
        ) => options.semantic_tokens_options.legend.clone(),
    }
}
