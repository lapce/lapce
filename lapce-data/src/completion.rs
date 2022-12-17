use std::{fmt::Display, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Error;
use druid::{EventCtx, Size, WidgetId};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use itertools::Itertools;
use lapce_core::command::FocusCommand;
use lapce_rpc::{buffer::BufferId, plugin::PluginId};
use lazy_static::lazy_static;
use lsp_types::{CompletionItem, CompletionResponse, Position};
use regex::Regex;

use crate::{config::LapceConfig, list::ListData, proxy::LapceProxy};

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

    fn extract_tabstop(str: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        lazy_static! {
            // Regex for `$...` pattern, where `...` is some number (for example `$1`)
            static ref REGEX_FIRST: Regex = Regex::new(r#"^\$(\d+)"#).unwrap();
            // Regex for `${...}` pattern, where `...` is some number (for example `${1}`)
            static ref REGEX_SECOND: Regex = Regex::new(r#"^\$\{(\d+)\}"#).unwrap();
        }

        let str = &str[pos..];
        if let Some(matched) = REGEX_FIRST.find(str) {
            // SAFETY:
            // * The start index is guaranteed not to exceed the end index, since we
            //   compare with the `$ ...` pattern, and, therefore, the first element
            //   is always equal to the symbol `$`;
            // * The indices are within the bounds of the original slice and lie on
            //   UTF-8 sequence boundaries, since we take the entire slice, with the
            //   exception of the first `$` char which is 1 byte in accordance with
            //   the UTF-8 standard.
            let n = unsafe {
                matched.as_str().get_unchecked(1..).parse::<usize>().ok()?
            };
            let end = pos + matched.end();
            return Some((SnippetElement::Tabstop(n), end));
        }
        if let Some(matched) = REGEX_SECOND.find(str) {
            let matched = matched.as_str();
            // SAFETY:
            // * The start index is guaranteed not to exceed the end index, since we
            //   compare with the `${...}` pattern, and, therefore, the first two elements
            //   are always equal to the `${` and the last one is equal to `}`;
            // * The indices are within the bounds of the original slice and lie on UTF-8
            //   sequence boundaries, since we take the entire slice, with the exception
            //   of the first two `${` and last one `}` chars each of which is 1 byte in
            //   accordance with the UTF-8 standard.
            let n = unsafe {
                matched
                    .get_unchecked(2..matched.len() - 1)
                    .parse::<usize>()
                    .ok()?
            };
            let end = pos + matched.len();
            return Some((SnippetElement::Tabstop(n), end));
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
                    ele += &s[1..2];
                    end += 2;
                    s = &s[2..];
                    continue;
                }
            }
            if escs.contains(&&s[0..1]) {
                break;
            }
            ele += &s[0..1];
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

#[derive(Clone, PartialEq, Eq)]
pub enum CompletionStatus {
    Inactive,
    Started,
}

#[derive(Clone)]
pub struct CompletionData {
    pub id: WidgetId,
    pub scroll_id: WidgetId,
    pub documentation_scroll_id: WidgetId,
    pub request_id: usize,
    pub status: CompletionStatus,
    pub offset: usize,
    pub buffer_id: BufferId,
    pub input: String,
    pub input_items: im::HashMap<String, im::Vector<ScoredCompletionItem>>,
    empty: im::Vector<ScoredCompletionItem>,
    pub completion_list: ListData<ScoredCompletionItem, ()>,
    pub matcher: Arc<SkimMatcherV2>,
    /// The size of the documentation view
    pub documentation_size: Size,
}

impl CompletionData {
    pub fn new(config: Arc<LapceConfig>) -> Self {
        let id = WidgetId::next();
        let mut completion_list = ListData::new(config, id, ());
        // TODO: Make this configurable
        completion_list.max_displayed_items = 15;
        Self {
            id,
            scroll_id: WidgetId::next(),
            documentation_scroll_id: WidgetId::next(),
            request_id: 0,
            offset: 0,
            status: CompletionStatus::Inactive,
            buffer_id: BufferId(0),
            input: "".to_string(),
            input_items: im::HashMap::new(),
            completion_list,
            matcher: Arc::new(SkimMatcherV2::default().ignore_case()),
            // TODO: Make this configurable
            documentation_size: Size::new(400.0, 300.0),
            empty: im::Vector::new(),
        }
    }

    /// Return the number of entries that are displayable
    pub fn len(&self) -> usize {
        self.completion_list.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn current_items(&self) -> &im::Vector<ScoredCompletionItem> {
        if self.input.is_empty() {
            self.all_items()
        } else {
            &self.completion_list.items
        }
    }

    pub fn all_items(&self) -> &im::Vector<ScoredCompletionItem> {
        self.input_items
            .get(&self.input)
            .filter(|items| !items.is_empty())
            .unwrap_or_else(move || self.input_items.get("").unwrap_or(&self.empty))
    }

    pub fn current_item(&self) -> Option<&ScoredCompletionItem> {
        self.completion_list
            .items
            .get(self.completion_list.selected_index)
    }

    pub fn current(&self) -> Option<&str> {
        self.current_item().map(|item| item.item.label.as_str())
    }

    pub fn request(
        &mut self,
        proxy: Arc<LapceProxy>,
        path: PathBuf,
        input: String,
        position: Position,
    ) {
        self.input_items.insert(input.clone(), im::Vector::new());
        proxy
            .proxy_rpc
            .completion(self.request_id, path, input, position);
    }

    pub fn cancel(&mut self) {
        if self.status == CompletionStatus::Inactive {
            return;
        }
        self.status = CompletionStatus::Inactive;
        self.input = "".to_string();
        self.input_items.clear();
        self.completion_list.clear_items();
    }

    pub fn update_input(&mut self, input: String) {
        self.input = input;
        self.completion_list.selected_index = 0;
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
        plugin_id: PluginId,
    ) {
        if self.status == CompletionStatus::Inactive || self.request_id != request_id
        {
            return;
        }

        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
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

        self.input_items.insert(input, items);
        self.filter_items();

        if self.completion_list.selected_index >= self.len() {
            self.completion_list.selected_index = 0;
        }
    }

    pub fn filter_items(&mut self) {
        if self.input.is_empty() {
            self.completion_list.items = self.all_items().clone();
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
        self.completion_list.items = items;
    }

    pub fn run_focus_command(&mut self, ctx: &mut EventCtx, command: &FocusCommand) {
        self.completion_list.run_focus_command(ctx, command);
    }
}

#[derive(Clone, PartialEq)]
pub struct ScoredCompletionItem {
    pub item: CompletionItem,
    pub plugin_id: PluginId,
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

    #[test]
    fn test_extract_tabstop() {
        fn vec_of_tab_elms(s: &str) -> Vec<(usize, usize)> {
            let mut pos = 0;
            let mut vec = Vec::new();
            for char in s.chars() {
                if let Some((elem, end)) = Snippet::extract_tabstop(s, pos) {
                    if let SnippetElement::Tabstop(stop) = elem {
                        vec.push((stop, end));
                    }
                }
                pos += char.len_utf8();
            }
            vec
        }

        let s = "start $1${2:second ${3:third}} $0";
        assert_eq!(&[(1, 8), (0, 33)][..], &vec_of_tab_elms(s)[..]);

        let s = "start ${1}${2:second ${3:third}} $0and ${4}fourth";
        assert_eq!(&[(1, 10), (0, 35), (4, 43)][..], &vec_of_tab_elms(s)[..]);

        let s = "$s$1first${2}$second$3${4}${5}$6and${7}$8fourth$9$$$10$$${11}$$$12$$$13$$${14}$$${15}";
        assert_eq!(
            &[
                (1, 4),
                (2, 13),
                (3, 22),
                (4, 26),
                (5, 30),
                (6, 32),
                (7, 39),
                (8, 41),
                (9, 49),
                (10, 54),
                (11, 61),
                (12, 66),
                (13, 71),
                (14, 78),
                (15, 85)
            ][..],
            &vec_of_tab_elms(s)[..]
        );

        let s = "$s$1ένα${2}$τρία$3${4}${5}$6τέσσερα${7}$8πέντε$9$$$10$$${11}$$$12$$$13$$${14}$$${15}";
        assert_eq!(
            &[
                (1, 4),
                (2, 14),
                (3, 25),
                (4, 29),
                (5, 33),
                (6, 35),
                (7, 53),
                (8, 55),
                (9, 67),
                (10, 72),
                (11, 79),
                (12, 84),
                (13, 89),
                (14, 96),
                (15, 103)
            ][..],
            &vec_of_tab_elms(s)[..]
        );
    }
}
