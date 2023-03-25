use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal},
};
use lapce_rpc::plugin::PluginId;
use lsp_types::CodeActionOrCommand;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CodeActionStatus {
    Inactive,
    Active,
}

#[derive(Clone, PartialEq)]
pub struct ScoredCodeActionItem {
    pub item: CodeActionOrCommand,
    pub plugin_id: PluginId,
    pub score: i64,
    pub indices: Vec<usize>,
}

impl ScoredCodeActionItem {
    pub fn title(&self) -> &str {
        match &self.item {
            CodeActionOrCommand::Command(c) => &c.title,
            CodeActionOrCommand::CodeAction(c) => &c.title,
        }
    }
}

#[derive(Clone)]
pub struct CodeActionData {
    pub status: CodeActionStatus,
    pub active: RwSignal<usize>,
    pub request_id: usize,
    pub input_id: usize,
    pub items: im::Vector<ScoredCodeActionItem>,
    pub filtered_items: im::Vector<ScoredCodeActionItem>,
}

impl CodeActionData {
    pub fn new(cx: AppContext) -> Self {
        let active = create_rw_signal(cx.scope, 0);
        Self {
            status: CodeActionStatus::Inactive,
            active,
            request_id: 0,
            input_id: 0,
            items: im::Vector::new(),
            filtered_items: im::Vector::new(),
        }
    }
}
