use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub id: usize,
    pub content: PaletteItemContent,
    pub filter_text: String,
    pub score: i64,
    pub indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteItemContent {
    File { path: PathBuf, full_path: PathBuf },
}
