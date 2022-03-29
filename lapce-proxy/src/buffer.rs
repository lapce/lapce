use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use lapce_rpc::buffer::BufferId;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::{borrow::Cow, path::Path, time::SystemTime};

use lsp_types::*;
use xi_rope::{interval::IntervalBounds, rope::Rope, RopeDelta};

pub struct Buffer {
    pub language_id: String,
    pub id: BufferId,
    pub rope: Rope,
    pub path: PathBuf,
    pub rev: u64,
    pub dirty: bool,
    sender: Sender<(BufferId, u64)>,
    pub mod_time: Option<SystemTime>,
}

impl Buffer {
    pub fn new(
        id: BufferId,
        path: PathBuf,
        sender: Sender<(BufferId, u64)>,
    ) -> Buffer {
        let rope = if let Ok(rope) = load_file(&path) {
            rope
        } else {
            Rope::from("")
        };
        let language_id = language_id_from_path(&path).unwrap_or("").to_string();
        let mod_time = get_mod_time(&path);
        Buffer {
            id,
            rope,
            path,
            language_id,
            rev: 0,
            sender,
            dirty: false,
            mod_time,
        }
    }

    pub fn save(&mut self, rev: u64) -> Result<()> {
        if self.rev != rev {
            return Err(anyhow!("not the right rev"));
        }
        self.dirty = false;
        let tmp_extension = self.path.extension().map_or_else(
            || OsString::from("swp"),
            |ext| {
                let mut ext = ext.to_os_string();
                ext.push(".swp");
                ext
            },
        );
        let tmp_path = &self.path.with_extension(tmp_extension);

        let mut f = File::create(tmp_path)?;
        for chunk in self.rope.iter_chunks(..self.rope.len()) {
            f.write_all(chunk.as_bytes())?;
        }
        fs::rename(tmp_path, &self.path)?;
        self.mod_time = get_mod_time(&self.path);
        Ok(())
    }

    pub fn reload(&mut self) {
        let rope = if let Ok(rope) = load_file(&self.path) {
            rope
        } else {
            Rope::from("")
        };

        self.rope = rope;
        self.rev += 1;
        let _ = self.sender.send((self.id, self.rev));
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
        self.dirty = true;
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
        let _ = self.sender.send((self.id, self.rev));
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
            line: line as u32,
            character: col as u32,
        }
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn load_file(path: &Path) -> Result<Rope> {
    let mut f = File::open(path)?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    Ok(Rope::from(std::str::from_utf8(&bytes)?))
}

fn language_id_from_path(path: &Path) -> Option<&str> {
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
            range_length: Some((end - start) as u32),
            text,
        };

        return Some(text_document_content_change_event);
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let end_position = buffer.offset_to_position(end);

        let text_document_content_change_event = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: buffer.offset_to_position(start),
                end: end_position,
            }),
            range_length: Some((end - start) as u32),
            text: String::new(),
        };

        return Some(text_document_content_change_event);
    }

    None
}

/// Returns the modification timestamp for the file at a given path,
/// if present.
pub fn get_mod_time<P: AsRef<Path>>(path: P) -> Option<SystemTime> {
    File::open(path)
        .and_then(|f| f.metadata())
        .and_then(|meta| meta.modified())
        .ok()
}
