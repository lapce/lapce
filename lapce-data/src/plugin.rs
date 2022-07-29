use druid::WidgetId;
use strum_macros::Display;

pub struct PluginData {
    pub widget_id: WidgetId,
    pub installed_id: WidgetId,
    pub uninstalled_id: WidgetId,
}

impl PluginData {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            installed_id: WidgetId::next(),
            uninstalled_id: WidgetId::next(),
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
    Disabled,
}
