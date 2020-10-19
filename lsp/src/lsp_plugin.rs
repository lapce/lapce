use std::{collections::HashMap, sync::Arc};

use languageserver_types::{
    InitializeResult, Position, Range, TextDocumentContentChangeEvent,
    TextDocumentSyncKind, Url,
};
use lapce_core::plugin::PluginBufferInfo;
use parking_lot::Mutex;
use xi_rope::RopeDelta;

use crate::{
    buffer::Buffer, lsp_client::LspClient, plugin::CoreProxy, plugin::Plugin,
};

pub struct LspPlugin {
    core: Option<CoreProxy>,
    lsp_clients: HashMap<String, Arc<Mutex<LspClient>>>,
}

impl LspPlugin {
    pub fn new() -> LspPlugin {
        LspPlugin {
            core: None,
            lsp_clients: HashMap::new(),
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
            lsp_client.send_initialize(None, move |ls_client, result| {
                if let Ok(result) = result {
                    let init_result: InitializeResult =
                        serde_json::from_value(result).unwrap();

                    ls_client.server_capabilities = Some(init_result.capabilities);
                    ls_client.is_initialized = true;
                    ls_client.send_did_open(&buffer_id, document_uri, document_text);
                }
            });
        }
        eprintln!("got new buffer");
    }

    fn update(&mut self, buffer: &mut Buffer, delta: &RopeDelta) {
        let mut lsp_client =
            self.lsp_clients.get(&buffer.language_id).unwrap().lock();
        let sync_kind = lsp_client.get_sync_kind();
        if let Some(changes) = get_change_for_sync_kind(sync_kind, buffer, delta) {
            lsp_client.send_did_change(
                &buffer.buffer_id,
                changes,
                view_info.version,
            );
        }
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
) -> Result<Vec<TextDocumentContentChangeEvent>, PluginLibError> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let text = String::from(node);

        let (start, end) = interval.start_end();
        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: get_position_of_offset(view, start)?,
                end: get_position_of_offset(view, end)?,
            }),
            range_length: Some((end - start) as u64),
            text,
        };

        return Ok(vec![text_document_content_change_event]);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let mut end_position = get_position_of_offset(view, end)?;

        // Hack around sending VSCode Style Positions to Language Server.
        // See this issue to understand: https://github.com/Microsoft/vscode/issues/23173
        if end_position.character == 0 {
            // There is an assumption here that the line separator character is exactly
            // 1 byte wide which is true for "\n" but it will be an issue if they are not
            // for example for u+2028
            let mut ep = get_position_of_offset(view, end - 1)?;
            ep.character += 1;
            end_position = ep;
        }

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: get_position_of_offset(view, start)?,
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
) -> Result<Position, PluginLibError> {
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
