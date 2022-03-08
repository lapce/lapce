use std::{collections::HashMap, path::PathBuf, sync::Arc};

use druid::WidgetId;

#[derive(Clone)]
pub struct SearchData {
    pub active: WidgetId,
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub editor_view_id: WidgetId,
    pub matches: Arc<HashMap<PathBuf, Vec<(usize, (usize, usize), String)>>>,
}

impl SearchData {
    pub fn new() -> Self {
        let editor_view_id = WidgetId::next();
        Self {
            active: editor_view_id,
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            editor_view_id,
            matches: Arc::new(HashMap::new()),
        }
    }
}

impl Default for SearchData {
    fn default() -> Self {
        Self::new()
    }
}
