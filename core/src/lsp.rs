use crate::{
    command::LapceUICommand, state::LapceWorkspaceType, state::LAPCE_APP_STATE,
};
use anyhow::{anyhow, Result};
use druid::{WidgetId, WindowId};
use jsonrpc_lite::{Id, JsonRpc, Params};
use lsp_types::SemanticTokensClientCapabilities;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    io::BufRead,
    io::BufReader,
    io::BufWriter,
    io::Write,
    process::Command,
    process::{self, Child, Stdio},
    sync::mpsc::{channel, Receiver},
    sync::Arc,
    thread,
    time::Duration,
};
use xi_rope::RopeDelta;

use lsp_types::*;
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
    window_id: WindowId,
    tab_id: WidgetId,
    clients: HashMap<String, Arc<LspClient>>,
}

impl Drop for LspCatalog {
    fn drop(&mut self) {
        println!("now drop lsp catalog");
    }
}

impl LspCatalog {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> LspCatalog {
        LspCatalog {
            window_id,
            tab_id,
            clients: HashMap::new(),
        }
    }

    pub fn stop(&mut self) {
        for (_, client) in self.clients.iter() {
            client.state.lock().process.kill();
        }
        self.clients.clear();
    }

    pub fn start_server(
        &mut self,
        exec_path: &str,
        language_id: &str,
        options: Option<Value>,
    ) {
        let client = LspClient::new(
            self.window_id,
            self.tab_id,
            language_id.to_string(),
            exec_path,
            options,
        );
        self.clients.insert(language_id.to_string(), client);
    }

    pub fn save_buffer(&self, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.send_did_save(buffer);
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
            client.send_did_open(
                buffer_id,
                document_uri.clone(),
                language_id,
                text.clone(),
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

    pub fn get_completion(
        &self,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.get_completion(request_id, buffer, position);
        }
    }

    pub fn get_semantic_tokens(&self, buffer: &Buffer) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.get_semantic_tokens(buffer);
        }
    }

    pub fn get_code_actions(&self, offset: usize, buffer: &Buffer) {
        // let start_offset = buffer.first_non_blank_character_on_line(line);
        // let end_offset = buffer.offset_of_line(line + 1) - 1;
        let range = Range {
            start: buffer.offset_to_position(offset),
            end: buffer.offset_to_position(offset),
        };
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.get_code_actions(buffer, offset, range.clone());
        }
    }

    pub fn get_document_symbols(
        &self,
        buffer: &Buffer,
    ) -> Option<DocumentSymbolResponse> {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let receiver = { client.get_document_symbols(buffer) };
            let result = receiver.recv_timeout(Duration::from_millis(100));
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
            client.go_to_definition(request_id, buffer, position);
        }
    }

    pub fn get_document_formatting(&self, buffer: &Buffer) -> Option<Vec<TextEdit>> {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let receiver = { client.get_document_formatting(buffer) };
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
            client.document_formatting(buffer);
        }
    }
}

pub trait Callable: Send {
    fn call(self: Box<Self>, client: &LspClient, result: Result<Value>);
}

impl<F: Send + FnOnce(&LspClient, Result<Value>)> Callable for F {
    fn call(self: Box<F>, client: &LspClient, result: Result<Value>) {
        (*self)(client, result)
    }
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
    window_id: WindowId,
    tab_id: WidgetId,
    language_id: String,
    options: Option<Value>,
    state: Arc<Mutex<LspState>>,
}

