use anyhow::{anyhow, Result};
use druid::{WidgetId, WindowId};
use jsonrpc_lite::Id;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    io::BufRead,
    io::Write,
    process::Child,
    sync::Arc,
};

use lsp_types::*;
use serde_json::Value;

use crate::buffer::BufferId;

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
