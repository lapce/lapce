use std::sync::Arc;

use floem::{
    app::AppContext,
    peniko::kurbo::Rect,
    reactive::{
        create_rw_signal, ReadSignal, RwSignal, SignalGetUntracked, SignalSet,
    },
};
use lapce_core::movement::Movement;
use lapce_rpc::plugin::PluginId;
use lsp_types::CodeActionOrCommand;

use crate::config::LapceConfig;

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
    pub offset: usize,
    pub items: im::Vector<ScoredCodeActionItem>,
    pub filtered_items: im::Vector<ScoredCodeActionItem>,
    pub layout_rect: Rect,
    config: ReadSignal<Arc<LapceConfig>>,
}

impl CodeActionData {
    pub fn new(cx: AppContext, config: ReadSignal<Arc<LapceConfig>>) -> Self {
        let active = create_rw_signal(cx.scope, 0);
        Self {
            status: CodeActionStatus::Inactive,
            active,
            request_id: 0,
            input_id: 0,
            offset: 0,
            items: im::Vector::new(),
            filtered_items: im::Vector::new(),
            layout_rect: Rect::ZERO,
            config,
        }
    }

    pub fn next(&mut self) {
        let active = self.active.get_untracked();
        let new =
            Movement::Down.update_index(active, self.filtered_items.len(), 1, true);
        self.active.set(new);
    }

    pub fn previous(&mut self) {
        let active = self.active.get_untracked();
        let new =
            Movement::Up.update_index(active, self.filtered_items.len(), 1, true);
        self.active.set(new);
    }

    pub fn next_page(&mut self) {
        let config = self.config.get_untracked();
        let count = ((self.layout_rect.size().height
            / config.editor.line_height() as f64)
            .floor() as usize)
            .saturating_sub(1);
        let active = self.active.get_untracked();
        let new = Movement::Down.update_index(
            active,
            self.filtered_items.len(),
            count,
            false,
        );
        self.active.set(new);
    }

    pub fn previous_page(&mut self) {
        let config = self.config.get_untracked();
        let count = ((self.layout_rect.size().height
            / config.editor.line_height() as f64)
            .floor() as usize)
            .saturating_sub(1);
        let active = self.active.get_untracked();
        let new = Movement::Up.update_index(
            active,
            self.filtered_items.len(),
            count,
            false,
        );
        self.active.set(new);
    }
}