impl LspClient {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        language_id: String,
        exec_path: &str,
        options: Option<Value>,
    ) -> Arc<LspClient> {
        let workspace_type = LAPCE_APP_STATE
            .get_tab_state(&window_id, &tab_id)
            .workspace
            .lock()
            .kind
            .clone();
        let mut process = match workspace_type {
            LapceWorkspaceType::Local => Command::new(exec_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("Error Occurred"),
            LapceWorkspaceType::RemoteSSH(user, host) => Command::new("ssh")
                .arg(format!("{}@{}", user, host))
                .arg(exec_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("Error Occurred"),
        };

        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));
        let stdout = process.stdout.take().unwrap();

        let lsp_client = Arc::new(LspClient {
            window_id,
            tab_id,
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

    //pub fn new_ssh(
    //    window_id: WindowId,
    //    tab_id: WidgetId,
    //    language_id: String,
    //    exec_path: &str,
    //    options: Option<Value>,
    //    user: &str,
    //    host: &str,
    //) -> Result<Arc<LspClient>> {
    //    let ssh_session = Arc::new(SshSession::new(user, host)?);
    //    let child = ssh_session.run(exec_path)?;
    //    println!("lsp {}", exec_path);
    //    let writer = Box::new(child.stdin());
    //    let reader = child.stdout();
    //    let lsp_client = Arc::new(LspClient {
    //        window_id,
    //        tab_id,
    //        language_id,
    //        state: Arc::new(Mutex::new(LspState {
    //            next_id: 0,
    //            writer,
    //            process: LspProcess::SshSession(ssh_session.clone()),
    //            pending: HashMap::new(),
    //            server_capabilities: None,
    //            opened_documents: HashMap::new(),
    //            is_initialized: false,
    //        })),
    //        options,
    //    });

    //    let local_lsp_client = lsp_client.clone();
    //    //  let reader = ssh_session.get_async_stream(channel.stream(0))?;
    //    thread::spawn(move || {
    //        let mut reader = Box::new(BufReader::new(reader));
    //        loop {
    //            match read_message(&mut reader) {
    //                Ok(message_str) => {
    //                    local_lsp_client.handle_message(message_str.as_ref());
    //                }
    //                Err(err) => {
    //                    //println!("Error occurred {:?}", err);
    //                }
    //            };
    //        }
    //    });

    //    Ok(lsp_client)
    //}

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

    pub fn get_completion(
        &self,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        let uri = self.get_uri(buffer);
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        self.request_completion(uri, position, move |lsp_client, result| {
            if let Ok(res) = result {
                thread::spawn(move || {
                    LAPCE_APP_STATE
                        .get_tab_state(&window_id, &tab_id)
                        .editor_split
                        .lock()
                        .show_completion(request_id, res);
                });
            } else {
                let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
                let mut editor_split = state.editor_split.lock();
                if editor_split.completion.offset == request_id {
                    editor_split.completion.clear();
                }
                println!("request completion error {:?}", result);
            }
        })
    }

    pub fn get_semantic_tokens(&self, buffer: &Buffer) {
        let uri = self.get_uri(buffer);
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        let rev = buffer.rev;
        let buffer_id = buffer.id;
        self.request_semantic_tokens(uri, move |lsp_client, result| {
            if let Ok(res) = result {
                let semantic_tokens_provider = lsp_client
                    .state
                    .lock()
                    .server_capabilities
                    .as_ref()
                    .unwrap()
                    .semantic_tokens_provider
                    .clone();
                thread::spawn(move || {
                    let state = LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
                    let mut editor_split = state.editor_split.lock();
                    let buffer = editor_split.buffers.get_mut(&buffer_id).unwrap();
                    buffer.set_semantic_tokens(semantic_tokens_provider, res);
                });
            }
        })
    }

    pub fn get_code_actions(&self, buffer: &Buffer, offset: usize, range: Range) {
        let uri = self.get_uri(buffer);
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        let rev = buffer.rev;
        let buffer_id = buffer.id;
        self.request_code_actions(uri, range, move |lsp_client, result| {
            if let Ok(res) = result {
                thread::spawn(move || {
                    LAPCE_APP_STATE
                        .get_tab_state(&window_id, &tab_id)
                        .editor_split
                        .lock()
                        .set_code_actions(buffer_id, offset, rev, res);
                });
            }
        })
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
                let window_id = self.window_id;
                let tab_id = self.tab_id;
                thread::spawn(move || {
                    let diagnostics: Result<
                        PublishDiagnosticsParams,
                        serde_json::Error,
                    > = serde_json::from_value(
                        serde_json::to_value(params).unwrap(),
                    );
                    if let Ok(diagnostics) = diagnostics {
                        let state =
                            LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
                        let mut editor_split = state.editor_split.lock();
                        let path = diagnostics.uri.path().to_string();
                        editor_split
                            .diagnostics
                            .insert(path.clone(), diagnostics.diagnostics);

                        LAPCE_APP_STATE.submit_ui_command(
                            LapceUICommand::RequestPaint,
                            state.status_id,
                        );
                        for (_, editor) in editor_split.editors.iter() {
                            if let Some(buffer_id) = editor.buffer_id.as_ref() {
                                if let Some(buffer) =
                                    editor_split.buffers.get(buffer_id)
                                {
                                    if buffer.path == path {
                                        LAPCE_APP_STATE.submit_ui_command(
                                            LapceUICommand::RequestPaint,
                                            editor.view_id,
                                        );
                                    }
                                }
                            }
                        }
                    }
                });
            }
            _ => println!("{} {:?}", method, params),
        }
    }

    pub fn handle_response(&self, id: u64, result: Result<Value>) {
        let callback = self
            .state
            .lock()
            .pending
            .remove(&id)
            .unwrap_or_else(|| panic!("id {} missing from request table", id));
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

    pub fn send_initialized(&self) {
        self.send_notification("initialized", Params::from(json!({})));
    }

    pub fn send_initialize<CB>(&self, root_uri: Option<Url>, on_init: CB)
    where
        CB: 'static + Send + FnOnce(&LspClient, Result<Value>),
    {
        let client_capabilities = ClientCapabilities {
            ..Default::default()
        };

        let init_params = InitializeParams {
            process_id: Some(u32::from(process::id())),
            root_uri,
            initialization_options: self.options.clone(),
            capabilities: client_capabilities,
            trace: Some(TraceOption::Verbose),
            workspace_folders: None,
            client_info: None,
            root_path: None,
            locale: None,
        };

        let params = Params::from(serde_json::to_value(init_params).unwrap());
        self.send_request("initialize", params, Box::new(on_init));
    }

    pub fn get_document_symbols(&self, buffer: &Buffer) -> Receiver<Result<Value>> {
        let uri = self.get_uri(buffer);
        let (sender, receiver) = channel();
        self.request_document_symbols(uri, move |lsp_client, result| {
            let result = sender.send(result);
        });
        return receiver;
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

    pub fn get_document_formatting(
        &self,
        buffer: &Buffer,
    ) -> Receiver<Result<Value>> {
        let uri = self.get_uri(buffer);
        let (sender, receiver) = channel();
        self.request_document_formatting(uri, move |lsp_client, result| {
            sender.send(result);
        });
        return receiver;
    }

    pub fn document_formatting(&self, buffer: &Buffer) {
        let uri = self.get_uri(buffer);
        let rev = buffer.rev;
        let offset = buffer.offset;
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        self.request_document_formatting(uri, move |lsp_client, result| {
            if let Ok(res) = result {
                let edits: Result<Vec<TextEdit>, serde_json::Error> =
                    serde_json::from_value(res);
                if let Ok(edits) = edits {
                    if edits.len() > 0 {
                        thread::spawn(move || {
                            let state =
                                LAPCE_APP_STATE.get_tab_state(&window_id, &tab_id);
                            LAPCE_APP_STATE.submit_ui_command(
                                LapceUICommand::ApplyEdits(offset, rev, edits),
                                state.editor_split.lock().widget_id,
                            );
                        });
                    }
                }
            }
        })
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

    pub fn go_to_definition(
        &self,
        request_id: usize,
        buffer: &Buffer,
        position: Position,
    ) {
        let uri = self.get_uri(buffer);
        let window_id = self.window_id;
        let tab_id = self.tab_id;
        self.request_definition(uri, position, move |lsp_client, result| {
            if let Ok(res) = result {
                thread::spawn(move || {
                    LAPCE_APP_STATE
                        .get_tab_state(&window_id, &tab_id)
                        .editor_split
                        .lock()
                        .go_to_definition(request_id, res);
                });
            }
        })
    }

    pub fn request_definition<CB>(
        &self,
        document_uri: Url,
        position: Position,
        on_definition: CB,
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
        self.send_request(
            "textDocument/definition",
            params,
            Box::new(on_definition),
        );
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
            let workspace_path = LAPCE_APP_STATE
                .get_tab_state(&self.window_id, &self.tab_id)
                .workspace
                .lock()
                .path
                .clone();
            let root_url = Url::from_directory_path(workspace_path).unwrap();
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

    pub fn send_did_save(&self, buffer: &Buffer) {
        let uri = self.get_uri(buffer);
        let params = DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
            text: None,
        };
        let params = Params::from(serde_json::to_value(params).unwrap());
        self.send_notification("textDocument/didSave", params);
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
        match &text_document_sync {
            &TextDocumentSyncCapability::Kind(kind) => Some(kind.clone()),
            &TextDocumentSyncCapability::Options(options) => options.clone().change,
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
