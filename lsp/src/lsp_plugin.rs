use anyhow::{anyhow, Result};
use serde_json::Value;
use std::{collections::HashMap, collections::VecDeque, sync::Arc};

use languageserver_types::{
    request::Completion, Hover, HoverContents, InitializeResult, MarkedString,
    Position, Range, TextDocumentContentChangeEvent, TextDocumentSyncKind, Url,
};
use lapce_core::plugin::Hover as CoreHover;
use lapce_core::plugin::PluginBufferInfo;
use lapce_core::plugin::Range as CoreRange;
use parking_lot::Mutex;
use xi_rope::RopeDelta;

use crate::{
    buffer::Buffer, lsp_client::LspClient, plugin::CoreProxy, plugin::Plugin,
};

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
    lsp_clients: HashMap<String, Arc<Mutex<LspClient>>>,
    result_queue: ResultQueue,
}

impl LspPlugin {
    pub fn new() -> LspPlugin {
        LspPlugin {
            core: None,
            lsp_clients: HashMap::new(),
            result_queue: ResultQueue::new(),
        }
    }
}

impl Plugin for LspPlugin {
    fn initialize(&mut self, core: CoreProxy) {
        self.core = Some(core);
    }

    fn new_buffer(&mut self, buffer: &mut Buffer) {
        if !self.lsp_clients.contains_key(&buffer.language_id) {
            let lsp_client = LspClient::new();
            self.lsp_clients
                .insert(buffer.language_id.clone(), lsp_client);
        }
        let mut lsp_client =
            self.lsp_clients.get(&buffer.language_id).unwrap().lock();
        let buffer_id = buffer.buffer_id.clone();
        let document_uri = Url::from_file_path(&buffer.path).unwrap();
        let document_text = buffer.get_document().unwrap_or("".to_string());
        if !lsp_client.is_initialized {
            lsp_client.send_initialize(None, move |lsp_client, result| {
                if let Ok(result) = result {
                    eprintln!("lsp initilize got result");
                    let init_result: InitializeResult =
                        serde_json::from_value(result).unwrap();

                    lsp_client.server_capabilities = Some(init_result.capabilities);
                    lsp_client.is_initialized = true;
                    lsp_client.send_initialized();
                    lsp_client.send_did_open(
                        &buffer_id,
                        document_uri,
                        document_text,
                    );
                } else {
                    eprintln!("lsp initilize error {}", result.err().unwrap());
                }
            });
        }
        eprintln!("got new buffer");
    }

    fn update(&mut self, buffer: &mut Buffer, delta: &RopeDelta, rev: u64) {
        let mut lsp_client =
            self.lsp_clients.get(&buffer.language_id).unwrap().lock();
        let sync_kind = lsp_client.get_sync_kind();
        if let Some(changes) = get_change_for_sync_kind(sync_kind, buffer, delta) {
            lsp_client.send_did_change(&buffer.buffer_id, changes, rev);
        }
    }

    fn get_completion(
        &mut self,
        buffer: &mut Buffer,
        request_id: usize,
        offset: usize,
    ) {
        let mut lsp_client =
            self.lsp_clients.get(&buffer.language_id).unwrap().lock();
        let buffer_id = buffer.buffer_id.clone();
        let mut result_queue = self.result_queue.clone();
        let mut core_proxy = self.core.clone().unwrap();
        let document_uri = Url::from_file_path(&buffer.path).unwrap();
        let position = get_position_of_offset(buffer, offset);
        match position {
            Ok(position) => lsp_client.request_completion(
                document_uri,
                position,
                move |lsp_client, result| {
                    if let Ok(res) = result {
                        result_queue
                            .push_result(request_id, LspResponse::Completion(res));
                        core_proxy.schedule_idle(buffer_id);
                    }
                },
            ),
            Err(e) => {}
        }
    }

    fn idle(&mut self, buffer: &mut Buffer) {
        let result = self.result_queue.pop_result();
        if let Some((request_id, reponse)) = result {
            match reponse {
                LspResponse::Completion(res) => self
                    .core
                    .as_mut()
                    .unwrap()
                    .show_completion(buffer.buffer_id.clone(), request_id, &res),
                LspResponse::Hover(res) => {
                    // let res = res
                    //     .and_then(|h| core_hover_from_hover(view, h))
                    //     .map_err(|e| e.into());
                    // self.with_language_server_for_view(view, |ls_client| {
                    //     ls_client
                    //         .core
                    //         .display_hover(view.get_id(), request_id, &res)
                    // });
                }
            }
        }
    }
}

