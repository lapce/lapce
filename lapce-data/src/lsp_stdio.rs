use druid::WidgetId;
use serde_json::Value;

#[derive(Clone)]
pub struct LspStdioData {
    pub widget_id: WidgetId,
    pub lsp_request: im::Vector<Value>,
    pub lsp_response: im::Vector<Value>,
}

impl Default for LspStdioData {
    fn default() -> LspStdioData {
        LspStdioData {
            widget_id: WidgetId::next(),
            lsp_request: im::Vector::default(),
            lsp_response: im::Vector::default(),
        }
    }
}
