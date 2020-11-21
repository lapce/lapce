use anyhow::Result;
use std::borrow::Cow;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use lsp_types::*;
use serde::{Deserialize, Deserializer, Serialize};
use xi_rope::{
    interval::IntervalBounds, rope::Rope, Cursor, Delta, DeltaBuilder, Interval,
    LinesMetric, RopeDelta, RopeInfo, Transformer,
};

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct BufferId(pub usize);

pub struct Buffer {
    pub language_id: String,
    pub id: BufferId,
    pub rope: Rope,
    pub path: PathBuf,
    pub rev: u64,
}

impl Buffer {
    pub fn new(id: BufferId, path: PathBuf) -> Buffer {
        let rope = if let Ok(rope) = load_file(&path) {
            rope
        } else {
            Rope::from("")
        };
        let language_id = language_id_from_path(&path).unwrap_or("").to_string();
        Buffer {
            id,
            rope,
            path,
            language_id,
            rev: 0,
        }
    }

    pub fn update(
        &mut self,
        delta: &RopeDelta,
        rev: u64,
    ) -> Option<TextDocumentContentChangeEvent> {
        if self.rev + 1 != rev {
            return None;
        }
        self.rev += 1;
        let content_change = get_document_content_changes(delta, self);
        self.rope = delta.apply(&self.rope);
        let content_change = match content_change {
            Some(content_change) => content_change,
            None => TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.get_document(),
            },
        };
        Some(content_change)
    }

    pub fn get_document(&self) -> String {
        self.rope.to_string()
    }

    pub fn offset_of_line(&self, offset: usize) -> usize {
        self.rope.offset_of_line(offset)
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.rope.line_of_offset(offset)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of_offset(offset);
        (line, offset - self.offset_of_line(line))
    }

    pub fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset);
        Position {
            line: line as u64,
            character: col as u64,
        }
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }
}

fn load_file(path: &PathBuf) -> Result<Rope> {
    let mut f = File::open(path)?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    Ok(Rope::from(std::str::from_utf8(&bytes)?))
}

fn language_id_from_path(path: &PathBuf) -> Option<&str> {
    Some(match path.extension()?.to_str()? {
        "rs" => "rust",
        "go" => "go",
        _ => return None,
    })
}

fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &Buffer,
) -> Option<TextDocumentContentChangeEvent> {
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

        return Some(text_document_content_change_event);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let mut end_position = buffer.offset_to_position(end);

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: end_position,
            }),
            range_length: Some((end - start) as u64),
            text: String::new(),
        };

        return Some(text_document_content_change_event);
    }

    None
}
