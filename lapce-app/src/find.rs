use std::cmp::{max, min};

use floem::{
    prelude::SignalTrack,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use lapce_core::{
    selection::{SelRegion, Selection},
    word::WordCursor,
};
use lapce_xi_rope::{
    Cursor, Interval, Rope,
    find::{CaseMatching, find, is_multiline_regex},
};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

const REGEX_SIZE_LIMIT: usize = 1000000;

/// Indicates what changed in the find state.
#[derive(PartialEq, Debug, Clone)]
pub enum FindProgress {
    /// Incremental find is done/not running.
    Ready,

    /// The find process just started.
    Started,

    /// Incremental find is in progress. Keeps tracked of already searched range.
    InProgress(Selection),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FindStatus {
    /// Identifier for the current search query.
    id: usize,

    /// The current search query.
    chars: Option<String>,

    /// Whether the active search is case matching.
    case_sensitive: Option<bool>,

    /// Whether the search query is considered as regular expression.
    is_regex: Option<bool>,

    /// Query only matches whole words.
    whole_words: Option<bool>,

    /// Total number of matches.
    matches: usize,

    /// Line numbers which have find results.
    lines: Vec<usize>,
}

#[derive(Clone)]
pub struct FindSearchString {
    pub content: String,
    pub regex: Option<Regex>,
}

#[derive(Clone)]
pub struct Find {
    pub rev: RwSignal<u64>,
    /// If the find is shown
    pub visual: RwSignal<bool>,
    /// The currently active search string.
    pub search_string: RwSignal<Option<FindSearchString>>,
    /// The case matching setting for the currently active search.
    pub case_matching: RwSignal<CaseMatching>,
    /// Query matches only whole words.
    pub whole_words: RwSignal<bool>,
    /// The search query should be considered as regular expression.
    pub is_regex: RwSignal<bool>,
    /// replace editor is shown
    pub replace_active: RwSignal<bool>,
    /// replace editor is focused
    pub replace_focus: RwSignal<bool>,
    /// Triggered by changes in the search string
    pub triggered_by_changes: RwSignal<bool>,
}

impl Find {
    pub fn new(cx: Scope) -> Self {
        let find = Self {
            rev: cx.create_rw_signal(0),
            visual: cx.create_rw_signal(false),
            search_string: cx.create_rw_signal(None),
            case_matching: cx.create_rw_signal(CaseMatching::CaseInsensitive),
            whole_words: cx.create_rw_signal(false),
            is_regex: cx.create_rw_signal(false),
            replace_active: cx.create_rw_signal(false),
            replace_focus: cx.create_rw_signal(false),
            triggered_by_changes: cx.create_rw_signal(false),
        };

        {
            let find = find.clone();
            cx.create_effect(move |_| {
                find.is_regex.with(|_| ());
                let s = find.search_string.with_untracked(|s| {
                    if let Some(s) = s.as_ref() {
                        s.content.clone()
                    } else {
                        "".to_string()
                    }
                });
                if !s.is_empty() {
                    find.set_find(&s);
                }
            });
        }

        {
            let find = find.clone();
            cx.create_effect(move |_| {
                find.search_string.track();
                find.case_matching.track();
                find.whole_words.track();
                find.rev.update(|rev| {
                    *rev += 1;
                });
            });
        }

        find
    }

    /// Returns `true` if case sensitive, otherwise `false`
    pub fn case_sensitive(&self, tracked: bool) -> bool {
        match if tracked {
            self.case_matching.get()
        } else {
            self.case_matching.get_untracked()
        } {
            CaseMatching::Exact => true,
            CaseMatching::CaseInsensitive => false,
        }
    }

    /// Sets find case sensitivity.
    pub fn set_case_sensitive(&self, case_sensitive: bool) {
        if self.case_sensitive(false) == case_sensitive {
            return;
        }

        let case_matching = if case_sensitive {
            CaseMatching::Exact
        } else {
            CaseMatching::CaseInsensitive
        };
        self.case_matching.set(case_matching);
    }

    pub fn set_find(&self, search_string: &str) {
        if search_string.is_empty() {
            self.search_string.set(None);
            return;
        }

        if !self.visual.get_untracked() {
            self.visual.set(true);
        }

        let is_regex = self.is_regex.get_untracked();

        let search_string_unchanged = self.search_string.with_untracked(|search| {
            if let Some(s) = search {
                s.content == search_string && s.regex.is_some() == is_regex
            } else {
                false
            }
        });

        if search_string_unchanged {
            return;
        }

        // create regex from untrusted input
        let regex = match is_regex {
            false => None,
            true => RegexBuilder::new(search_string)
                .size_limit(REGEX_SIZE_LIMIT)
                .case_insensitive(!self.case_sensitive(false))
                .build()
                .ok(),
        };
        self.triggered_by_changes.set(true);
        self.search_string.set(Some(FindSearchString {
            content: search_string.to_string(),
            regex,
        }));
    }

    pub fn next(
        &self,
        text: &Rope,
        offset: usize,
        reverse: bool,
        wrap: bool,
    ) -> Option<(usize, usize)> {
        if !self.visual.get_untracked() {
            self.visual.set(true);
        }
        let case_matching = self.case_matching.get_untracked();
        let whole_words = self.whole_words.get_untracked();
        self.search_string.with_untracked(
            |search_string| -> Option<(usize, usize)> {
                let search_string = search_string.as_ref()?;
                if !reverse {
                    let mut raw_lines = text.lines_raw(offset..text.len());
                    let mut find_cursor = Cursor::new(text, offset);
                    while let Some(start) = find(
                        &mut find_cursor,
                        &mut raw_lines,
                        case_matching,
                        &search_string.content,
                        search_string.regex.as_ref(),
                    ) {
                        let end = find_cursor.pos();

                        if whole_words
                            && !Self::is_matching_whole_words(text, start, end)
                        {
                            raw_lines =
                                text.lines_raw(find_cursor.pos()..text.len());
                            continue;
                        }
                        raw_lines = text.lines_raw(find_cursor.pos()..text.len());

                        if start > offset {
                            return Some((start, end));
                        }
                    }
                    if wrap {
                        let mut raw_lines = text.lines_raw(0..offset);
                        let mut find_cursor = Cursor::new(text, 0);
                        while let Some(start) = find(
                            &mut find_cursor,
                            &mut raw_lines,
                            case_matching,
                            &search_string.content,
                            search_string.regex.as_ref(),
                        ) {
                            let end = find_cursor.pos();

                            if whole_words
                                && !Self::is_matching_whole_words(text, start, end)
                            {
                                raw_lines =
                                    text.lines_raw(find_cursor.pos()..offset);
                                continue;
                            }
                            return Some((start, end));
                        }
                    }
                } else {
                    let mut raw_lines = text.lines_raw(0..offset);
                    let mut find_cursor = Cursor::new(text, 0);
                    let mut regions = Vec::new();
                    while let Some(start) = find(
                        &mut find_cursor,
                        &mut raw_lines,
                        case_matching,
                        &search_string.content,
                        search_string.regex.as_ref(),
                    ) {
                        let end = find_cursor.pos();
                        raw_lines = text.lines_raw(find_cursor.pos()..offset);
                        if whole_words
                            && !Self::is_matching_whole_words(text, start, end)
                        {
                            continue;
                        }
                        if start < offset {
                            regions.push((start, end));
                        }
                    }
                    if !regions.is_empty() {
                        return Some(regions[regions.len() - 1]);
                    }
                    if wrap {
                        let mut raw_lines = text.lines_raw(offset..text.len());
                        let mut find_cursor = Cursor::new(text, offset);
                        let mut regions = Vec::new();
                        while let Some(start) = find(
                            &mut find_cursor,
                            &mut raw_lines,
                            case_matching,
                            &search_string.content,
                            search_string.regex.as_ref(),
                        ) {
                            let end = find_cursor.pos();

                            if whole_words
                                && !Self::is_matching_whole_words(text, start, end)
                            {
                                raw_lines =
                                    text.lines_raw(find_cursor.pos()..text.len());
                                continue;
                            }
                            raw_lines =
                                text.lines_raw(find_cursor.pos()..text.len());

                            if start > offset {
                                regions.push((start, end));
                            }
                        }
                        if !regions.is_empty() {
                            return Some(regions[regions.len() - 1]);
                        }
                    }
                }
                None
            },
        )
    }

    /// Checks if the start and end of a match is matching whole words.
    fn is_matching_whole_words(text: &Rope, start: usize, end: usize) -> bool {
        let mut word_end_cursor = WordCursor::new(text, end - 1);
        let mut word_start_cursor = WordCursor::new(text, start + 1);

        if word_start_cursor.prev_code_boundary() != start {
            return false;
        }

        if word_end_cursor.next_code_boundary() != end {
            return false;
        }

        true
    }

    /// Returns `true` if the search query is a multi-line regex.
    pub fn is_multiline_regex(&self) -> bool {
        self.search_string.with_untracked(|search| {
            if let Some(search) = search.as_ref() {
                search.regex.is_some() && is_multiline_regex(&search.content)
            } else {
                false
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn find(
        text: &Rope,
        search: &FindSearchString,
        start: usize,
        end: usize,
        case_matching: CaseMatching,
        whole_words: bool,
        include_slop: bool,
        occurrences: &mut Selection,
    ) {
        let search_string = &search.content;

        let slop = if include_slop {
            search.content.len() * 2
        } else {
            0
        };

        // expand region to be able to find occurrences around the region's edges
        let expanded_start = max(start, slop) - slop;
        let expanded_end = min(end + slop, text.len());
        let from = text
            .at_or_prev_codepoint_boundary(expanded_start)
            .unwrap_or(0);
        let to = text
            .at_or_next_codepoint_boundary(expanded_end)
            .unwrap_or_else(|| text.len());
        let mut to_cursor = Cursor::new(text, to);
        let _ = to_cursor.next_leaf();

        let sub_text = text.subseq(Interval::new(0, to_cursor.pos()));
        let mut find_cursor = Cursor::new(&sub_text, from);

        let mut raw_lines = text.lines_raw(from..to);

        while let Some(start) = find(
            &mut find_cursor,
            &mut raw_lines,
            case_matching,
            search_string,
            search.regex.as_ref(),
        ) {
            let end = find_cursor.pos();

            if whole_words && !Self::is_matching_whole_words(text, start, end) {
                raw_lines = text.lines_raw(find_cursor.pos()..to);
                continue;
            }

            let region = SelRegion::new(start, end, None);
            let (_, e) = occurrences.add_range_distinct(region);
            // in case of ambiguous search results (e.g. search "aba" in "ababa"),
            // the search result closer to the beginning of the file wins
            if e != end {
                // Skip the search result and keep the occurrence that is closer to
                // the beginning of the file. Re-align the cursor to the kept
                // occurrence
                find_cursor.set(e);
                raw_lines = text.lines_raw(find_cursor.pos()..to);
                continue;
            }

            // in case current cursor matches search result (for example query a* matches)
            // all cursor positions, then cursor needs to be increased so that search
            // continues at next position. Otherwise, search will result in overflow since
            // search will always repeat at current cursor position.
            if start == end {
                // determine whether end of text is reached and stop search or increase
                // cursor manually
                if end + 1 >= text.len() {
                    break;
                } else {
                    find_cursor.set(end + 1);
                }
            }

            // update line iterator so that line starts at current cursor position
            raw_lines = text.lines_raw(find_cursor.pos()..to);
        }
    }

    /// Execute the search on the provided text in the range provided by `start` and `end`.
    pub fn update_find(
        &self,
        text: &Rope,
        start: usize,
        end: usize,
        include_slop: bool,
        occurrences: &mut Selection,
    ) {
        if self.search_string.with_untracked(|search| search.is_none()) {
            return;
        }

        let search = self.search_string.get_untracked().unwrap();
        let search_string = &search.content;
        // extend the search by twice the string length (twice, because case matching may increase
        // the length of an occurrence)
        let slop = if include_slop {
            search.content.len() * 2
        } else {
            0
        };

        // expand region to be able to find occurrences around the region's edges
        let expanded_start = max(start, slop) - slop;
        let expanded_end = min(end + slop, text.len());
        let from = text
            .at_or_prev_codepoint_boundary(expanded_start)
            .unwrap_or(0);
        let to = text
            .at_or_next_codepoint_boundary(expanded_end)
            .unwrap_or_else(|| text.len());
        let mut to_cursor = Cursor::new(text, to);
        let _ = to_cursor.next_leaf();

        let sub_text = text.subseq(Interval::new(0, to_cursor.pos()));
        let mut find_cursor = Cursor::new(&sub_text, from);

        let mut raw_lines = text.lines_raw(from..to);

        let case_matching = self.case_matching.get_untracked();
        let whole_words = self.whole_words.get_untracked();
        while let Some(start) = find(
            &mut find_cursor,
            &mut raw_lines,
            case_matching,
            search_string,
            search.regex.as_ref(),
        ) {
            let end = find_cursor.pos();

            if whole_words && !Self::is_matching_whole_words(text, start, end) {
                raw_lines = text.lines_raw(find_cursor.pos()..to);
                continue;
            }

            let region = SelRegion::new(start, end, None);
            let (_, e) = occurrences.add_range_distinct(region);
            // in case of ambiguous search results (e.g. search "aba" in "ababa"),
            // the search result closer to the beginning of the file wins
            if e != end {
                // Skip the search result and keep the occurrence that is closer to
                // the beginning of the file. Re-align the cursor to the kept
                // occurrence
                find_cursor.set(e);
                raw_lines = text.lines_raw(find_cursor.pos()..to);
                continue;
            }

            // in case current cursor matches search result (for example query a* matches)
            // all cursor positions, then cursor needs to be increased so that search
            // continues at next position. Otherwise, search will result in overflow since
            // search will always repeat at current cursor position.
            if start == end {
                // determine whether end of text is reached and stop search or increase
                // cursor manually
                if end + 1 >= text.len() {
                    break;
                } else {
                    find_cursor.set(end + 1);
                }
            }

            // update line iterator so that line starts at current cursor position
            raw_lines = text.lines_raw(find_cursor.pos()..to);
        }
    }
}

#[derive(Clone)]
pub struct FindResult {
    pub find_rev: RwSignal<u64>,
    pub progress: RwSignal<FindProgress>,
    pub occurrences: RwSignal<Selection>,
    pub search_string: RwSignal<Option<FindSearchString>>,
    pub case_matching: RwSignal<CaseMatching>,
    pub whole_words: RwSignal<bool>,
    pub is_regex: RwSignal<bool>,
}

impl FindResult {
    pub fn new(cx: Scope) -> Self {
        Self {
            find_rev: cx.create_rw_signal(0),
            progress: cx.create_rw_signal(FindProgress::Started),
            occurrences: cx.create_rw_signal(Selection::new()),
            search_string: cx.create_rw_signal(None),
            case_matching: cx.create_rw_signal(CaseMatching::Exact),
            whole_words: cx.create_rw_signal(false),
            is_regex: cx.create_rw_signal(false),
        }
    }

    pub fn reset(&self) {
        self.progress.set(FindProgress::Started);
    }
}
