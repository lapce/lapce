use std::{borrow::Cow, fmt::Display, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Error;
use core::fmt;
use druid::{EventCtx, Size, WidgetId};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use lapce_core::command::FocusCommand;
use lapce_rpc::{buffer::BufferId, plugin::PluginId};
use lsp_types::{CompletionItem, CompletionResponse, CompletionTextEdit, Position};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::{
    config::LapceConfig, data::LapceEditorData, document::Document, list::ListData,
    proxy::LapceProxy,
};

#[derive(Debug, PartialEq)]
pub struct Snippet {
    elements: Vec<SnippetElement>,
}

impl Snippet {
    #[inline]
    fn extract_elements(
        s: &str,
        pos: usize,
        escs: &[char],
        loose_escs: &[char],
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
                Self::extract_text(s, pos, escs, loose_escs)
            {
                elements.push(ele);
                pos = end;
            } else {
                break;
            }
        }
        (elements, pos)
    }

    #[inline]
    fn extract_tabstop(str: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        // Regex for `$...` pattern, where `...` is some number (for example `$1`)
        static REGEX_FIRST: Lazy<Regex> =
            Lazy::new(|| Regex::new(r#"^\$(\d+)"#).unwrap());
        // Regex for `${...}` pattern, where `...` is some number (for example `${1}`)
        static REGEX_SECOND: Lazy<Regex> =
            Lazy::new(|| Regex::new(r#"^\$\{(\d+)\}"#).unwrap());

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

    #[inline]
    fn extract_placeholder(s: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        // Regex for `${num:text}` pattern, where text can be empty (for example `${1:first}`
        // and `${2:}`)
        static REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r#"^\$\{(\d+):(.*?)\}"#).unwrap());

        let caps = REGEX.captures(&s[pos..])?;

        let tab = caps.get(1)?.as_str().parse::<usize>().ok()?;

        let m = caps.get(2)?;
        let content = m.as_str();
        if content.is_empty() {
            return Some((
                SnippetElement::PlaceHolder(
                    tab,
                    vec![SnippetElement::Text(String::new())],
                ),
                pos + caps.get(0).unwrap().end(),
            ));
        }
        let (els, pos) =
            Self::extract_elements(s, pos + m.start(), &['$', '}', '\\'], &[]);
        Some((SnippetElement::PlaceHolder(tab, els), pos + 1))
    }

    #[inline]
    fn extract_text(
        s: &str,
        pos: usize,
        escs: &[char],
        loose_escs: &[char],
    ) -> Option<(SnippetElement, usize)> {
        let mut ele = String::new();
        let mut end = pos;
        let mut chars_iter = s[pos..].chars().peekable();

        while let Some(char) = chars_iter.next() {
            if char == '\\' {
                if let Some(&next) = chars_iter.peek() {
                    if escs.iter().chain(loose_escs.iter()).any(|c| *c == next) {
                        chars_iter.next();
                        ele.push(next);
                        end += 1 + next.len_utf8();
                        continue;
                    }
                }
            }
            if escs.contains(&char) {
                break;
            }
            ele.push(char);
            end += char.len_utf8();
        }
        if ele.is_empty() {
            return None;
        }
        Some((SnippetElement::Text(ele), end))
    }

    #[inline]
    pub fn text(&self) -> String {
        let mut buf = String::new();
        self.write_text_to(&mut buf)
            .expect("Snippet::write_text_to function unexpectedly return error");
        buf
    }

    #[inline]
    fn write_text_to<Buffer: fmt::Write>(&self, buf: &mut Buffer) -> fmt::Result {
        for snippet_element in self.elements.iter() {
            snippet_element.write_text_to(buf)?
        }
        fmt::Result::Ok(())
    }

    #[inline]
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

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (elements, _) = Self::extract_elements(s, 0, &['$', '\\'], &['}']);
        Ok(Snippet { elements })
    }
}

impl Display for Snippet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for snippet_element in self.elements.iter() {
            fmt::Display::fmt(snippet_element, f)?;
        }
        fmt::Result::Ok(())
    }
}

#[derive(Debug, PartialEq)]
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

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn text(&self) -> String {
        let mut buf = String::new();
        self.write_text_to(&mut buf)
            .expect("a write_to function returned an error unexpectedly");
        buf
    }

    fn write_text_to<Buffer: fmt::Write>(&self, buf: &mut Buffer) -> fmt::Result {
        match self {
            SnippetElement::Text(text) => buf.write_str(text),
            SnippetElement::PlaceHolder(_, elements) => {
                for child_snippet_elm in elements {
                    // call ourselves recursively
                    child_snippet_elm.write_text_to(buf)?;
                }
                fmt::Result::Ok(())
            }
            SnippetElement::Tabstop(_) => fmt::Result::Ok(()),
        }
    }
}

impl Display for SnippetElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            SnippetElement::Text(text) => f.write_str(text),
            SnippetElement::PlaceHolder(tab, elements) => {
                // Trying to write to the provided buffer in the form "${tab:text}"
                write!(f, "${{{tab}:")?;
                for child_snippet_elm in elements {
                    // call ourselves recursively
                    fmt::Display::fmt(child_snippet_elm, f)?;
                }
                f.write_str("}")
            }
            SnippetElement::Tabstop(tab) => write!(f, "${tab}"),
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

    pub fn run_focus_command(
        &mut self,
        editor: &LapceEditorData,
        doc: &mut Arc<Document>,
        config: &LapceConfig,
        ctx: &mut EventCtx,
        command: &FocusCommand,
    ) {
        let prev_index = self.completion_list.selected_index;
        self.completion_list.run_focus_command(ctx, command);

        if prev_index != self.completion_list.selected_index {
            self.update_document_completion(editor, doc, config);
        }
    }

    /// Update the active document's completion phantomtext, if it is enabled and needed.
    pub fn update_document_completion(
        &self,
        editor: &LapceEditorData,
        doc: &mut Arc<Document>,
        config: &LapceConfig,
    ) {
        if editor.content.is_file() {
            let doc = Arc::make_mut(doc);

            // It isn't enabled at all, so we just clear it (in case it was
            // enabled, and then disalbled) which is cheap with no existing completion lens
            if !config.editor.enable_completion_lens {
                doc.clear_completion();
                return;
            }

            let item = if let Some(item) = self.current_item() {
                if let Some(edit) = &item.item.text_edit {
                    // There's a text edit, which is used rather than the label

                    let text_format = item
                        .item
                        .insert_text_format
                        .unwrap_or(lsp_types::InsertTextFormat::PLAIN_TEXT);

                    match edit {
                        CompletionTextEdit::Edit(edit) => {
                            // The completion offset can be different from the current
                            // cursor offset
                            let completion_offset = self.offset;

                            let offset = editor.cursor.offset();
                            let start_offset =
                                doc.buffer().prev_code_boundary(offset);
                            let edit_start =
                                doc.buffer().offset_of_position(&edit.range.start);

                            // If the start of the edit isn't where the cursor currently is
                            // and it isn't at the start of the completion, then we ignore
                            // it. This captures most cases that we want, even if it
                            // skips over some easily displayeable edits.
                            if start_offset != edit_start
                                && completion_offset != edit_start
                            {
                                None
                            } else {
                                match text_format {
                                    lsp_types::InsertTextFormat::PLAIN_TEXT => {
                                        // This isn't entirely correcty because it assumes that the position is `{start,end}_offset` when it may not necessarily be.
                                        let text = &edit.new_text;

                                        Some(Cow::Borrowed(text))
                                    }
                                    lsp_types::InsertTextFormat::SNIPPET => {
                                        // TODO: Don't unwrap
                                        let snippet =
                                            Snippet::from_str(&edit.new_text)
                                                .unwrap();

                                        let text = snippet.text();
                                        Some(Cow::Owned(text))
                                    }
                                    _ => None,
                                }
                            }
                        }
                        CompletionTextEdit::InsertAndReplace(_) => None,
                    }
                } else {
                    // There's no specific text edit, so we just use the label displayed in
                    // the completion list
                    let label = &item.item.label;

                    Some(Cow::Borrowed(label))
                }
            } else {
                None
            };

            // We strip the prefix of the currently inputted text off of it, so that
            // 'p' with a completion of `println` only sets the completion to 'rintln'.
            // If it does not include the prefix in the right position, then we don't
            // display it. There's probably nicer ways to do this, but that's how it works
            // in other editors that impl it atm and it is simpler to implement.
            let item = item.as_ref().and_then(|x| x.strip_prefix(&self.input));
            // Get only the first line of text, because Lapce does not currently support
            // multi-line phantom-text.
            // TODO: Once Lapce supports multi-line phantom text, then this can be
            // removed/modified to support it.
            let item = item.map(|x| x.lines().next().unwrap_or(x));

            // If the item we got is different from the one stored, either in content or
            // existence, then we update the document's stored completion text.
            if doc.completion() != item {
                if let Some(item) = item {
                    let offset = self.offset + self.input.len();
                    let (line, col) = doc.buffer().offset_to_line_col(offset);
                    doc.set_completion(item.to_string(), line, col);
                } else {
                    doc.clear_completion();
                }
            }
        }
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
        use SnippetElement::*;

        let s = "start $1${2:second ${3:third}} $0";
        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(s, parsed.to_string());

        let text = "start second third ";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![(1, (6, 6)), (2, (6, 18)), (3, (13, 18)), (0, (19, 19))],
            parsed.tabs(0)
        );

        let s = "start ${1}${2:second ${3:third}} $0and ${4}fourth";

        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(
            "start $1${2:second ${3:third}} $0and $4fourth",
            parsed.to_string()
        );

        let text = "start second third and fourth";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![
                (1, (6, 6)),
                (2, (6, 18)),
                (3, (13, 18)),
                (0, (19, 19)),
                (4, (23, 23))
            ],
            parsed.tabs(0)
        );

        let s = "${1:first $6${2:second ${7}${3:third ${4:fourth ${5:fifth}}}}}";

        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(
            "${1:first $6${2:second $7${3:third ${4:fourth ${5:fifth}}}}}",
            parsed.to_string()
        );

        let text = "first second third fourth fifth";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![
                (1, (0, 31)),
                (6, (6, 6)),
                (2, (6, 31)),
                (7, (13, 13)),
                (3, (13, 31)),
                (4, (19, 31)),
                (5, (26, 31))
            ],
            parsed.tabs(0)
        );

        assert_eq!(
            Snippet {
                elements: vec![PlaceHolder(
                    1,
                    vec![
                        Text("first ".into()),
                        Tabstop(6),
                        PlaceHolder(
                            2,
                            vec![
                                Text("second ".into()),
                                Tabstop(7),
                                PlaceHolder(
                                    3,
                                    vec![
                                        Text("third ".into()),
                                        PlaceHolder(
                                            4,
                                            vec![
                                                Text("fourth ".into()),
                                                PlaceHolder(
                                                    5,
                                                    vec![Text("fifth".into())]
                                                )
                                            ]
                                        )
                                    ]
                                )
                            ]
                        )
                    ]
                )]
            },
            parsed
        );

        let s = "\\$1 start \\$2$3${4}${5:some text\\${6:third\\} $7}";

        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(
            "$1 start $2$3$4${5:some text${6:third} $7}",
            parsed.to_string()
        );

        let text = "$1 start $2some text${6:third} ";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![(3, (11, 11)), (4, (11, 11)), (5, (11, 31)), (7, (31, 31))],
            parsed.tabs(0)
        );

        assert_eq!(
            Snippet {
                elements: vec![
                    Text("$1 start $2".into()),
                    Tabstop(3),
                    Tabstop(4),
                    PlaceHolder(
                        5,
                        vec![Text("some text${6:third} ".into()), Tabstop(7)]
                    )
                ]
            },
            parsed
        );
    }

    #[test]
    fn test_extract_tabstop() {
        fn vec_of_tab_elms(s: &str) -> Vec<(usize, usize)> {
            let mut pos = 0;
            let mut vec = Vec::new();
            for char in s.chars() {
                if let Some((SnippetElement::Tabstop(stop), end)) =
                    Snippet::extract_tabstop(s, pos)
                {
                    vec.push((stop, end));
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

    #[test]
    fn test_extract_placeholder() {
        use super::SnippetElement::*;
        let s1 = "${1:first ${2:second ${3:third ${4:fourth ${5:fifth}}}}}";

        assert_eq!(
            (
                PlaceHolder(
                    1,
                    vec![
                        Text("first ".into()),
                        PlaceHolder(
                            2,
                            vec![
                                Text("second ".into()),
                                PlaceHolder(
                                    3,
                                    vec![
                                        Text("third ".into()),
                                        PlaceHolder(
                                            4,
                                            vec![
                                                Text("fourth ".into()),
                                                PlaceHolder(
                                                    5,
                                                    vec![Text("fifth".into())]
                                                )
                                            ]
                                        )
                                    ]
                                )
                            ]
                        )
                    ]
                ),
                56
            ),
            Snippet::extract_placeholder(s1, 0).unwrap()
        );

        let s1 = "${1:first}${2:second}${3:third }${4:fourth ${5:fifth}}}}}";
        assert_eq!(
            (PlaceHolder(1, vec![Text("first".to_owned())]), 10),
            Snippet::extract_placeholder(s1, 0).unwrap()
        );
        assert_eq!(
            (PlaceHolder(2, vec![Text("second".to_owned())]), 21),
            Snippet::extract_placeholder(s1, 10).unwrap()
        );
        assert_eq!(
            (PlaceHolder(3, vec![Text("third ".to_owned())]), 32),
            Snippet::extract_placeholder(s1, 21).unwrap()
        );

        assert_eq!(
            (
                PlaceHolder(
                    4,
                    vec![
                        Text("fourth ".into()),
                        PlaceHolder(5, vec![Text("fifth".into())])
                    ]
                ),
                54
            ),
            Snippet::extract_placeholder(s1, 32).unwrap()
        );
    }

    #[test]
    fn test_extract_text() {
        use SnippetElement::*;

        // 1. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";
        let (snip_elm, end) = Snippet::extract_text(s, 0, &['$'], &[]).unwrap();
        assert_eq!((Text("start ".to_owned()), 6), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 8), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("{2:second ".to_owned()), 19), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("{3:third}} ".to_owned()), 31), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 33), (snip_elm, end));

        // 2. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";

        let (snip_elm, end) = Snippet::extract_text(s, 0, &['{'], &[]).unwrap();
        assert_eq!((Text("start $1$".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['{'], &[]).unwrap();
        assert_eq!((Text("2:second $".to_owned()), 20), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['{'], &[]).unwrap();
        assert_eq!((Text("3:third}} $0".to_owned()), 33), (snip_elm, end));

        // 3. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";

        let (snip_elm, end) = Snippet::extract_text(s, 0, &['}'], &[]).unwrap();
        assert_eq!(
            (Text("start $1${2:second ${3:third".to_owned()), 28),
            (snip_elm, end)
        );

        assert_eq!(None, Snippet::extract_text(s, end + 1, &['}'], &[]));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['}'], &[]).unwrap();
        assert_eq!((Text(" $0".to_owned()), 33), (snip_elm, end));

        // 4. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";

        let (snip_elm, end) = Snippet::extract_text(s, 0, &['\\'], &[]).unwrap();
        assert_eq!((Text(s.to_owned()), 33), (snip_elm, end));

        // 5. ====================================================================================

        let s = "start \\$1${2:second \\${3:third}} $0";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['$', '\\'], &[]).unwrap();
        assert_eq!((Text("start $1".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &[]).unwrap();
        assert_eq!(
            (Text("{2:second ${3:third}} ".to_owned()), 33),
            (snip_elm, end)
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 35), (snip_elm, end));

        // 6. ====================================================================================

        let s = "\\{start $1${2:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['{', '\\'], &[]).unwrap();
        assert_eq!((Text("{start $1$".to_owned()), 11), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['{', '\\'], &[]).unwrap();
        assert_eq!(
            (Text("2:second ${3:third}} $0}".to_owned()), 37),
            (snip_elm, end)
        );

        // 7. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text("{start $1${2".to_owned()), 12), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second $".to_owned()), 22), (snip_elm, end));

        assert_eq!(None, Snippet::extract_text(s, end, &['}', '\\'], &[]));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 31), (snip_elm, end));

        assert_eq!(None, Snippet::extract_text(s, end + 1, &['}', '\\'], &[]));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text(" $0".to_owned()), 36), (snip_elm, end));

        // 8. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("{start ".to_owned()), 7), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("1".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("{2}:second ".to_owned()), 21), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}'])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("{3:third}} ".to_owned()), 34), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("0}".to_owned()), 37), (snip_elm, end));

        // 9. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{start ".to_owned()), 7), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{2".to_owned()), 12), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second ".to_owned()), 21), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 31), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 34), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 36), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[])
        );

        // 10. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        assert_eq!(
            None,
            Snippet::extract_text(s, 0, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("start ".to_owned()), 7), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 9), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("2".to_owned()), 12), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second ".to_owned()), 21), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 31), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 34), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 36), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        // 11. ====================================================================================

        let s = "{start\\\\ $1${2}:second\\ $\\{3:third}} $0}";

        assert_eq!(
            None,
            Snippet::extract_text(s, 0, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("start\\ ".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 11), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("2".to_owned()), 14), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second".to_owned()), 22), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 24), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 34), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 37), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 39), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );
    }
}
