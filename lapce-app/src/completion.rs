use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    peniko::kurbo::Rect,
    reactive::{create_rw_signal, ReadSignal, RwSignal, UntrackedGettableSignal},
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use lapce_core::movement::Movement;
use lapce_rpc::{plugin::PluginId, proxy::ProxyRpcHandler};
use lsp_types::{CompletionItem, CompletionResponse, Position};

use crate::config::LapceConfig;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompletionStatus {
    Inactive,
    Started,
    Done,
}

#[derive(Clone, PartialEq)]
pub struct ScoredCompletionItem {
    pub item: CompletionItem,
    pub plugin_id: PluginId,
    pub score: i64,
    pub label_score: i64,
    pub indices: Vec<usize>,
}

#[derive(Clone)]
pub struct CompletionData {
    pub status: CompletionStatus,
    pub request_id: usize,
    pub input_id: usize,
    pub path: PathBuf,
    pub offset: usize,
    pub active: RwSignal<usize>,
    pub input: String,
    pub input_items: im::HashMap<String, im::Vector<ScoredCompletionItem>>,
    pub filtered_items: im::Vector<ScoredCompletionItem>,
    pub layout_rect: Rect,
    matcher: Arc<SkimMatcherV2>,
    config: ReadSignal<Arc<LapceConfig>>,
}

impl CompletionData {
    pub fn new(cx: AppContext, config: ReadSignal<Arc<LapceConfig>>) -> Self {
        let active = create_rw_signal(cx.scope, 0);
        Self {
            status: CompletionStatus::Inactive,
            request_id: 0,
            input_id: 0,
            path: PathBuf::new(),
            offset: 0,
            active,
            input: "".to_string(),
            input_items: im::HashMap::new(),
            filtered_items: im::Vector::new(),
            layout_rect: Rect::ZERO,
            matcher: Arc::new(SkimMatcherV2::default().ignore_case()),
            config,
        }
    }

    pub fn receive(
        &mut self,
        request_id: usize,
        input: &str,
        resp: &CompletionResponse,
        plugin_id: PluginId,
    ) {
        if self.status == CompletionStatus::Inactive || self.request_id != request_id
        {
            return;
        }

        println!("receive completion");
        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => &list.items,
        };
        let items: im::Vector<ScoredCompletionItem> = items
            .iter()
            .map(|i| ScoredCompletionItem {
                item: i.to_owned(),
                plugin_id,
                score: 0,
                label_score: 0,
                indices: Vec::new(),
            })
            .collect();
        self.input_items.insert(input.to_string(), items);
        self.filter_items();
    }

    pub fn request(
        &mut self,
        proxy_rpc: &ProxyRpcHandler,
        path: PathBuf,
        input: String,
        position: Position,
    ) {
        self.input_items.insert(input.clone(), im::Vector::new());
        proxy_rpc.completion(self.request_id, path, input, position);
    }

    pub fn cancel(&mut self) {
        if self.status == CompletionStatus::Inactive {
            return;
        }
        self.status = CompletionStatus::Inactive;
        self.input_id = 0;
        self.active.set(0);
        self.input.clear();
        self.input_items.clear();
        self.filtered_items.clear();
    }

    pub fn update_input(&mut self, input: String) {
        if self.status == CompletionStatus::Inactive {
            return;
        }
        self.input = input;
        self.active.set(0);
        self.filter_items();
    }

    fn all_items(&self) -> im::Vector<ScoredCompletionItem> {
        self.input_items
            .get(&self.input)
            .cloned()
            .filter(|items| !items.is_empty())
            .unwrap_or_else(move || {
                self.input_items.get("").cloned().unwrap_or_default()
            })
    }

    pub fn filter_items(&mut self) {
        self.input_id += 1;
        if self.input.is_empty() {
            self.filtered_items = self.all_items();
            return;
        }

        let mut items: im::Vector<ScoredCompletionItem> = self
            .all_items()
            .iter()
            .filter_map(|i| {
                let filter_text =
                    i.item.filter_text.as_ref().unwrap_or(&i.item.label);
                let shift = i
                    .item
                    .label
                    .match_indices(filter_text)
                    .next()
                    .map(|(shift, _)| shift)
                    .unwrap_or(0);
                if let Some((score, mut indices)) =
                    self.matcher.fuzzy_indices(filter_text, &self.input)
                {
                    if shift > 0 {
                        for idx in indices.iter_mut() {
                            *idx += shift;
                        }
                    }
                    let mut item = i.clone();
                    item.score = score;
                    item.label_score = score;
                    item.indices = indices;
                    if let Some(score) =
                        self.matcher.fuzzy_match(&i.item.label, &self.input)
                    {
                        item.label_score = score;
                    }
                    Some(item)
                } else {
                    None
                }
            })
            .collect();
        items.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.label_score.cmp(&a.label_score))
                .then_with(|| a.item.label.len().cmp(&b.item.label.len()))
        });
        self.filtered_items = items;
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

    pub fn current_item(&self) -> Option<&ScoredCompletionItem> {
        self.filtered_items.get(self.active.get_untracked())
    }
}
