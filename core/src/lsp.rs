use crate::{
    command::LapceUICommand,
    ssh::{SshSession, SshStream},
    state::LapceWorkspaceType,
    state::LAPCE_APP_STATE,
};
use anyhow::{anyhow, Result};
use druid::{WidgetId, WindowId};
use jsonrpc_lite::{Id, JsonRpc, Params};
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

use lsp_types::{
    ClientCapabilities, CodeActionCapability, CodeActionContext, CodeActionKind,
    CodeActionKindLiteralSupport, CodeActionLiteralSupport, CodeActionParams,
    CodeActionResponse, CompletionCapability, CompletionItemCapability,
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

pub enum LspProcess {
    Child(Child),
    SshSession(SshSession),
}

pub struct LspCatalog {
    window_id: WindowId,
    tab_id: WidgetId,
    clients: HashMap<String, Arc<Mutex<LspClient>>>,
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

    pub fn stop(&self) {
        for (_, client) in self.clients.iter() {
            match &mut client.lock().process {
                LspProcess::Child(child) => {
                    child.kill();
                }
                LspProcess::SshSession(ssh_session) => {
                    ssh_session.session.disconnect(None, "closing down", None);
                }
            }
        }
    }

    pub fn start_server(
        &mut self,
        exec_path: &str,
        language_id: &str,
        options: Option<Value>,
    ) {
        let workspace_type = LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .workspace
            .lock()
            .kind
            .clone();
        match workspace_type {
            LapceWorkspaceType::Local => {
                let client =
                    LspClient::new(self.window_id, self.tab_id, exec_path, options);
                self.clients.insert(language_id.to_string(), client);
            }
            LapceWorkspaceType::RemoteSSH(host) => {
                if let Ok(client) = LspClient::new_ssh(
                    self.window_id,
                    self.tab_id,
                    exec_path,
                    options,
                    &host,
                ) {
                    self.clients.insert(language_id.to_string(), client);
                }
            }
        };
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

    pub fn update(
        &self,
        buffer: &Buffer,
        content_change: &TextDocumentContentChangeEvent,
        rev: u64,
    ) {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client.lock().update(buffer, content_change, rev);
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

    pub fn get_code_actions(&self, offset: usize, buffer: &Buffer) {
        // let start_offset = buffer.first_non_blank_character_on_line(line);
        // let end_offset = buffer.offset_of_line(line + 1) - 1;
        let range = Range {
            start: buffer.offset_to_position(offset),
            end: buffer.offset_to_position(offset),
        };
        if let Some(client) = self.clients.get(&buffer.language_id) {
            client
                .lock()
                .get_code_actions(buffer, offset, range.clone());
        }
    }

    pub fn get_document_symbols(
        &self,
        buffer: &Buffer,
    ) -> Option<DocumentSymbolResponse> {
        if let Some(client) = self.clients.get(&buffer.language_id) {
            let receiver = { client.lock().get_document_symbols(buffer) };
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
    window_id: WindowId,
    tab_id: WidgetId,
    writer: Box<dyn Write + Send>,
    next_id: u64,
    pending: HashMap<u64, Callback>,
    options: Option<Value>,
    process: LspProcess,
    pub server_capabilities: Option<ServerCapabilities>,
    pub opened_documents: HashMap<BufferId, Url>,
    pub is_initialized: bool,
}

impl LspClient {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        exec_path: &str,
        options: Option<Value>,
    ) -> Arc<Mutex<LspClient>> {
        let mut process = Command::new(exec_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Error Occurred");

        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));
        let stdout = process.stdout.take().unwrap();

        let lsp_client = Arc::new(Mutex::new(LspClient {
            window_id,
            tab_id,
            writer,
            next_id: 0,
            process: LspProcess::Child(process),
            pending: HashMap::new(),
            server_capabilities: None,
            opened_documents: HashMap::new(),
            is_initialized: false,
            options,
        }));

        let local_lsp_client = lsp_client.clone();
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stdout));
            loop {
                match read_message(&mut reader) {
                    Ok(message_str) => {
                        local_lsp_client.lock().handle_message(message_str.as_ref());
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

    pub fn new_ssh(
        window_id: WindowId,
        tab_id: WidgetId,
        exec_path: &str,
        options: Option<Value>,
        host: &str,
    ) -> Result<Arc<Mutex<LspClient>>> {
        let mut ssh_session = SshSession::new(host)?;
        let mut channel = ssh_session.get_channel()?;
        ssh_session.channel_exec(&mut channel, exec_path)?;
        println!("lsp {}", exec_path);
        let writer = Box::new(ssh_session.get_stream(&channel));
        let reader = ssh_session.get_stream(&channel);
        let lsp_client = Arc::new(Mutex::new(LspClient {
            window_id,
            tab_id,
            writer,
            next_id: 0,
            pending: HashMap::new(),
            server_capabilities: None,
            process: LspProcess::SshSession(ssh_session),
            opened_documents: HashMap::new(),
            is_initialized: false,
            options,
        }));

        let local_lsp_client = lsp_client.clone();
        //  let reader = ssh_session.get_async_stream(channel.stream(0))?;
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(reader));
            loop {
                match read_message(&mut reader) {
                    Ok(message_str) => {
                        local_lsp_client.lock().handle_message(message_str.as_ref());
                    }
                    Err(err) => {
                        //println!("Error occurred {:?}", err);
                    }
                };
            }
        });

        Ok(lsp_client)
    }

    pub fn update(
        &mut self,
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
            }
        })
    }

    pub fn get_code_actions(
        &mut self,
        buffer: &Buffer,
        offset: usize,
        range: Range,
    ) {
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
            Err(err) => {
                eprintln!("Error in parsing incoming string: {} \n {}", err, message)
            }
        }
    }

    pub fn handle_notification(&mut self, method: &str, params: Params) {
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

    pub fn get_document_symbols(
        &mut self,
        buffer: &Buffer,
    ) -> Receiver<Result<Value>> {
        let uri = self.get_uri(buffer);
        let (sender, receiver) = channel();
        self.request_document_symbols(uri, move |lsp_client, result| {
            let result = sender.send(result);
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
                                LapceUICommand::ApplyEdits(rev, edits),
                                state.editor_split.lock().widget_id,
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

    pub fn request_code_actions<CB>(
        &mut self,
        document_uri: Url,
        range: Range,
        cb: CB,
    ) where
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value>),
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
        let workspace_path = LAPCE_APP_STATE
            .get_tab_state(&self.window_id, &self.tab_id)
            .workspace
            .lock()
            .path
            .clone();
        let root_url = Url::from_directory_path(workspace_path).unwrap();
        if !self.is_initialized {
            self.send_initialize(Some(root_url), move |lsp_client, result| {
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

    pub fn get_sync_kind(&self) -> Option<TextDocumentSyncKind> {
        let text_document_sync = self
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
