use crate::{command::LapceUICommand, state::LAPCE_STATE};
use anyhow::{anyhow, Result};
use jsonrpc_lite::{Id, JsonRpc, Params};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    io::BufRead,
    io::BufReader,
    io::BufWriter,
    io::Write,
    process::Command,
    process::{self, Stdio},
    sync::mpsc::{channel, Receiver},
    sync::Arc,
    thread,
    time::Duration,
};
use xi_rope::RopeDelta;

use lsp_types::{
    ClientCapabilities, CompletionCapability, CompletionItemCapability,
    CompletionParams, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, FormattingOptions, GotoDefinitionParams,
    InitializeParams, InitializeResult, PartialResultParams, Position,
    PublishDiagnosticsParams, Range, ServerCapabilities,
    TextDocumentClientCapabilities, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, TraceOption, Url,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};
use serde_json::{json, to_value, Value};

use crate::buffer::{Buffer, BufferId};

pub type Callback = Box<dyn Callable>;
const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

pub struct LspCatalog {
    clients: HashMap<String, Arc<Mutex<LspClient>>>,
}

impl LspCatalog {
    pub fn new() -> LspCatalog {
        LspCatalog {
            clients: HashMap::new(),
        }
    }

    pub fn start_server(&mut self, exec_path: &str, language_id: &str) {
        let client = LspClient::new(exec_path);
        self.clients.insert(language_id.to_string(), client);
    }

    pub fn save_buffer(&self, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.lock().send_did_save(buffer);
        }
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
            client.lock().send_did_open(
                buffer_id,
                document_uri.clone(),
                language_id,
                text.clone(),
            );
        }
    }

    pub fn update(&self, buffer: &Buffer, delta: &RopeDelta, rev: u64) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.lock().update(buffer, delta, rev);
        }
    }

    pub fn get_completion(
        &self,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.lock().get_completion(request_id, buffer, position);
        }
    }

    pub fn get_document_symbols(
        &self,
        buffer: &Buffer,
    ) -> Option<DocumentSymbolResponse> {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let receiver = { client.lock().get_document_symbols(buffer) };
            let result = receiver.recv_timeout(Duration::from_millis(100));
            println!("recv {:?}", result);
            let result = result.ok()?;
            let value = result.ok()?;
            let resp: DocumentSymbolResponse = serde_json::from_value(value).ok()?;
            return Some(resp);
        }
        None
    }

    pub fn go_to_definition(
        &self,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.lock().go_to_definition(request_id, buffer, position);
        }
    }

    pub fn get_document_formatting(&self, buffer: &Buffer) -> Option<Vec<TextEdit>> {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let receiver = { client.lock().get_document_formatting(buffer) };
            let result = receiver.recv_timeout(Duration::from_millis(100));
            let result = result.ok()?;
            let value = result.ok()?;
            let resp: Vec<TextEdit> = serde_json::from_value(value).ok()?;
            return Some(resp);
        }
        None
    }

    pub fn request_document_formatting(&self, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.lock().document_formatting(buffer);
        }
    }
}

pub trait Callable: Send {
    fn call(self: Box<Self>, client: &mut LspClient, result: Result<Value>);
}

impl<F: Send + FnOnce(&mut LspClient, Result<Value>)> Callable for F {
    fn call(self: Box<F>, client: &mut LspClient, result: Result<Value>) {
        (*self)(client, result)
    }
}

pub struct LspClient {
    writer: Box<dyn Write + Send>,
    next_id: u64,
    pending: HashMap<u64, Callback>,
    pub server_capabilities: Option<ServerCapabilities>,
    pub opened_documents: HashMap<BufferId, Url>,
    pub is_initialized: bool,
}

impl LspClient {
    pub fn new(exec_path: &str) -> Arc<Mutex<LspClient>> {
        let mut process = Command::new(exec_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Error Occurred");

        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));

        let lsp_client = Arc::new(Mutex::new(LspClient {
            writer,
            next_id: 0,
            pending: HashMap::new(),
            server_capabilities: None,
            opened_documents: HashMap::new(),
            is_initialized: false,
        }));