pub fn core_hover_from_hover(
    buffer: &mut Buffer,
    hover: Hover,
) -> Result<CoreHover> {
    Ok(CoreHover {
        content: markdown_from_hover_contents(hover.contents)?,
        range: match hover.range {
            Some(range) => Some(core_range_from_range(buffer, range)?),
            None => None,
        },
    })
}

pub(crate) fn offset_of_position(
    buffer: &mut Buffer,
    position: Position,
) -> Result<usize> {
    let line_offset = buffer.offset_of_line(position.line as usize);

    let mut cur_len_utf16 = 0;
    let mut cur_len_utf8 = 0;

    for u in buffer.get_line(position.line as usize)?.chars() {
        if cur_len_utf16 >= (position.character as usize) {
            break;
        }
        cur_len_utf16 += u.len_utf16();
        cur_len_utf8 += u.len_utf8();
    }

    Ok(cur_len_utf8 + line_offset?)
}

pub(crate) fn core_range_from_range(
    buffer: &mut Buffer,
    range: Range,
) -> Result<CoreRange> {
    Ok(CoreRange {
        start: offset_of_position(buffer, range.start)?,
        end: offset_of_position(buffer, range.end)?,
    })
}

pub(crate) fn marked_string_to_string(marked_string: &MarkedString) -> String {
    match *marked_string {
        MarkedString::String(ref text) => text.to_owned(),
        MarkedString::LanguageString(ref d) => {
            format!("```{}\n{}\n```", d.language, d.value)
        }
    }
}

pub(crate) fn markdown_from_hover_contents(
    hover_contents: HoverContents,
) -> Result<String> {
    let res = match hover_contents {
        HoverContents::Scalar(content) => marked_string_to_string(&content),
        HoverContents::Array(content) => {
            let res: Vec<String> =
                content.iter().map(|c| marked_string_to_string(c)).collect();
            res.join("\n")
        }
        HoverContents::Markup(content) => content.value,
    };
    if res.is_empty() {
        Err(anyhow!("no hover contents"))
    } else {
        Ok(res)
    }
}

pub fn get_change_for_sync_kind(
    sync_kind: TextDocumentSyncKind,
    buffer: &mut Buffer,
    delta: &RopeDelta,
) -> Option<Vec<TextDocumentContentChangeEvent>> {
    match sync_kind {
        TextDocumentSyncKind::None => None,
        TextDocumentSyncKind::Full => {
            let text_document_content_change_event =
                TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: buffer.get_document().unwrap_or("".to_string()),
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
                            text: buffer.get_document().unwrap(),
                        };
                    Some(vec![text_document_content_change_event])
                }
            }
        }
    }
}

pub fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &mut Buffer,
) -> Result<Vec<TextDocumentContentChangeEvent>> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let text = String::from(node);

        let (start, end) = interval.start_end();
        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: get_position_of_offset(buffer, start)?,
                end: get_position_of_offset(buffer, end)?,
            }),
            range_length: Some((end - start) as u64),
            text,
        };

        return Ok(vec![text_document_content_change_event]);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let mut end_position = get_position_of_offset(buffer, end)?;

        // Hack around sending VSCode Style Positions to Language Server.
        // See this issue to understand: https://github.com/Microsoft/vscode/issues/23173
        if end_position.character == 0 {
            // There is an assumption here that the line separator character is exactly
            // 1 byte wide which is true for "\n" but it will be an issue if they are not
            // for example for u+2028
            let mut ep = get_position_of_offset(buffer, end - 1)?;
            ep.character += 1;
            end_position = ep;
        }

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: get_position_of_offset(buffer, start)?,
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
        text: buffer.get_document()?,
    };

    Ok(vec![text_document_content_change_event])
}

pub(crate) fn get_position_of_offset(
    buffer: &mut Buffer,
    offset: usize,
) -> Result<Position> {
    let line_num = buffer.line_of_offset(offset)?;
    let line_offset = buffer.offset_of_line(line_num)?;

    let char_offset =
        count_utf16(&(buffer.get_line(line_num)?[0..(offset - line_offset)]));

    Ok(Position {
        line: line_num as u64,
        character: char_offset as u64,
    })
}

pub(crate) fn count_utf16(s: &str) -> usize {
    let mut utf16_count = 0;
    for &b in s.as_bytes() {
        if (b as i8) >= -0x40 {
            utf16_count += 1;
        }
        if b >= 0xf0 {
            utf16_count += 1;
        }
    }
    utf16_count
}
