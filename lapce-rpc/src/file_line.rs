use std::path::PathBuf;

use lsp_types::Position;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLine {
    pub path: PathBuf,
    pub position: Position,
    pub content: String,
}
