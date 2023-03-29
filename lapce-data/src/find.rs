use std::cmp::{max, min};

use lapce_core::{
    selection::{InsertDrift, SelRegion, Selection},
    word::WordCursor,
};
use lapce_xi_rope::{
    delta::DeltaRegion,
    find::{find, is_multiline_regex, CaseMatching},
    Cursor, Interval, LinesMetric, Metric, Rope, RopeDelta,
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
pub struct Find {
    /// Uniquely identifies this search query.
    id: usize,

    /// The occurrences, which determine the highlights, have been updated.
    hls_dirty: bool,

    pub visual: bool,

    /// The currently active search string.
    pub search_string: Option<String>,

    /// The case matching setting for the currently active search.
    pub case_matching: CaseMatching,

    /// The search query should be considered as regular expression.
    pub regex: Option<Regex>,

    /// Query matches only whole words.
    pub whole_words: bool,

    /// The set of all known find occurrences (highlights).
    occurrences: Selection,
}

impl Find {
    pub fn new(id: usize) -> Find {
        Find {
            id,
            hls_dirty: true,
            search_string: None,
            case_matching: CaseMatching::CaseInsensitive,
            regex: None,
            whole_words: false,
            visual: false,
            occurrences: Selection::new(),
        }
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn occurrences(&self) -> &Selection {
        &self.occurrences
    }

    pub fn hls_dirty(&self) -> bool {
        self.hls_dirty
    }

    pub fn set_hls_dirty(&mut self, is_dirty: bool) {
        self.hls_dirty = is_dirty
    }

    /// Returns `true` if case sensitive and `false` if not.
    pub fn case_sensitive(&self) -> bool {
        match self.case_matching {
            CaseMatching::Exact => true,
            CaseMatching::CaseInsensitive => false,
        }
    }

    /// Reverses the current case setting and returns the reversed value.
    /// `true` means case sensitive and `false` means case insensitive.
    pub fn toggle_case_sensitive(&mut self) -> bool {
        let toggled = !self.case_sensitive();
        self.set_case_sensitive(toggled);
        toggled
    }

    /// Returns `true` if the search query is a multi-line regex.
    pub(crate) fn is_multiline_regex(&self) -> bool {
        self.regex.is_some()
            && is_multiline_regex(self.search_string.as_ref().unwrap())
    }

    /// Clears the search and removes all occurrences.
    pub fn unset(&mut self) {
        self.search_string = None;
        self.occurrences = Selection::new();
        self.hls_dirty = true;
    }

    /// Sets the case sensitivity setting.
    pub fn set_case_sensitive(&mut self, case_sensitive: bool) {
        self.case_matching = if case_sensitive {
            CaseMatching::Exact
        } else {
            CaseMatching::CaseInsensitive
        };
    }

    /// Clears old results and sets new search parameters.
    /// Note: does not reset or change case sensitivity.
    pub fn set_find(
        &mut self,
        search_string: &str,
        is_regex: bool,
        whole_words: bool,
    ) {
        self.unset();

        self.search_string = Some(search_string.to_string());
        self.whole_words = whole_words;

        // create regex from untrusted input
        self.regex = match is_regex {
            false => None,
            true => RegexBuilder::new(search_string)
                .size_limit(REGEX_SIZE_LIMIT)
                .case_insensitive(!self.case_sensitive())
                .build()
                .ok(),
        };
    }

    pub fn next(
        &self,
        text: &Rope,
        offset: usize,
        reverse: bool,
        wrap: bool,
    ) -> Option<(usize, usize)> {
        let search_string = self.search_string.as_ref()?;

        if !reverse {
            let mut raw_lines = text.lines_raw(offset..text.len());
            let mut find_cursor = Cursor::new(text, offset);

            while let Some(start) = find(
                &mut find_cursor,
                &mut raw_lines,
                self.case_matching,
                search_string,
                self.regex.as_ref(),
            ) {
                let end = find_cursor.pos();

                if self.whole_words && !is_matching_whole_words(text, start, end) {
                    raw_lines = text.lines_raw(find_cursor.pos()..text.len());
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
                    self.case_matching,
                    search_string,
                    self.regex.as_ref(),
                ) {
                    let end = find_cursor.pos();

                    if self.whole_words && !is_matching_whole_words(text, start, end)
                    {
                        raw_lines = text.lines_raw(find_cursor.pos()..offset);
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
                self.case_matching,
                search_string,
                self.regex.as_ref(),
            ) {
                let end = find_cursor.pos();
                raw_lines = text.lines_raw(find_cursor.pos()..offset);

                if self.whole_words && !is_matching_whole_words(text, start, end) {
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
                    self.case_matching,
                    search_string,
                    self.regex.as_ref(),
                ) {
                    let end = find_cursor.pos();

                    if self.whole_words && !is_matching_whole_words(text, start, end)
                    {
                        raw_lines = text.lines_raw(find_cursor.pos()..text.len());
                        continue;
                    }

                    raw_lines = text.lines_raw(find_cursor.pos()..text.len());

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
    }

    /// Performs a search on the specified text within the range defined by `start` and `end`.
    pub fn update_find(
        &mut self,
        text: &Rope,
        start: usize,
        end: usize,
        include_slop: bool,
    ) {
        let search_string = if let Some(s) = self.search_string.as_ref() {
            s
        } else {
            return;
        };

        // extend the search by twice the string length (twice, because case matching may increase
        // the length of an occurrence)
        let slop = if include_slop {
            search_string.len() * 2
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
            self.case_matching,
            search_string,
            self.regex.as_ref(),
        ) {
            let end = find_cursor.pos();

            if self.whole_words && !is_matching_whole_words(text, start, end) {
                raw_lines = text.lines_raw(find_cursor.pos()..to);
                continue;
            }

            self.occurrences
                .add_region(SelRegion::new(start, end, None));

            // in case of ambiguous search results (e.g. search "aba" in "ababa"),
            // the search result closer to the beginning of the file wins
            //             if e != end {
            //                 // Skip the search result and keep the occurrence that is closer to
            //                 // the beginning of the file. Re-align the cursor to the kept
            //                 // occurrence
            //                 find_cursor.set(e);
            //                 raw_lines = text.lines_raw(find_cursor.pos()..to);
            //                 continue;
            //             }
            //
            //             // in case current cursor matches search result (for example query a* matches)
            //             // all cursor positions, then cursor needs to be increased so that search
            //             // continues at next position. Otherwise, search will result in overflow since
            //             // search will always repeat at current cursor position.
            //             if start == end {
            //                 // determine whether end of text is reached and stop search or increase
            //                 // cursor manually
            //                 if end + 1 >= text.len() {
            //                     break;
            //                 } else {
            //                     find_cursor.set(end + 1);
            //                 }
            //             }

            // update line iterator so that line starts at current cursor position
            raw_lines = text.lines_raw(find_cursor.pos()..to);
        }

        self.hls_dirty = true;
    }

    pub fn update_highlights(&mut self, text: &Rope, delta: &RopeDelta) {
        // update search highlights for changed regions
        if self.search_string.is_some() {
            // invalidate occurrences around deletion positions
            for DeltaRegion {
                old_offset, len, ..
            } in delta.iter_deletions()
            {
                self.occurrences.delete_range(old_offset, old_offset + len);
            }

            self.occurrences =
                self.occurrences
                    .apply_delta(delta, false, InsertDrift::Default);

            // invalidate occurrences around insert positions
            for DeltaRegion {
                new_offset, len, ..
            } in delta.iter_inserts()
            {
                // also invalidate previous occurrence since it might expand after insertion
                // eg. for regex .* every insertion after match will be part of match
                self.occurrences
                    .delete_range(new_offset.saturating_sub(1), new_offset + len);
            }

            // update find for the whole delta and everything after
            let (iv, new_len) = delta.summary();

            // get last valid occurrence that was unaffected by the delta
            let start = match self.occurrences.regions_in_range(0, iv.start()).last()
            {
                Some(reg) => reg.end,
                None => 0,
            };

            // invalidate all search results from the point of the last valid search result until ...
            let is_multiline =
                LinesMetric::next(self.search_string.as_ref().unwrap(), 0).is_some();

            if is_multiline || self.is_multiline_regex() {
                // ... the end of the file
                self.occurrences.delete_range(iv.start(), text.len());
                self.update_find(text, start, text.len(), false);
            } else {
                // ... the end of the line including line break
                let mut cursor = Cursor::new(text, iv.end() + new_len);

                let end_of_line = match cursor.next::<LinesMetric>() {
                    Some(end) => end,
                    None if cursor.pos() == text.len() => cursor.pos(),
                    _ => return,
                };

                self.occurrences.delete_range(iv.start(), end_of_line);
                self.update_find(text, start, end_of_line, false);
            }
        }
    }

    /// Returns the occurrence closest to the given selection `sel`. If the search is reversed, then
    /// the occurrence closest to the start of the selection is returned. `wrapped` indicates that
    /// if the end of the text is reached then the search continues from the start.
    pub fn next_occurrence(
        &self,
        text: &Rope,
        reverse: bool,
        wrapped: bool,
        sel: &Selection,
    ) -> Option<SelRegion> {
        if self.occurrences.is_empty() {
            return None;
        }

        let (sel_start, sel_end) = match sel.last() {
            Some(last) => {
                // if the last selection is a caret, then allow the current position to be part of the occurrence
                // if the last selection is not a caret, then continue searching after the caret
                (last.min(), last.max() + (!last.is_caret()) as usize)
            }
            _ => (0, 0),
        };

        let next_occurrence = if reverse {
            sel_start.checked_sub(1).and_then(|search_end| {
                self.occurrences.full_regions_in_range(0, search_end).last()
            })
        } else {
            self.occurrences
                .full_regions_in_range(sel_end, text.len())
                .first()
        };

        if next_occurrence.is_none() && !wrapped {
            let mut filtered = self
                .occurrences
                .full_regions_in_range(0, text.len())
                .iter()
                .filter(|occ| {
                    sel.full_regions_in_range(occ.min(), occ.max()).is_empty()
                });

            // get previous or next unselected occurrence
            if reverse {
                filtered.last()
            } else {
                filtered.next()
            }
        } else {
            next_occurrence
        }
        .cloned()
    }
}

/// Checks whether the `start` and `end` of a match are on word boundaries.
fn is_matching_whole_words(text: &Rope, start: usize, end: usize) -> bool {
    WordCursor::new(text, start + 1).prev_code_boundary() == start
        && WordCursor::new(text, end - 1).next_code_boundary() == end
}
