use std::{
    borrow::Cow,
    ffi::OsString,
    fs,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{anyhow, Result};
use lapce_core::{
    buffer::rope_text::CharIndicesJoin, encoding::offset_utf8_to_utf16,
};
use lapce_rpc::{buffer::BufferId, language_id::LanguageId};
use lapce_xi_rope::{interval::IntervalBounds, rope::Rope, RopeDelta};
use lsp_types::*;

#[derive(Clone)]
pub struct Buffer {
    pub language_id: LanguageId,
    pub id: BufferId,
    pub rope: Rope,
    pub path: PathBuf,
    pub rev: u64,
    pub mod_time: Option<SystemTime>,
}

impl Buffer {
    pub fn new(id: BufferId, path: PathBuf) -> Buffer {
        let rope = Rope::from(load_file(&path).unwrap_or_default());
        let rev = u64::from(!rope.is_empty());
        let language_id = LanguageId::from_path(&path);
        let mod_time = get_mod_time(&path);
        Buffer {
            id,
            rope,
            path,
            language_id,
            rev,
            mod_time,
        }
    }

    pub fn save(&mut self, rev: u64) -> Result<()> {
        if self.rev != rev {
            return Err(anyhow!("not the right rev"));
        }
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

        if let Ok(metadata) = fs::metadata(&self.path) {
            let perm = metadata.permissions();
            fs::set_permissions(tmp_path, perm)?;
        }

        fs::rename(tmp_path, &self.path)?;
        self.mod_time = get_mod_time(&self.path);
        Ok(())
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
        Some(
            content_change.unwrap_or_else(|| TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.get_document(),
            }),
        )
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

    /// Converts a UTF8 offset to a UTF16 LSP position  
    pub fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset);
        // Get the offset of line to make the conversion cheaper, rather than working
        // from the very start of the document to `offset`
        let line_offset = self.offset_of_line(line);
        let utf16_col =
            offset_utf8_to_utf16(self.char_indices_iter(line_offset..), col);

        Position {
            line: line as u32,
            character: utf16_col as u32,
        }
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }

    /// Iterate over (utf8_offset, char) values in the given range  
    /// This uses `iter_chunks` and so does not allocate, compared to `slice_to_cow` which can
    pub fn char_indices_iter<T: IntervalBounds>(
        &self,
        range: T,
    ) -> impl Iterator<Item = (usize, char)> + '_ {
        CharIndicesJoin::new(self.rope.iter_chunks(range).map(str::char_indices))
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn load_file(path: &Path) -> Result<String> {
    Ok(read_path_to_string_lossy(path)?)
}

pub fn read_path_to_string_lossy<P: AsRef<Path>>(
    path: P,
) -> Result<String, std::io::Error> {
    let path = path.as_ref();

    let mut file = File::open(path)?;
    // Read the file in as bytes
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Parse the file contents as utf8, replacing non-utf8 data with the
    // replacement character
    let contents = String::from_utf8_lossy(&buffer);

    Ok(contents.to_string())
}

fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &Buffer,
) -> Option<TextDocumentContentChangeEvent> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let (start, end) = interval.start_end();
        let start = buffer.offset_to_position(start);

        let end = buffer.offset_to_position(end);

        Some(TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text: String::from(node),
        })
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let end_position = buffer.offset_to_position(end);

        let start = buffer.offset_to_position(start);

        Some(TextDocumentContentChangeEvent {
            range: Some(Range {
                start,
                end: end_position,
            }),
            range_length: None,
            text: String::new(),
        })
    } else {
        None
    }
}

/// Returns the modification timestamp for the file at a given path,
/// if present.
pub fn get_mod_time<P: AsRef<Path>>(path: P) -> Option<SystemTime> {
    File::open(path)
        .and_then(|f| f.metadata())
        .and_then(|meta| meta.modified())
        .ok()
}
