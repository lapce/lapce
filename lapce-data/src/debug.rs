use std::path::Path;

use druid::WidgetId;

pub struct DebugData {
    pub widget_id: WidgetId,
    pub split_id: WidgetId,
}

impl DebugData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            split_id: WidgetId::next(),
        }
    }
}

pub struct RunConfigs {
    pub configs: Vec<RunConfig>,
}

pub struct RunConfig {}

pub fn run_configs(workspace: Option<&Path>) {}
