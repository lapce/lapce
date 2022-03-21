use druid::WidgetId;
use strum_macros::Display;

pub struct PluginData {
    pub widget_id: WidgetId,
}

impl PluginData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
        }
    }
}

impl Default for PluginData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Display, PartialEq)]
pub enum PluginStatus {
    Installed,
    Install,
    Upgrade,
}
