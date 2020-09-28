use anyhow::Result;
use language::{new_highlight_config, new_parser, LapceLanguage};
use std::{
    borrow::Cow,
    io::{self, Read, Write},
};
use std::{collections::HashMap, fs::File};
use tree_sitter::{Parser, Tree};
use tree_sitter_highlight::{
    Highlight, HighlightConfiguration, HighlightEvent, Highlighter,
};
use xi_core_lib::line_offset::{LineOffset, LogicalLines};
use xi_rope::{
    interval::IntervalBounds, rope::Rope, Cursor, Interval, LinesMetric,
    RopeInfo,
};

use crate::language;

#[derive(Eq, PartialEq, Hash, Clone)]
pub struct BufferId(pub usize);

pub struct Buffer {
    rope: Rope,
    pub max_line_len: usize,
    tree: Tree,
    highlight_config: HighlightConfiguration,
    highlight_names: Vec<String>,
    highlights: Vec<(usize, usize, Highlight)>,
    line_highlights: HashMap<usize, Vec<(usize, usize, String)>>,
}

impl Buffer {
    pub fn new(buffer_id: BufferId, path: &str) -> Buffer {
        let rope = if let Ok(rope) = load_file(path) {
            rope
        } else {
            Rope::from("")
        };
        let mut parser = new_parser(LapceLanguage::Rust);
        let tree = parser.parse(&rope.to_string(), None).unwrap();
        let num_lines = rope.line_of_offset(rope.len()) + 1;

        let mut pre_offset = 0;
        let mut max_line_len = 0;
        for i in 0..num_lines {
            let offset = rope.offset_of_line(i);
            let line_len = offset - pre_offset;
            pre_offset = offset;
            if line_len > max_line_len {
                max_line_len = line_len;
            }
        }

        let (highlight_config, highlight_names) =
            new_highlight_config(LapceLanguage::Rust);

        let mut buffer = Buffer {
            rope,
            max_line_len,
            tree,
            highlight_config,
            highlight_names,
            highlights: Vec::new(),
            line_highlights: HashMap::new(),
        };
        buffer.update_highlights();
        buffer
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn update_highlights(&mut self) {
        let mut highlights: Vec<(usize, usize, Highlight)> = Vec::new();
        let mut highlighter = Highlighter::new();
        let rope_str = self.slice_to_cow(..self.len());
        let mut current_hl: Option<Highlight> = None;
        for hightlight in highlighter
            .highlight(
                &self.highlight_config,
                &rope_str.as_bytes(),
                None,
                |_| None,
            )
            .unwrap()
        {
            if let Ok(highlight) = hightlight {
                match highlight {
                    HighlightEvent::Source { start, end } => {
                        if let Some(hl) = current_hl {
                            highlights.push((start, end, hl.clone()));
                        }
                    }
                    HighlightEvent::HighlightStart(hl) => {
                        current_hl = Some(hl);
                    }
                    HighlightEvent::HighlightEnd => current_hl = None,
                }
            }
        }
        self.highlights = highlights;
        self.line_highlights = HashMap::new();
    }

    pub fn get_line_highligh(
        &mut self,
        line: usize,
    ) -> &Vec<(usize, usize, String)> {
        if self.line_highlights.get(&line).is_none() {
            let mut line_highlight = Vec::new();
            let start_offset = self.offset_of_line(line);
            let end_offset = self.offset_of_line(line + 1) - 1;
            for (start, end, hl) in &self.highlights {
                if start > end_offset {
                    break;
                }
                if *start >= start_offset && *start <= end_offset {
                    line_highlight.push((
                        *start,
                        *end,
                        self.highlight_names[hl.0].to_string(),
                    ));
                }
            }
            self.line_highlights.insert(line, line_highlight);
        }
        self.line_highlights.get(&line).unwrap()
    }

    pub fn edit(&mut self, interval: Interval, new_text: &str) {
        self.rope.edit(interval, new_text);
        self.update_highlights();
    }

    pub fn indent_on_line(&self, line: usize) -> String {
        let line_start_offset = self.rope.offset_of_line(line);
        let word_boundary = WordCursor::new(&self.rope, line_start_offset)
            .next_non_blank_char();
        let indent = self.rope.slice_to_cow(line_start_offset..word_boundary);
        indent.to_string()
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.rope.line_of_offset(offset)
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        self.rope.offset_of_line(line)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        LogicalLines.offset_to_line_col(&self.rope, offset)
    }

    pub fn num_lines(&self) -> usize {
        self.line_of_offset(self.rope.len()) + 1
    }

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.rope.len())
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        let line_start_offset = self.rope.offset_of_line(line);
        WordCursor::new(&self.rope, line_start_offset).next_non_blank_char()
    }

    pub fn word_forward(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).next_boundary().unwrap()
    }

    pub fn word_backword(&self, offset: usize) -> usize {
        WordCursor::new(&self.rope, offset).prev_boundary().unwrap()
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }
}

fn load_file(path: &str) -> Result<Rope> {
    let mut f = File::open(path)?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    Ok(Rope::from(std::str::from_utf8(&bytes)?))
}

pub struct WordCursor<'a> {
    inner: Cursor<'a, RopeInfo>,
}

