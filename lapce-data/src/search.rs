use std::{path::PathBuf, sync::Arc};

use druid::WidgetId;
use indexmap::IndexMap;

pub type Match = (usize, (usize, usize), String);
#[derive(Clone)]
pub struct SearchData {
    pub active: WidgetId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub editor_view_id: WidgetId,
    pub matches: Arc<IndexMap<PathBuf, Vec<Match>>>,
}

impl SearchData {
    pub fn new() -> Self {
        let editor_view_id = WidgetId::next();
        Self {
            active: editor_view_id,
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            editor_view_id,
            matches: Arc::new(IndexMap::new()),
        }
    }
}

impl Default for SearchData {
    fn default() -> Self {
        Self::new()
    }
}
