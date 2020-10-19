use anyhow::{anyhow, Result};
use lapce_core::{
    buffer::BufferId,
    plugin::{GetDataResponse, PluginBufferInfo, PluginId},
};
use memchr::memchr;
use serde::Deserialize;
use serde_json::json;
use xi_rpc::RpcPeer;

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
            contents: "".to_string(),
            peer,
            plugin_id,
            language_id: info.language_id,
            buffer_id: info.buffer_id,
            path: info.path,
            line_offsets: Vec::new(),
        }
    }

    pub fn get_document(&mut self) -> Result<String> {
        let params = json!({
            "plugin_id": self.plugin_id,
            "buffer_id": self.buffer_id,
            "rev": 0,
        });
        let response = self
            .peer
            .send_rpc_request("get_data", &params)
            .map_err(|e| anyhow!("rpc error"))?;
        let response = GetDataResponse::deserialize(response)?;
        self.contents = response.chunk;
        self.recalculate_line_offsets();
        Ok(self.contents.clone())
    }

    fn recalculate_line_offsets(&mut self) {
        self.line_offsets.clear();
        newline_offsets(&self.contents, &mut self.line_offsets);
    }

    fn offset_of_line(&mut self, line_num: usize) -> Result<usize, Error> {
        if line_num > self.num_lines {
            return Err(Error::BadRequest);
        }
        self.cached_offset_of_line(line_num)
    }

    fn line_of_offset<DS: DataSource>(
        &mut self,
        source: &DS,
        offset: usize,
    ) -> Result<usize, Error> {
        if offset > self.buf_size {
            return Err(Error::BadRequest);
        }
        if self.contents.is_empty()
            || offset < self.offset
            || offset > self.offset + self.contents.len()
        {
            let resp =
                source.get_data(offset, TextUnit::Utf8, CHUNK_SIZE, self.rev)?;
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
