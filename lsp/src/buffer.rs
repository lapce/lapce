use anyhow::{anyhow, Result};
use languageserver_types::Position;
use lapce_core::{
    buffer::BufferId,
    plugin::{GetDataResponse, PluginBufferInfo, PluginId, TextUnit},
};
use memchr::memchr;
use serde::Deserialize;
use serde_json::json;
use xi_rope::{DeltaElement, LinesMetric, Rope, RopeDelta};
use xi_rpc::RemoteError;
use xi_rpc::RpcPeer;

const CHUNK_SIZE: usize = 1024 * 1024;

pub struct Buffer {
    pub buffer_id: BufferId,
    plugin_id: PluginId,
    pub language_id: String,
    pub path: String,
    peer: RpcPeer,

    pub offset: usize,
    pub contents: String,
    pub first_line: usize,
    pub first_line_offset: usize,
    pub line_offsets: Vec<usize>,
    pub buf_size: usize,
    pub num_lines: usize,
    pub rev: u64,
}

impl Buffer {
    pub fn new(peer: RpcPeer, plugin_id: PluginId, info: PluginBufferInfo) -> Self {
        Buffer {
            peer,
            plugin_id,
            language_id: info.language_id,
            buffer_id: info.buffer_id,
            path: info.path,
            line_offsets: Vec::new(),
            buf_size: info.buf_size,
            num_lines: info.nb_lines,
            rev: info.rev,
            offset: 0,
            first_line: 0,
            first_line_offset: 0,
            contents: "".to_string(),
        }
    }

    pub fn get_line(&mut self, line_num: usize) -> Result<&str> {
        if line_num >= self.num_lines {
            return Err(anyhow!("bad request"));
        }

        // if chunk does not include the start of this line, fetch and reset everything
        if self.contents.is_empty()
            || line_num < self.first_line
            || (line_num == self.first_line && self.first_line_offset > 0)
            || (line_num > self.first_line + self.line_offsets.len())
        {
            let resp =
                self.get_data(line_num, TextUnit::Line, CHUNK_SIZE, self.rev)?;
            self.reset_chunk(resp);
        }

        // We now know that the start of this line is contained in self.contents.
        let mut start_off =
            self.cached_offset_of_line(line_num).unwrap() - self.offset;

        // Now we make sure we also contain the end of the line, fetching more
        // of the document as necessary.
        loop {
            if let Some(end_off) = self.cached_offset_of_line(line_num + 1) {
                return Ok(&self.contents[start_off..end_off - self.offset]);
            }
            // if we have a chunk and we're fetching more, discard unnecessary
            // portion of our chunk.
            if start_off != 0 {
                self.clear_up_to(start_off);
                start_off = 0;
            }

            let chunk_end = self.offset + self.contents.len();
            let resp =
                self.get_data(chunk_end, TextUnit::Utf8, CHUNK_SIZE, self.rev)?;
            self.append_chunk(&resp);
        }
    }

    fn get_data(
        &self,
        start: usize,
        unit: TextUnit,
        max_size: usize,
        rev: u64,
    ) -> Result<GetDataResponse> {
        let params = json!({
            "plugin_id": self.plugin_id,
            "buffer_id": self.buffer_id,
            "start": start,
            "unit": unit,
            "max_size": max_size,
            "rev": rev,
        });
        let result = self
            .peer
            .send_rpc_request("get_data", &params)
            .map_err(|e| anyhow!(""))?;
        GetDataResponse::deserialize(result)
            .map_err(|e| anyhow!("wrong return type"))
    }

    pub fn get_document(&mut self) -> Result<String> {
        let mut result = String::new();
        let mut cur_idx = 0;
        while cur_idx < self.buf_size {
            if self.contents.is_empty() || cur_idx != self.offset {
                let resp =
                    self.get_data(cur_idx, TextUnit::Utf8, CHUNK_SIZE, self.rev)?;
                self.reset_chunk(resp);
            }
            result.push_str(&self.contents);
            cur_idx = self.offset + self.contents.len();
        }
        Ok(result)
    }

    fn append_chunk(&mut self, data: &GetDataResponse) {
        self.contents.push_str(data.chunk.as_str());
        // this is doing extra work in the case where we're fetching a single
        // massive (multiple of CHUNK_SIZE) line, but unclear if it's worth optimizing
        self.recalculate_line_offsets();
    }