impl<'a> WordCursor<'a> {
    pub fn new(text: &'a Rope, pos: usize) -> WordCursor<'a> {
        let inner = Cursor::new(text, pos);
        WordCursor { inner }
    }

    /// Get previous boundary, and set the cursor at the boundary found.
    pub fn prev_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.prev_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(prev) = self.inner.prev_codepoint() {
                let prop_prev = get_word_property(prev);
                if classify_boundary(prop_prev, prop).is_start() {
                    break;
                }
                prop = prop_prev;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    pub fn next_non_blank_char(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(next) = self.inner.next_codepoint() {
            let prop = get_word_property(next);
            if prop != WordProperty::Space {
                break;
            }
            candidate = self.inner.pos();
        }
        self.inner.set(candidate);
        candidate
    }

    /// Get next boundary, and set the cursor at the boundary found.
    pub fn next_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.next_codepoint() {
            let mut prop = get_word_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_word_property(next);
                if classify_boundary(prop, prop_next).is_end() {
                    break;
                }
                prop = prop_next;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    /// Return the selection for the word containing the current cursor. The
    /// cursor is moved to the end of that selection.
    pub fn select_word(&mut self) -> (usize, usize) {
        let initial = self.inner.pos();
        let init_prop_after =
            self.inner.next_codepoint().map(get_word_property);
        self.inner.set(initial);
        let init_prop_before =
            self.inner.prev_codepoint().map(get_word_property);
        let mut start = initial;
        let init_boundary =
            if let (Some(pb), Some(pa)) = (init_prop_before, init_prop_after) {
                classify_boundary_initial(pb, pa)
            } else {
                WordBoundary::Both
            };
        let mut prop_after = init_prop_after;
        let mut prop_before = init_prop_before;
        if prop_after.is_none() {
            start = self.inner.pos();
            prop_after = prop_before;
            prop_before = self.inner.prev_codepoint().map(get_word_property);
        }
        while let (Some(pb), Some(pa)) = (prop_before, prop_after) {
            if start == initial {
                if init_boundary.is_start() {
                    break;
                }
            } else if !init_boundary.is_boundary() {
                if classify_boundary(pb, pa).is_boundary() {
                    break;
                }
            } else if classify_boundary(pb, pa).is_start() {
                break;
            }
            start = self.inner.pos();
            prop_after = prop_before;
            prop_before = self.inner.prev_codepoint().map(get_word_property);
        }
        self.inner.set(initial);
        let mut end = initial;
        prop_after = init_prop_after;
        prop_before = init_prop_before;
        if prop_before.is_none() {
            prop_before = self.inner.next_codepoint().map(get_word_property);
            end = self.inner.pos();
            prop_after = self.inner.next_codepoint().map(get_word_property);
        }
        while let (Some(pb), Some(pa)) = (prop_before, prop_after) {
            if end == initial {
                if init_boundary.is_end() {
                    break;
                }
            } else if !init_boundary.is_boundary() {
                if classify_boundary(pb, pa).is_boundary() {
                    break;
                }
            } else if classify_boundary(pb, pa).is_end() {
                break;
            }
            end = self.inner.pos();
            prop_before = prop_after;
            prop_after = self.inner.next_codepoint().map(get_word_property);
        }
        self.inner.set(end);
        (start, end)
    }
}

#[derive(PartialEq, Eq)]
enum WordBoundary {
    Interior,
    Start, // a boundary indicating the end of a word
    End,   // a boundary indicating the start of a word
    Both,
}

impl WordBoundary {
    fn is_start(&self) -> bool {
        *self == WordBoundary::Start || *self == WordBoundary::Both
    }

    fn is_end(&self) -> bool {
        *self == WordBoundary::End || *self == WordBoundary::Both
    }

    fn is_boundary(&self) -> bool {
        *self != WordBoundary::Interior
    }
}

fn classify_boundary(prev: WordProperty, next: WordProperty) -> WordBoundary {
    use self::WordBoundary::*;
    use self::WordProperty::*;
    match (prev, next) {
        (_, Lf) => Start,
        (Lf, _) => Start,
        (_, Space) => Interior,
        (Space, _) => Both,
        (Punctuation, Other) => Both,
        (Other, Punctuation) => Both,
        _ => Interior,
    }
}

fn classify_boundary_initial(
    prev: WordProperty,
    next: WordProperty,
) -> WordBoundary {
    use self::WordBoundary::*;
    use self::WordProperty::*;
    match (prev, next) {
        (Lf, Other) => Start,
        (Other, Lf) => End,
        (Lf, Space) => Interior,
        (Lf, Punctuation) => Interior,
        (Space, Lf) => Interior,
        (Punctuation, Lf) => Interior,
        (Space, Punctuation) => Interior,
        (Punctuation, Space) => Interior,
        _ => classify_boundary(prev, next),
    }
}

#[derive(Copy, Clone, PartialEq)]
enum WordProperty {
    Lf,
    Space,
    Punctuation,
    Other, // includes letters and all of non-ascii unicode
}

fn get_word_property(codepoint: char) -> WordProperty {
    if codepoint <= ' ' {
        // TODO: deal with \r
        if codepoint == '\n' {
            return WordProperty::Lf;
        }
        return WordProperty::Space;
    } else if codepoint <= '\u{3f}' {
        // Hardcoded: !"#$%&'()*+,-./:;<=>?
        if (0xfc00fffe00000000u64 >> (codepoint as u32)) & 1 != 0 {
            return WordProperty::Punctuation;
        }
    } else if codepoint <= '\u{7f}' {
        // Hardcoded: @[\]^`{|}~
        if (0x7800000178000001u64 >> ((codepoint as u32) & 0x3f)) & 1 != 0 {
            return WordProperty::Punctuation;
        }
    }
    WordProperty::Other
}
