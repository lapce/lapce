use std::{fmt::Display, sync::Arc};

use anyhow::Error;
use druid::{ExtEventSink, Size, Target, WidgetId};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use itertools::Itertools;
use lapce_core::movement::Movement;
use lapce_rpc::buffer::BufferId;
use lsp_types::{CompletionItem, CompletionResponse, Position};
use regex::Regex;
use std::str::FromStr;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    proxy::LapceProxy,
};

#[derive(Debug)]
pub struct Snippet {
    elements: Vec<SnippetElement>,
}

impl Snippet {
    fn extract_elements(
        s: &str,
        pos: usize,
        escs: Vec<&str>,
        loose_escs: Vec<&str>,
    ) -> (Vec<SnippetElement>, usize) {
        let mut elements = Vec::new();
        let mut pos = pos;
        loop {
            if s.len() == pos {
                break;
            } else if let Some((ele, end)) = Self::extract_tabstop(s, pos) {
                elements.push(ele);
                pos = end;
            } else if let Some((ele, end)) = Self::extract_placeholder(s, pos) {
                elements.push(ele);
                pos = end;
            } else if let Some((ele, end)) =
                Self::extract_text(s, pos, escs.clone(), loose_escs.clone())
            {
                elements.push(ele);
                pos = end;
            } else {
                break;
            }
        }
        (elements, pos)
    }

    fn extract_tabstop(s: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        for re in &[
            Regex::new(r#"^\$(\d+)"#).unwrap(),
            Regex::new(r#"^\$\{(\d+)\}"#).unwrap(),
        ] {
            if let Some(caps) = re.captures(&s[pos..]) {
                let end = pos + re.find(&s[pos..])?.end();
                let m = caps.get(1)?;
                let n = m.as_str().parse::<usize>().ok()?;
                return Some((SnippetElement::Tabstop(n), end));
            }
        }

        None
    }

    fn extract_placeholder(s: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        let re = Regex::new(r#"^\$\{(\d+):(.*?)\}"#).unwrap();
        let end = pos + re.find(&s[pos..])?.end();

        let caps = re.captures(&s[pos..])?;

        let tab = caps.get(1)?.as_str().parse::<usize>().ok()?;

        let m = caps.get(2)?;
        let content = m.as_str();
        if content.is_empty() {
            return Some((
                SnippetElement::PlaceHolder(
                    tab,
                    vec![SnippetElement::Text("".to_string())],
                ),
                end,
            ));
        }
        let (els, pos) =
            Self::extract_elements(s, pos + m.start(), vec!["$", "}", "\\"], vec![]);
        Some((SnippetElement::PlaceHolder(tab, els), pos + 1))
    }

    fn extract_text(
        s: &str,
        pos: usize,
        escs: Vec<&str>,
        loose_escs: Vec<&str>,
    ) -> Option<(SnippetElement, usize)> {
        let mut s = &s[pos..];
        let mut ele = "".to_string();
        let mut end = pos;

        while !s.is_empty() {
            if s.len() >= 2 {
                let esc = &s[..2];
                let mut new_escs = escs.clone();
                new_escs.extend_from_slice(&loose_escs);

                if new_escs
                    .iter()
                    .map(|e| format!("\\{}", e))
                    .any(|x| x == *esc)
                {
                    ele = ele + &s[1..2].to_string();
                    end += 2;
                    s = &s[2..];
                    continue;
                }
            }
            if escs.contains(&&s[0..1]) {
                break;
            }
            ele = ele + &s[0..1].to_string();
            end += 1;
            s = &s[1..];
        }
        if ele.is_empty() {
            return None;
        }
        Some((SnippetElement::Text(ele), end))
    }

    pub fn text(&self) -> String {
        self.elements.iter().map(|e| e.text()).join("")
    }

    pub fn tabs(&self, pos: usize) -> Vec<(usize, (usize, usize))> {
        Self::elements_tabs(&self.elements, pos)
    }

    pub fn elements_tabs(
        elements: &[SnippetElement],
        start: usize,
    ) -> Vec<(usize, (usize, usize))> {
        let mut tabs = Vec::new();
        let mut pos = start;
        for el in elements {
            match el {
                SnippetElement::Text(t) => {
                    pos += t.len();
                }
                SnippetElement::PlaceHolder(tab, els) => {
                    let placeholder_tabs = Self::elements_tabs(els, pos);
                    let end = pos + els.iter().map(|e| e.len()).sum::<usize>();
                    tabs.push((*tab, (pos, end)));
                    tabs.extend_from_slice(&placeholder_tabs);
                    pos = end;
                }
                SnippetElement::Tabstop(tab) => {
                    tabs.push((*tab, (pos, pos)));
                }
            }
        }
        tabs
    }
}

impl FromStr for Snippet {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (elements, _) = Self::extract_elements(s, 0, vec!["$", "\\"], vec!["}"]);
        Ok(Snippet { elements })
    }
}

impl Display for Snippet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = self.elements.iter().map(|e| e.to_string()).join("");
        f.write_str(&text)
    }
}

#[derive(Debug)]
pub enum SnippetElement {
    Text(String),
    PlaceHolder(usize, Vec<SnippetElement>),
    Tabstop(usize),
}