        let local_lsp_client = lsp_client.clone();
        let mut stdout = process.stdout;
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stdout.take().unwrap()));
            loop {
                match read_message(&mut reader) {
                    Ok(message_str) => {
                        local_lsp_client.lock().handle_message(message_str.as_ref());
                    }
                    Err(err) => {
                        // eprintln!("Error occurred {:?}", err);
                    }
                };
            }
        });

        lsp_client
    }

    pub fn update(&mut self, buffer: &Buffer, delta: &RopeDelta, rev: u64) {
        let sync_kind = self.get_sync_kind();
        if let Some(changes) = get_change_for_sync_kind(sync_kind, buffer, delta) {
            self.send_did_change(buffer, changes, rev);
        }
    }

    pub fn get_uri(&mut self, buffer: &Buffer) -> Url {
        if !self.opened_documents.contains_key(&buffer.id) {
            let document_uri = Url::from_file_path(&buffer.path).unwrap();
            self.send_did_open(
                &buffer.id,
                document_uri,
                &buffer.language_id,
                buffer.get_document(),
            );
        }
        self.opened_documents.get(&buffer.id).unwrap().clone()
    }

    pub fn get_completion(
        &mut self,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        let uri = self.get_uri(buffer);
        self.request_completion(uri, position, move |lsp_client, result| {
            if let Ok(res) = result {
                thread::spawn(move || {
                    LAPCE_STATE
                        .editor_split
                        .lock()
                        .show_completion(request_id, res);
                });
            }
        })
    }

    pub fn handle_message(&mut self, message: &str) {
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
            Err(err) => eprintln!("Error in parsing incoming string: {}", err),
        }
    }

    pub fn handle_notification(&mut self, method: &str, params: Params) {
        match method {
            "textDocument/publishDiagnostics" => {
                thread::spawn(move || {
                    let diagnostics: Result<
                        PublishDiagnosticsParams,
                        serde_json::Error,
                    > = serde_json::from_value(
                        serde_json::to_value(params).unwrap(),
                    );
                    if let Ok(diagnostics) = diagnostics {
                        let mut editor_split = LAPCE_STATE.editor_split.lock();
                        editor_split.diagnostics.insert(
                            diagnostics.uri.path().to_string(),
                            diagnostics.diagnostics,
                        );
                        LAPCE_STATE.request_paint();
                    }
                });
            }
            _ => (),
        }
    }

    pub fn handle_response(&mut self, id: u64, result: Result<Value>) {
        let callback = self
            .pending
            .remove(&id)
            .unwrap_or_else(|| panic!("id {} missing from request table", id));
        callback.call(self, result);
    }

    pub fn write(&mut self, msg: &str) {
        self.writer
            .write_all(msg.as_bytes())
            .expect("error writing to stdin");

        self.writer.flush().expect("error flushing child stdin");
    }

    pub fn send_request(
        &mut self,
        method: &str,
        params: Params,
        completion: Callback,
    ) {
        let request = JsonRpc::request_with_params(
            Id::Num(self.next_id as i64),
            method,
            params,
        );

        self.pending.insert(self.next_id, completion);
        self.next_id += 1;

        self.send_rpc(&to_value(&request).unwrap());
    }

    fn send_rpc(&mut self, value: &Value) {
        let rpc = match prepare_lsp_json(value) {
            Ok(r) => r,
            Err(err) => panic!("Encoding Error {:?}", err),
        };

        self.write(rpc.as_ref());
    }

    pub fn send_notification(&mut self, method: &str, params: Params) {
        let notification = JsonRpc::notification_with_params(method, params);
        let res = to_value(&notification).unwrap();
        self.send_rpc(&res);
    }

    pub fn send_initialized(&mut self) {
        self.send_notification("initialized", Params::from(json!({})));
    }

    pub fn send_initialize<CB>(&mut self, root_uri: Option<Url>, on_init: CB)
    where
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value>),
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
                ..Default::default()
            }),
            ..Default::default()
        };

        let init_params = InitializeParams {
            process_id: Some(u64::from(process::id())),
            root_uri,
            initialization_options: None,
            capabilities: client_capabilities,
            trace: Some(TraceOption::Verbose),
            workspace_folders: None,
            client_info: None,
            root_path: None,
        };

        let params = Params::from(serde_json::to_value(init_params).unwrap());
        self.send_request("initialize", params, Box::new(on_init));
    }

    pub fn get_document_symbols(
        &mut self,
        buffer: &Buffer,
    ) -> Receiver<Result<Value>> {
        let uri = self.get_uri(buffer);
        let (sender, receiver) = channel();
        self.request_document_symbols(uri, move |lsp_client, result| {
            let result = sender.send(result);
            println!("send {:?}", result);
        });
        return receiver;
    }

    pub fn request_document_symbols<CB>(&mut self, document_uri: Url, cb: CB)
    where
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value>),
    {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: document_uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_request("textDocument/documentSymbol", params, Box::new(cb));
    }

    pub fn get_document_formatting(
        &mut self,
        buffer: &Buffer,
    ) -> Receiver<Result<Value>> {
        let uri = self.get_uri(buffer);
        let (sender, receiver) = channel();
        self.request_document_formatting(uri, move |lsp_client, result| {
            sender.send(result);
        });
        return receiver;
    }

    pub fn document_formatting(&mut self, buffer: &Buffer) {
        let uri = self.get_uri(buffer);
        let rev = buffer.rev;
        self.request_document_formatting(uri, move |lsp_client, result| {
            if let Ok(res) = result {
                let edits: Result<Vec<TextEdit>, serde_json::Error> =
                    serde_json::from_value(res);
                if let Ok(edits) = edits {
                    if edits.len() > 0 {
                        thread::spawn(move || {
                            LAPCE_STATE.submit_ui_command(
                                LapceUICommand::ApplyEdits(rev, edits),
                                LAPCE_STATE.editor_split.lock().widget_id,
                            );
                        });
                    }
                }
            }
        })
    }

    pub fn request_document_formatting<CB>(&mut self, document_uri: Url, cb: CB)
    where
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value>),
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

    pub fn go_to_definition(
        &mut self,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        let uri = self.get_uri(buffer);
        self.request_definition(uri, position, move |lsp_client, result| {
            if let Ok(res) = result {
                thread::spawn(move || {
                    LAPCE_STATE
                        .editor_split
                        .lock()
                        .go_to_definition(request_id, res);
                });
            }
        })
    }

    pub fn request_definition<CB>(
        &mut self,
        document_uri: Url,
        position: Position,
        on_definition: CB,
    ) where
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value>),
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
        self.send_request(
            "textDocument/definition",
            params,
            Box::new(on_definition),
        );
    }

    pub fn request_completion<CB>(
        &mut self,
        document_uri: Url,
        position: Position,
        on_completion: CB,
    ) where
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value>),
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

    pub fn send_did_open(
        &mut self,
        buffer_id: &BufferId,
        document_uri: Url,
        language_id: &str,
        document_text: String,
    ) {
        self.opened_documents
            .insert(buffer_id.clone(), document_uri.clone());

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
        if !self.is_initialized {
            self.send_initialize(None, move |lsp_client, result| {
                if let Ok(result) = result {
                    let init_result: InitializeResult =
                        serde_json::from_value(result).unwrap();

                    lsp_client.server_capabilities = Some(init_result.capabilities);
                    lsp_client.is_initialized = true;
                    lsp_client.send_initialized();
                    lsp_client.send_notification("textDocument/didOpen", params);
                }
            });
        } else {
            self.send_notification("textDocument/didOpen", params);
        }
    }

    pub fn send_did_save(&mut self, buffer: &Buffer) {
        let uri = self.get_uri(buffer);
        let params = DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
            text: None,
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_notification("textDocument/didSave", params);
    }

    pub fn send_did_change(
        &mut self,
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

    pub fn get_sync_kind(&mut self) -> TextDocumentSyncKind {
        match self
            .server_capabilities
            .as_ref()
            .and_then(|c| c.text_document_sync.as_ref())
        {
            Some(&TextDocumentSyncCapability::Kind(kind)) => kind,
            _ => TextDocumentSyncKind::Full,
        }
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

fn number_from_id(id: &Id) -> u64 {
    match *id {
        Id::Num(n) => n as u64,
        Id::Str(ref s) => {
            u64::from_str_radix(s, 10).expect("failed to convert string id to u64")
        }
        _ => panic!("unexpected value for id: None"),
    }
}

pub fn get_change_for_sync_kind(
    sync_kind: TextDocumentSyncKind,
    buffer: &Buffer,
    delta: &RopeDelta,
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
        TextDocumentSyncKind::Incremental => {
            match get_document_content_changes(delta, buffer) {
                Ok(result) => Some(result),
                Err(err) => {
                    let text_document_content_change_event =
                        TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: buffer.get_document(),
                        };
                    Some(vec![text_document_content_change_event])
                }
            }
        }
    }
}

pub fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &Buffer,
) -> Result<Vec<TextDocumentContentChangeEvent>> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let text = String::from(node);

        let (start, end) = interval.start_end();
        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: buffer.offset_to_position(end),
            }),
            range_length: Some((end - start) as u64),
            text,
        };

        return Ok(vec![text_document_content_change_event]);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let mut end_position = buffer.offset_to_position(end);

        // Hack around sending VSCode Style Positions to Language Server.
        // See this issue to understand: https://github.com/Microsoft/vscode/issues/23173
        if end_position.character == 0 {
            // There is an assumption here that the line separator character is exactly
            // 1 byte wide which is true for "\n" but it will be an issue if they are not
            // for example for u+2028
            let mut ep = buffer.offset_to_position(end - 1);
            ep.character += 1;
            end_position = ep;
        }

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: end_position,
            }),
            range_length: Some((end - start) as u64),
            text: String::new(),
        };

        return Ok(vec![text_document_content_change_event]);
    }

    let text_document_content_change_event = TextDocumentContentChangeEvent {
        range: None,
        range_length: None,
        text: buffer.get_document(),
    };

    Ok(vec![text_document_content_change_event])
}
