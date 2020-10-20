use anyhow::Result;
use jsonrpc_lite::{Error as JsonRpcError, Id, JsonRpc, Params};
use languageserver_types::*;
use lapce_core::buffer::BufferId;
use parking_lot::Mutex;
use serde_json::{json, to_value, Value};
use std::{
    collections::HashMap,
    io::BufRead,
    io::BufReader,
    io::BufWriter,
    io::Write,
    process::Command,
    process::{self, Stdio},
    sync::Arc,
    thread,
};

pub trait Callable: Send {
    fn call(
        self: Box<Self>,
        client: &mut LspClient,
        result: Result<Value, JsonRpcError>,
    );
}

impl<F: Send + FnOnce(&mut LspClient, Result<Value, JsonRpcError>)> Callable for F {
    fn call(
        self: Box<F>,
        client: &mut LspClient,
        result: Result<Value, JsonRpcError>,
    ) {
        (*self)(client, result)
    }
}

pub type Callback = Box<dyn Callable>;

pub struct LspClient {
    writer: Box<dyn Write + Send>,
    next_id: u64,
    pending: HashMap<u64, Callback>,
    pub server_capabilities: Option<ServerCapabilities>,
    pub opened_documents: HashMap<BufferId, Url>,
    pub is_initialized: bool,
}

impl LspClient {
    pub fn new() -> Arc<Mutex<LspClient>> {
        let mut process = Command::new("rust-analyzer-mac")
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

    pub fn handle_message(&mut self, message: &str) {
        match JsonRpc::parse(message) {
            Ok(JsonRpc::Request(obj)) => {
                // trace!("client received unexpected request: {:?}", obj)
            }
            Ok(value @ JsonRpc::Notification(_)) => {
                // self.handle_notification(
                //     value.get_method().unwrap(),
                //     value.get_params().unwrap(),
                // );
            }
            Ok(value @ JsonRpc::Success(_)) => {
                let id = number_from_id(&value.get_id().unwrap());
                let result = value.get_result().unwrap();
                self.handle_response(id, Ok(result.clone()));
            }
            Ok(value @ JsonRpc::Error(_)) => {
                let id = number_from_id(&value.get_id().unwrap());
                let error = value.get_error().unwrap();
                self.handle_response(id, Err(error.clone()));
            }
            Err(err) => eprintln!("Error in parsing incoming string: {}", err),
        }
    }

    pub fn handle_response(
        &mut self,
        id: u64,
        result: Result<Value, jsonrpc_lite::Error>,
    ) {
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
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value, JsonRpcError>),
    {
        let client_capabilities = ClientCapabilities::default();

        let init_params = InitializeParams {
            process_id: Some(u64::from(process::id())),
            root_uri,
            root_path: None,
            initialization_options: None,
            capabilities: client_capabilities,
            trace: Some(TraceOption::Verbose),
            workspace_folders: None,
        };

        eprintln!("send initilize");
        let params = Params::from(serde_json::to_value(init_params).unwrap());
        self.send_request("initialize", params, Box::new(on_init));
    }

    pub fn request_completion<CB>(
        &mut self,
        document_uri: Url,
        position: Position,
        on_completion: CB,
    ) where
        CB: 'static + Send + FnOnce(&mut LspClient, Result<Value, JsonRpcError>),
    {
        let completion_params = CompletionParams {
            text_document: TextDocumentIdentifier { uri: document_uri },
            position,
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
        document_text: String,
    ) {
        self.opened_documents
            .insert(buffer_id.clone(), document_uri.clone());
        eprintln!("open docuemnts insert {:?}", buffer_id);

        let text_document_did_open_params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                language_id: "rust".to_string(),
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

    pub fn send_did_change(
        &mut self,
        buffer_id: &BufferId,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: u64,
    ) {
        eprintln!("send did change {:?}", buffer_id);
        let uri = self.opened_documents.get(buffer_id).unwrap().clone();
        let text_document_did_change_params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri,
                version: Some(version),
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

fn prepare_lsp_json(msg: &Value) -> Result<String, serde_json::error::Error> {
    let request = serde_json::to_string(&msg)?;
    Ok(format!(
        "Content-Length: {}\r\n\r\n{}",
        request.len(),
        request
    ))
}

const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

/// Type to represent errors occurred while parsing LSP RPCs
#[derive(Debug)]
pub enum ParseError {
    Io(std::io::Error),
    ParseInt(std::num::ParseIntError),
    Utf8(std::string::FromUtf8Error),
    Json(serde_json::Error),
    Unknown(String),
}

impl From<std::io::Error> for ParseError {
    fn from(err: std::io::Error) -> ParseError {
        ParseError::Io(err)
    }
}

impl From<std::string::FromUtf8Error> for ParseError {
    fn from(err: std::string::FromUtf8Error) -> ParseError {
        ParseError::Utf8(err)
    }
}

impl From<serde_json::Error> for ParseError {
    fn from(err: serde_json::Error) -> ParseError {
        ParseError::Json(err)
    }
}

impl From<std::num::ParseIntError> for ParseError {
    fn from(err: std::num::ParseIntError) -> ParseError {
        ParseError::ParseInt(err)
    }
}

impl From<String> for ParseError {
    fn from(s: String) -> ParseError {
        ParseError::Unknown(s)
    }
}

/// parse header from the incoming input string
fn parse_header(s: &str) -> Result<LspHeader, ParseError> {
    let split: Vec<String> =
        s.splitn(2, ": ").map(|s| s.trim().to_lowercase()).collect();
    if split.len() != 2 {
        return Err(ParseError::Unknown("Malformed".to_string()));
    };
    match split[0].as_ref() {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => Ok(LspHeader::ContentLength(
            usize::from_str_radix(&split[1], 10)?,
        )),
        _ => Err(ParseError::Unknown(
            "Unknown parse error occurred".to_string(),
        )),
    }
}

/// Blocking call to read a message from the provided BufRead
pub fn read_message<T: BufRead>(reader: &mut T) -> Result<String, ParseError> {
    let mut buffer = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buffer.clear();
        let _result = reader.read_line(&mut buffer);

        // eprintln!("got message {} {}", buffer, buffer.trim().is_empty());
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
        .ok_or_else(|| format!("missing content-length header: {}", buffer))?;

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