    fn reset_chunk(&mut self, data: GetDataResponse) {
        self.contents = data.chunk;
        self.offset = data.offset;
        self.first_line = data.first_line;
        self.first_line_offset = data.first_line_offset;
        self.recalculate_line_offsets();
    }

    pub fn update(
        &mut self,
        delta: &RopeDelta,
        new_len: usize,
        new_num_lines: usize,
        rev: u64,
    ) {
        let is_empty = self.offset == 0 && self.contents.is_empty();
        let should_clear = if !is_empty {
            self.should_clear(delta)
        } else {
            true
        };

        if should_clear {
            self.clear();
        } else {
            // only reached if delta exists
            self.update_chunk(delta);
        }
        self.buf_size = new_len;
        self.num_lines = new_num_lines;
        self.rev = rev;
    }

    fn update_chunk(&mut self, delta: &RopeDelta) {
        let chunk_start = self.offset;
        let chunk_end = chunk_start + self.contents.len();
        let mut new_state = String::with_capacity(self.contents.len());
        let mut prev_copy_end = 0;
        let mut del_before: usize = 0;
        let mut ins_before: usize = 0;

        for op in delta.els.as_slice() {
            match *op {
                DeltaElement::Copy(start, end) => {
                    if start < chunk_start {
                        del_before += start - prev_copy_end;
                        if end >= chunk_start {
                            let cp_end =
                                (end - chunk_start).min(self.contents.len());
                            new_state.push_str(&self.contents[0..cp_end]);
                        }
                    } else if start <= chunk_end {
                        if prev_copy_end < chunk_start {
                            del_before += chunk_start - prev_copy_end;
                        }
                        let cp_start = start - chunk_start;
                        let cp_end = (end - chunk_start).min(self.contents.len());
                        new_state.push_str(&self.contents[cp_start..cp_end]);
                    }
                    prev_copy_end = end;
                }
                DeltaElement::Insert(ref s) => {
                    if prev_copy_end < chunk_start {
                        ins_before += s.len();
                    } else if prev_copy_end <= chunk_end {
                        let s: String = s.into();
                        new_state.push_str(&s);
                    }
                }
            }
        }
        self.offset += ins_before;
        self.offset -= del_before;
        self.contents = new_state;
    }

    fn should_clear(&mut self, delta: &RopeDelta) -> bool {
        let (iv, _) = delta.summary();
        let start = iv.start();
        let end = iv.end();
        // we only apply the delta if it is a simple edit, which
        // begins inside or immediately following our chunk.
        // - If it begins _before_ our chunk, we are likely going to
        // want to fetch the edited region, which will reset our state;
        // - If it's a complex edit the logic is tricky, and this should
        // be rare enough we can afford to discard.
        // The one 'complex edit' we should probably be handling is
        // the replacement of a single range. This could be a new
        // convenience method on `Delta`?
        if start < self.offset || start > self.offset + self.contents.len() {
            true
        } else if delta.is_simple_delete() {
            // Don't go over cache boundary.
            let end = end.min(self.offset + self.contents.len());

            self.simple_delete(start, end);
            false
        } else if let Some(text) = delta.as_simple_insert() {
            assert_eq!(iv.size(), 0);
            self.simple_insert(text, start);
            false
        } else {
            true
        }
    }

    fn simple_insert(&mut self, text: &Rope, ins_offset: usize) {
        let has_newline = text.measure::<LinesMetric>() > 0;
        let self_off = self.offset;
        assert!(ins_offset >= self_off);
        // regardless of if we are inserting newlines we adjust offsets
        self.line_offsets.iter_mut().for_each(|off| {
            if *off > ins_offset - self_off {
                *off += text.len()
            }
        });
        // calculate and insert new newlines if necessary
        // we could save some hassle and just rerun memchr on the chunk here?
        if has_newline {
            let mut new_offsets = Vec::new();
            newline_offsets(&String::from(text), &mut new_offsets);
            new_offsets
                .iter_mut()
                .for_each(|off| *off += ins_offset - self_off);

            let split_idx = self
                .line_offsets
                .binary_search(&new_offsets[0])
                .err()
                .expect("new index cannot be occupied");

            self.line_offsets = [
                &self.line_offsets[..split_idx],
                &new_offsets,
                &self.line_offsets[split_idx..],
            ]
            .concat();
        }
    }

