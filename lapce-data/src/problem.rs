use druid::WidgetId;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone)]
pub struct ProblemData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
    pub error_widget_id: WidgetId,
    pub warning_widget_id: WidgetId,
    pub fold: HashMap<PathBuf, bool>,
}

impl ProblemData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
            error_widget_id: WidgetId::next(),
            warning_widget_id: WidgetId::next(),
            fold: HashMap::new(),
        }
    }
}

impl Default for ProblemData {
    fn default() -> Self {
        Self::new()
    }
}
