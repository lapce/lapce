use std::{
    collections::HashMap,
    io::BufRead,
    io::{BufReader, BufWriter, Write},
    process::{self, Child, Command, Stdio},
    sync::{mpsc::channel, Arc},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use jsonrpc_lite::{Id, JsonRpc, Params};
use lapce_rpc::RequestId;
use lsp_types::*;
use parking_lot::Mutex;
use serde_json::{json, to_value, Value};

use crate::buffer::Buffer;
use crate::buffer::BufferId;
use crate::dispatch::Dispatcher;
use crate::dispatch::Request;

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
}

pub struct LspClient {
    language_id: String,
    options: Option<Value>,
    state: Arc<Mutex<LspState>>,
    dispatcher: Dispatcher,
}

impl LspCatalog {
    pub fn new() -> LspCatalog {
        LspCatalog {
            dispatcher: None,
            clients: HashMap::new(),
        }
    }

    pub fn start_server(
        &mut self,
        exec_path: &str,
        language_id: &str,
        options: Option<Value>,
    ) {
        let client = LspClient::new(
            language_id.to_string(),
            exec_path,
            options,
            self.dispatcher.clone().unwrap(),
        );
        self.clients.insert(language_id.to_string(), client);
    }

    pub fn new_buffer(
        &self,
        buffer_id: &BufferId,
        path: &str,
        language_id: &str,
        text: String,
    ) {
        let document_uri = Url::from_file_path(path).unwrap();
        if let Some(client) = self.clients.get(language_id) {
            client.send_did_open(
                buffer_id,
                document_uri.clone(),
                language_id,
                text.clone(),
            );
        }
    }

    pub fn save_buffer(&self, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let uri = client.get_uri(buffer);
            client.send_did_save(uri);
        }
    }

    pub fn get_semantic_tokens(&self, buffer: &Buffer) {
        let buffer_id = buffer.id;
        let rev = buffer.rev;
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let uri = client.get_uri(buffer);
            let local_dispatcher = self.dispatcher.clone().unwrap();
            client.request_semantic_tokens(uri, move |lsp_client, result| {
                if let Ok(res) = result {
                    let buffers = local_dispatcher.buffers.lock();
                    let buffer = buffers.get(&buffer_id).unwrap();
                    if buffer.rev != rev {
                        return;
                    }
                    let lsp_state = lsp_client.state.lock();
                    let semantic_tokens_provider = &lsp_state
                        .server_capabilities
                        .as_ref()
                        .unwrap()
                        .semantic_tokens_provider;
                    if let Some(tokens) =
                        format_semantic_tokens(buffer, semantic_tokens_provider, res)
                    {
                        local_dispatcher.send_notification(
                            "semantic_tokens",
                            json!({
                                "rev": rev,
                                "buffer_id": buffer_id,
                                "tokens": tokens,
                            }),
                        )
                    }
                }
            });
        }
    }

    pub fn get_document_symbols(&self, id: RequestId, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let uri = client.get_uri(buffer);
            client.request_document_symbols(uri, move |lsp_client, result| {
                lsp_client.dispatcher.respond(id, result);
            });
        }
    }

    pub fn get_document_formatting(&self, id: RequestId, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let uri = client.get_uri(buffer);
            client.request_document_formatting(uri, move |lsp_client, result| {
                lsp_client.dispatcher.respond(id, result);
            });
        }
    }

    pub fn get_completion(
        &self,
        id: RequestId,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
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
                lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_signature(&self, id: RequestId, buffer: &Buffer, position: Position) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
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
                lsp_client.dispatcher.sender.send(resp);
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
                lsp_client.dispatcher.sender.send(resp);
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
                lsp_client.dispatcher.sender.send(resp);
            });
        }
    }

    pub fn get_definition(
        &self,
        id: RequestId,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
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
                lsp_client.dispatcher.sender.send(resp);
            });
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

impl LspClient {
    pub fn new(
        language_id: String,
        exec_path: &str,
        options: Option<Value>,
        dispatcher: Dispatcher,
    ) -> Arc<LspClient> {
        let mut process = Command::new(exec_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Error Occurred");

        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));
        let stdout = process.stdout.take().unwrap();

        let lsp_client = Arc::new(LspClient {
            dispatcher,
            language_id,
            options,
            state: Arc::new(Mutex::new(LspState {
                next_id: 0,
                writer,
                process,
                pending: HashMap::new(),
                server_capabilities: None,
                opened_documents: HashMap::new(),
                is_initialized: false,
            })),
        });