impl SnippetElement {
    pub fn len(&self) -> usize {
        match &self {
            SnippetElement::Text(text) => text.len(),
            SnippetElement::PlaceHolder(_, elements) => {
                elements.iter().map(|e| e.len()).sum()
            }
            SnippetElement::Tabstop(_) => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn text(&self) -> String {
        match &self {
            SnippetElement::Text(t) => t.to_string(),
            SnippetElement::PlaceHolder(_, elements) => {
                elements.iter().map(|e| e.text()).join("")
            }
            SnippetElement::Tabstop(_) => "".to_string(),
        }
    }
}

impl Display for SnippetElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            SnippetElement::Text(text) => f.write_str(text),
            SnippetElement::PlaceHolder(tab, elements) => {
                let elements = elements.iter().map(|e| e.to_string()).join("");
                write!(f, "${{{}:{}}}", tab, elements)
            }
            SnippetElement::Tabstop(tab) => write!(f, "${}", tab),
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum CompletionStatus {
    Inactive,
    Started,
}

#[derive(Clone)]
pub struct CompletionData {
    pub id: WidgetId,
    pub scroll_id: WidgetId,
    pub request_id: usize,
    pub status: CompletionStatus,
    pub offset: usize,
    pub buffer_id: BufferId,
    pub input: String,
    pub index: usize,
    pub input_items: im::HashMap<String, Arc<Vec<ScoredCompletionItem>>>,
    empty: Arc<Vec<ScoredCompletionItem>>,
    pub filtered_items: Arc<Vec<ScoredCompletionItem>>,
    pub matcher: Arc<SkimMatcherV2>,
    pub size: Size,
}

impl CompletionData {
    pub fn new() -> Self {
        Self {
            id: WidgetId::next(),
            scroll_id: WidgetId::next(),
            request_id: 0,
            index: 0,
            offset: 0,
            status: CompletionStatus::Inactive,
            buffer_id: BufferId(0),
            input: "".to_string(),
            input_items: im::HashMap::new(),
            filtered_items: Arc::new(Vec::new()),
            matcher: Arc::new(SkimMatcherV2::default().ignore_case()),
            size: Size::new(400.0, 300.0),
            empty: Arc::new(Vec::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.current_items().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn next(&mut self) {
        self.index = Movement::Down.update_index(self.index, self.len(), 1, true);
    }

    pub fn previous(&mut self) {
        self.index = Movement::Up.update_index(self.index, self.len(), 1, true);
    }

    pub fn current_items(&self) -> &Arc<Vec<ScoredCompletionItem>> {
        if self.input.is_empty() {
            self.all_items()
        } else {
            &self.filtered_items
        }
    }

    pub fn all_items(&self) -> &Arc<Vec<ScoredCompletionItem>> {
        self.input_items
            .get(&self.input)
            .filter(|items| !items.is_empty())
            .unwrap_or_else(move || self.input_items.get("").unwrap_or(&self.empty))
    }

    pub fn current_item(&self) -> &CompletionItem {
        &self.current_items()[self.index].item
    }

    pub fn current(&self) -> &str {
        self.current_items()[self.index].item.label.as_str()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &self,
        proxy: Arc<LapceProxy>,
        request_id: usize,
        buffer_id: BufferId,
        input: String,
        position: Position,
        completion_widget_id: WidgetId,
        event_sink: ExtEventSink,
    ) {
        proxy.get_completion(
            request_id,
            buffer_id,
            position,
            Box::new(move |result| {
                if let Ok(res) = result {
                    if let Ok(resp) =
                        serde_json::from_value::<CompletionResponse>(res)
                    {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateCompletion(
                                request_id, input, resp,
                            ),
                            Target::Widget(completion_widget_id),
                        );
                    }
                }
            }),
        );
    }

    pub fn cancel(&mut self) {
        if self.status == CompletionStatus::Inactive {
            return;
        }
        self.status = CompletionStatus::Inactive;
        self.input = "".to_string();
        self.input_items.clear();
        self.index = 0;
    }

    pub fn update_input(&mut self, input: String) {
        self.input = input;
        self.index = 0;
        if self.status == CompletionStatus::Inactive {
            return;
        }
        self.filter_items();
    }

    pub fn receive(
        &mut self,
        request_id: usize,
        input: String,
        resp: CompletionResponse,
    ) {
        if self.status == CompletionStatus::Inactive || self.request_id != request_id
        {
            return;
        }

        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let items: Vec<ScoredCompletionItem> = items
            .iter()
            .map(|i| ScoredCompletionItem {
                item: i.to_owned(),
                score: 0,
                label_score: 0,
                indices: Vec::new(),
            })
            .collect();

        self.input_items.insert(input, Arc::new(items));
        self.filter_items();
    }

    pub fn filter_items(&mut self) {
        if self.input.is_empty() {
            return;
        }

        let mut items: Vec<ScoredCompletionItem> = self
            .all_items()
            .iter()
            .filter_map(|i| {
                let filter_text =
                    i.item.filter_text.as_ref().unwrap_or(&i.item.label);
                let shift = i.item.label.match_indices(filter_text).next()?.0;
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
        self.filtered_items = Arc::new(items);
    }
}

impl Default for CompletionData {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Completion {}

impl Completion {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Completion {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ScoredCompletionItem {
    pub item: CompletionItem,

    pub score: i64,
    pub label_score: i64,
    pub indices: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet() {
        let s = "start $1${2:second ${3:third}} $0";
        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(s, parsed.to_string());

        let text = "start second third ";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![(1, (6, 6)), (2, (6, 18)), (3, (13, 18)), (0, (19, 19))],
            parsed.tabs(0)
        );
    }
}
