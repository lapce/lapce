use anyhow::Result;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};
use xi_rope::{
    interval::IntervalBounds, rope::Rope, Cursor, Delta, DeltaBuilder, Interval,
    LinesMetric, RopeDelta, RopeInfo, Transformer,
};

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct BufferId(pub usize);

pub struct Buffer {
    pub id: BufferId,
    pub rope: Rope,
    pub path: PathBuf,
}

impl Buffer {
    pub fn new(id: BufferId, path: PathBuf) -> Buffer {
        let rope = if let Ok(rope) = load_file(&path) {
            rope
        } else {
            Rope::from("")
        };
        Buffer { id, rope, path }
    }
}

fn load_file(path: &PathBuf) -> Result<Rope> {
    let mut f = File::open(path)?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    Ok(Rope::from(std::str::from_utf8(&bytes)?))
}