    /// Patches up `self.line_offsets` in the simple delete case.
    fn simple_delete(&mut self, start: usize, end: usize) {
        let del_size = end - start;
        let start = start - self.offset;
        let end = end - self.offset;
        let has_newline =
            memchr(b'\n', &self.contents.as_bytes()[start..end]).is_some();
        // a bit too fancy: only reallocate if we need to remove an item
        if has_newline {
            self.line_offsets = self
                .line_offsets
                .iter()
                .filter_map(|off| match *off {
                    x if x <= start => Some(x),
                    x if x > start && x <= end => None,
                    x if x > end => Some(x - del_size),
                    hmm => panic!("invariant violated {} {} {}?", start, end, hmm),
                })
                .collect();
        } else {
            self.line_offsets.iter_mut().for_each(|off| {
                if *off >= end {
                    *off -= del_size
                }
            });
        }
    }

    fn clear(&mut self) {
        self.contents.clear();
        self.offset = 0;
        self.line_offsets.clear();
        self.first_line = 0;
        self.first_line_offset = 0;
    }

    fn clear_up_to(&mut self, offset: usize) {
        if offset > self.contents.len() {
            panic!(
                "offset greater than content length: {} > {}",
                offset,
                self.contents.len()
            )
        }

        let new_contents = self.contents.split_off(offset);
        self.contents = new_contents;
        self.offset += offset;
        // first find out if offset is a line offset, and set first_line / first_line_offset
        let (new_line, new_line_off) = match self.line_offsets.binary_search(&offset)
        {
            Ok(idx) => (self.first_line + idx + 1, 0),
            Err(0) => (self.first_line, self.first_line_offset + offset),
            Err(idx) => (self.first_line + idx, offset - self.line_offsets[idx - 1]),
        };

        // then clear line_offsets up to and including offset
        self.line_offsets = self
            .line_offsets
            .iter()
            .filter(|i| **i > offset)
            .map(|i| i - offset)
            .collect();

        self.first_line = new_line;
        self.first_line_offset = new_line_off;
    }

    fn recalculate_line_offsets(&mut self) {
        self.line_offsets.clear();
        newline_offsets(&self.contents, &mut self.line_offsets);
    }

    pub fn offset_of_line(&mut self, line_num: usize) -> Result<usize> {
        if line_num > self.num_lines {
            return Err(anyhow!("bad request"));
        }
        match self.cached_offset_of_line(line_num) {
            Some(offset) => Ok(offset),
            None => {
                let resp =
                    self.get_data(line_num, TextUnit::Line, CHUNK_SIZE, self.rev)?;
                self.reset_chunk(resp);
                self.offset_of_line(line_num)
            }
        }
    }

    pub fn line_of_offset(&mut self, offset: usize) -> Result<usize> {
        if offset > self.buf_size {
            return Err(anyhow!("bad request"));
        }
        if self.contents.is_empty()
            || offset < self.offset
            || offset > self.offset + self.contents.len()
        {
            let resp =
                self.get_data(offset, TextUnit::Utf8, CHUNK_SIZE, self.rev)?;
            self.reset_chunk(resp);
        }

        let rel_offset = offset - self.offset;
        let line_num = match self.line_offsets.binary_search(&rel_offset) {
            Ok(ix) => ix + self.first_line + 1,
            Err(ix) => ix + self.first_line,
        };
        Ok(line_num)
    }

    fn cached_offset_of_line(&self, line_num: usize) -> Option<usize> {
        if line_num < self.first_line {
            return None;
        }

        let rel_line_num = line_num - self.first_line;

        if rel_line_num == 0 {
            return Some(self.offset - self.first_line_offset);
        }

        if rel_line_num <= self.line_offsets.len() {
            return Some(self.offset + self.line_offsets[rel_line_num - 1]);
        }

        // EOF
        if line_num == self.num_lines
            && self.offset + self.contents.len() == self.buf_size
        {
            return Some(self.offset + self.contents.len());
        }
        None
    }
}

fn newline_offsets(text: &str, storage: &mut Vec<usize>) {
    let mut cur_idx = 0;
    while let Some(idx) = memchr(b'\n', &text.as_bytes()[cur_idx..]) {
        storage.push(cur_idx + idx + 1);
        cur_idx += idx + 1;
    }
}
