use anyhow::Result;
use std::fs::File;
use std::io::{self, Read, Write};
use xi_rope::{rope::Rope, LinesMetric};

#[derive(Eq, PartialEq, Hash, Clone)]
pub struct BufferId(pub usize);

pub struct Buffer {
    pub rope: Rope,
    pub num_lines: usize,
    pub max_line_len: usize,
}

impl Buffer {
    pub fn new(buffer_id: BufferId, path: &str) -> Buffer {
        let rope = if let Ok(rope) = load_file(path) {
            rope
        } else {
            Rope::from("")
        };
        let num_lines = rope.line_of_offset(rope.len()) + 1;

        let mut pre_offset = 0;
        let mut max_line_len = 0;
        for i in 0..num_lines {
            let offset = rope.offset_of_line(i);
            let line_len = offset - pre_offset;
            pre_offset = offset;
            if line_len > max_line_len {
                max_line_len = line_len;
            }
        }
        Buffer {
            rope,
            num_lines,
            max_line_len,
        }
    }
}

fn load_file(path: &str) -> Result<Rope> {
    let mut f = File::open(path)?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    Ok(Rope::from(std::str::from_utf8(&bytes)?))
}