        let local_lsp_client = lsp_client.clone();
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stdout));
            loop {
                match read_message(&mut reader) {
                    Ok(message_str) => {
                        local_lsp_client.handle_message(message_str.as_ref());
                    }
                    Err(err) => {
                        eprintln!("lsp read Error occurred {:?}", err);
                        return;
                    }
                };
            }
        });

        lsp_client
    }

    pub fn get_uri(&self, buffer: &Buffer) -> Url {
        let exits = {
            let state = self.state.lock();
            state.opened_documents.contains_key(&buffer.id)
        };
        if !exits {
            let document_uri = Url::from_file_path(&buffer.path).unwrap();
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
        match JsonRpc::parse(message) {
            Ok(JsonRpc::Request(obj)) => {
                // trace!("client received unexpected request: {:?}", obj)
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
            Err(err) => {
                eprintln!("Error in parsing incoming string: {} \n {}", err, message)
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
            _ => eprintln!("{} {:?}", method, params),
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

    pub fn write(&self, msg: &str) {
        let mut state = self.state.lock();
        state
            .writer
            .write_all(msg.as_bytes())
            .expect("error writing to stdin");
        state.writer.flush().expect("error flushing child stdin");
    }

    fn send_rpc(&self, value: &Value) {
        let rpc = match prepare_lsp_json(value) {
            Ok(r) => r,
            Err(err) => panic!("Encoding Error {:?}", err),
        };

        self.write(rpc.as_ref());
    }

    pub fn send_notification(&self, method: &str, params: Params) {
        let notification = JsonRpc::notification_with_params(method, params);
        let res = to_value(&notification).unwrap();
        self.send_rpc(&res);
    }

    pub fn send_request(&self, method: &str, params: Params, completion: Callback) {
        let request = {
            let mut state = self.state.lock();
            let next_id = state.next_id;
            state.pending.insert(next_id, completion);
            state.next_id += 1;

            JsonRpc::request_with_params(Id::Num(next_id as i64), method, params)
        };

        self.send_rpc(&to_value(&request).unwrap());
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
                .insert(buffer_id.clone(), document_uri.clone());
            state.is_initialized
        };

        if !is_initialized {
            let root_url =
                Url::from_directory_path(self.dispatcher.workspace.lock().clone())
                    .unwrap();
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
                sender.send(true);
            });
            receiver.recv_timeout(Duration::from_millis(1000));
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

    pub fn send_did_save(&self, uri: Url) {
        let params = DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
            text: None,
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
                completion: Some(CompletionCapability {
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: Some(false),
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
                code_action: Some(CodeActionCapability {
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
                semantic_highlighting_capabilities: Some(
                    SemanticHighlightingClientCapability {
                        semantic_highlighting: true,
                    },
                ),
                ..Default::default()
            }),
            ..Default::default()
        };

        let init_params = InitializeParams {
            process_id: Some(u64::from(process::id())),
            root_uri,
            initialization_options: self.options.clone(),
            capabilities: client_capabilities,
            trace: Some(TraceOption::Verbose),
            workspace_folders: None,
            client_info: None,
            root_path: None,
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
                include_declaration: true,
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
                version: Some(version as i64),
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
        match &text_document_sync {
            &TextDocumentSyncCapability::Kind(kind) => Some(kind.clone()),
            &TextDocumentSyncCapability::Options(options) => options.clone().change,
        }
    }

    pub fn update(
        &self,
        buffer: &Buffer,
        content_change: &TextDocumentContentChangeEvent,
        rev: u64,
    ) {
        let sync_kind = self.get_sync_kind().unwrap_or(TextDocumentSyncKind::Full);
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
        Id::Str(ref s) => {
            u64::from_str_radix(s, 10).expect("failed to convert string id to u64")
        }
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
        HEADER_CONTENT_LENGTH => Ok(LspHeader::ContentLength(
            usize::from_str_radix(&split[1], 10)?,
        )),
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
        TextDocumentSyncKind::None => None,
        TextDocumentSyncKind::Full => {
            let text_document_content_change_event =
                TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: buffer.get_document(),
                };
            Some(vec![text_document_content_change_event])
        }
        TextDocumentSyncKind::Incremental => Some(vec![content_change.clone()]),
    }
}

fn format_semantic_tokens(
    buffer: &Buffer,
    semantic_tokens_provider: &Option<SemanticTokensServerCapabilities>,
    value: Value,
) -> Option<Vec<(usize, usize, String)>> {
    let semantic_tokens: SemanticTokens = serde_json::from_value(value).ok()?;
    let semantic_tokens_provider = semantic_tokens_provider.as_ref()?;
    let semantic_lengends = semantic_tokens_lengend(semantic_tokens_provider);

    let mut highlights = Vec::new();
    let mut line = 0;
    let mut start = 0;
    for semantic_token in &semantic_tokens.data {
        if semantic_token.delta_line > 0 {
            line += semantic_token.delta_line as usize;
            start = buffer.offset_of_line(line);
        }
        start += semantic_token.delta_start as usize;
        let end = start + semantic_token.length as usize;
        let kind = semantic_lengends.token_types[semantic_token.token_type as usize]
            .as_str()
            .to_string();
        highlights.push((start, end, kind));
    }

    Some(highlights)
}

fn semantic_tokens_lengend(
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
