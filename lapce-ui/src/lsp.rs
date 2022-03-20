// TODO: This file is not included in compilation and duplicates `lapce-data::lsp.rs`; Document rationale or remove
use anyhow::{anyhow, Result};
use druid::{WidgetId, WindowId};
use jsonrpc_lite::Id;
use parking_lot::Mutex;
use std::{collections::HashMap, io::BufRead, io::Write, process::Child, sync::Arc};

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
    #[allow(dead_code)]
    window_id: WindowId,

    #[allow(dead_code)]
    tab_id: WidgetId,

    clients: HashMap<String, Arc<LspClient>>,
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
            let _ = client.state.lock().process.kill();
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
    #[allow(dead_code)]
    next_id: u64,

    #[allow(dead_code)]
    writer: Box<dyn Write + Send>,
    process: Child,

    #[allow(dead_code)]
    pending: HashMap<u64, Callback>,

    pub server_capabilities: Option<ServerCapabilities>,
    pub opened_documents: HashMap<BufferId, Url>,
    pub is_initialized: bool,
}

pub struct LspClient {
    #[allow(dead_code)]
    window_id: WindowId,

    #[allow(dead_code)]
    tab_id: WidgetId,

    #[allow(dead_code)]
    language_id: String,

    #[allow(dead_code)]
    options: Option<Value>,

    state: Arc<Mutex<LspState>>,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn number_from_id(id: &Id) -> u64 {
    match *id {
        Id::Num(n) => n as u64,
        Id::Str(ref s) => s
            .parse::<u64>()
            .expect("failed to convert string id to u64"),
        _ => panic!("unexpected value for id: None"),
    }
}
