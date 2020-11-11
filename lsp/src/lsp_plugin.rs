use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, collections::VecDeque, sync::Arc};

use languageserver_types::{
    request::Completion, Hover, HoverContents, InitializeResult, MarkedString,
    Position, Range, TextDocumentContentChangeEvent, TextDocumentSyncKind, Url,
};
use parking_lot::Mutex;
use xi_rope::RopeDelta;

use crate::{plugin::CoreProxy, plugin::Plugin};

#[derive(Debug)]
pub enum LspResponse {
    Hover(Result<Hover>),
    Completion(Value),
}

#[derive(Clone, Debug, Default)]
pub struct ResultQueue(Arc<Mutex<VecDeque<(usize, LspResponse)>>>);

impl ResultQueue {
    pub fn new() -> Self {
        ResultQueue(Arc::new(Mutex::new(VecDeque::new())))
    }

    pub fn push_result(&mut self, request_id: usize, response: LspResponse) {
        let mut queue = self.0.lock();
        queue.push_back((request_id, response));
    }

    pub fn pop_result(&mut self) -> Option<(usize, LspResponse)> {
        let mut queue = self.0.lock();
        queue.pop_front()
    }
}

pub struct LspPlugin {
    core: Option<CoreProxy>,
    result_queue: ResultQueue,
}

impl LspPlugin {
    pub fn new() -> LspPlugin {
        LspPlugin {
            core: None,
            result_queue: ResultQueue::new(),
        }
    }
}

impl Plugin for LspPlugin {
    fn initialize(&mut self, core: CoreProxy) {
        self.core = Some(core);
        self.core
            .as_mut()
            .unwrap()
            .start_lsp_server("gopls", "go", None);
        self.core.as_mut().unwrap().start_lsp_server(
            "rust-analyzer-mac",
            "rust",
            Some(json!({
                "diagnostics.enable": false,
                "diagnostics.enableExperimental": false,
            })),
        );
    }

    //    fn new_buffer(&mut self, buffer: &mut Buffer) {}
    //
    //    fn update(&mut self, buffer: &mut Buffer, delta: &RopeDelta, rev: u64) {}
    //
    //    fn idle(&mut self, buffer: &mut Buffer) {}
}
