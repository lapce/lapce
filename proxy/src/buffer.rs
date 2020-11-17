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
    pub language_id: String,
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
        let language_id = language_id_from_path(&path).unwrap_or("").to_string();
        Buffer {
            id,
            rope,
            path,
            language_id,
        }
    }

    pub fn get_document(&self) -> String {
        self.rope.to_string()
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
