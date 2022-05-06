use druid::PaintCtx;
use druid::{piet::PietTextLayout, Vec2};
use druid::{
    piet::{Text, TextAttribute, TextLayoutBuilder},
    ExtEventSink, Target, WidgetId,
};
use lapce_core::indent::{auto_detect_indent_style, IndentStyle};
use lapce_core::style::line_styles;
use lapce_core::syntax::Syntax;
use lapce_rpc::buffer::{BufferHeadResponse, BufferId, NewBufferResponse};
use lapce_rpc::style::{LineStyle, LineStyles, Style};
use lsp_types::{CodeActionResponse, Position};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{self, AtomicU64};
use std::{borrow::Cow, collections::BTreeSet, path::PathBuf, sync::Arc, thread};
use unicode_width::UnicodeWidthChar;
use xi_rope::{
    multiset::Subset, rope::Rope, spans::Spans, Cursor, Delta, RopeDelta, RopeInfo,
};
use xi_unicode::EmojiExt;

use crate::buffer::data::{BufferData, BufferDataListener, EditableBufferData};
use crate::buffer::decoration::BufferDecoration;
use crate::config::{Config, LapceTheme};
use crate::editor::EditorLocationNew;
use crate::find::FindProgress;
use crate::{
    command::LapceUICommand, command::LAPCE_UI_COMMAND, find::Find,
    proxy::LapceProxy,
};

pub mod data;
pub mod decoration;

#[derive(Debug, Clone)]
pub struct InvalLines {
    pub start_line: usize,
    pub inval_count: usize,
    pub new_count: usize,
}

#[derive(Clone, Debug)]
pub enum DiffLines {
    Left(Range<usize>),
    Both(Range<usize>, Range<usize>),
    Skip(Range<usize>, Range<usize>),
    Right(Range<usize>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DiffResult<T> {
    Left(T),
    Both(T, T),
    Right(T),
}

pub struct StyledTextLayout {
    pub text: String,
    pub layout: PietTextLayout,
    pub styles: Arc<Vec<(usize, usize, Style)>>,
    pub bounds: [f64; 2],
}

pub struct HighlightTextLayout {
    pub layout: PietTextLayout,
    pub text: String,
    pub highlights: Vec<(usize, usize, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditType {
    Other,
    InsertChars,
    InsertNewline,
    Delete,
    Undo,
    Redo,
}

impl EditType {
    /// Checks whether a new undo group should be created between two edits.
    fn breaks_undo_group(self, previous: EditType) -> bool {
        self == EditType::Other || self != previous
    }
}

#[derive(Clone)]
enum Contents {
    Edit {
        /// Groups related edits together so that they are undone and re-done
        /// together. For example, an auto-indent insertion would be un-done
        /// along with the newline that triggered it.
        undo_group: usize,
        /// The subset of the characters of the union string from after this
        /// revision that were added by this revision.
        inserts: Subset,
        /// The subset of the characters of the union string from after this
        /// revision that were deleted by this revision.
        deletes: Subset,
    },
    Undo {
        /// The set of groups toggled between undone and done.
        /// Just the `symmetric_difference` (XOR) of the two sets.
        toggled_groups: BTreeSet<usize>, // set of undo_group id's
        /// Used to store a reversible difference between the old
        /// and new deletes_from_union
        deletes_bitxor: Subset,
    },
}

#[derive(Clone)]
struct Revision {
    max_undo_so_far: usize,
    edit: Contents,
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub enum LocalBufferKind {
    Empty,
    Palette,
    Search,
    SourceControl,
    FilePicker,
    Keymap,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BufferContent {
    File(PathBuf),
    Local(LocalBufferKind),
    Value(String),
}

impl BufferContent {
    pub fn is_file(&self) -> bool {
        matches!(self, BufferContent::File(_))
    }

    pub fn is_special(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Local(local) => match local {
                LocalBufferKind::Search
                | LocalBufferKind::Palette
                | LocalBufferKind::SourceControl
                | LocalBufferKind::FilePicker
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => true,
                LocalBufferKind::Empty => false,
            },
            BufferContent::Value(_) => true,
        }
    }

    pub fn is_input(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Local(local) => match local {
                LocalBufferKind::Search
                | LocalBufferKind::Palette
                | LocalBufferKind::FilePicker
                | LocalBufferKind::Settings
                | LocalBufferKind::Keymap => true,
                LocalBufferKind::Empty | LocalBufferKind::SourceControl => false,
            },
            BufferContent::Value(_) => true,
        }
    }

    pub fn is_search(&self) -> bool {
        match &self {
            BufferContent::File(_) => false,
            BufferContent::Value(_) => false,
            BufferContent::Local(local) => matches!(local, LocalBufferKind::Search),
        }
    }
}

#[derive(Clone)]
pub struct Buffer {
    data: BufferData,
    pub start_to_load: Rc<RefCell<bool>>,

    pub history_styles: im::HashMap<String, Arc<Spans<Style>>>,
    pub history_line_styles: Rc<RefCell<HashMap<String, LineStyles>>>,
    pub history_changes: im::HashMap<String, Arc<Vec<DiffLines>>>,

    pub cursor_offset: usize,
    pub scroll_offset: Vec2,

    pub code_actions: im::HashMap<usize, CodeActionResponse>,

    pub decoration: BufferDecoration,
}

pub struct BufferEditListener<'a> {
    decoration: &'a mut BufferDecoration,
    proxy: &'a LapceProxy,
}

impl BufferDataListener for BufferEditListener<'_> {
    fn should_apply_edit(&self) -> bool {
        self.decoration.loaded
    }

    fn on_edit_applied(&mut self, buffer: &BufferData, delta: &RopeDelta) {
        if !self.decoration.local {
            self.proxy.update(buffer.id, delta, buffer.rev);
        }

        self.decoration.update_styles(delta);
        self.decoration.find.borrow_mut().unset();
        *self.decoration.find_progress.borrow_mut() = FindProgress::Started;
        self.decoration.notify_update(buffer, Some(delta));
        self.decoration.notify_special(buffer);
    }
}

fn shuffle_tombstones(
    text: &Rope,
    tombstones: &Rope,
    old_deletes_from_union: &Subset,
    new_deletes_from_union: &Subset,
) -> Rope {
    // Taking the complement of deletes_from_union leads to an interleaving valid for swapped text and tombstones,
    // allowing us to use the same method to insert the text into the tombstones.
    let inverse_tombstones_map = old_deletes_from_union.complement();
    let move_delta = Delta::synthesize(
        text,
        &inverse_tombstones_map,
        &new_deletes_from_union.complement(),
    );
    move_delta.apply(tombstones)
}

fn shuffle(
    text: &Rope,
    tombstones: &Rope,
    old_deletes_from_union: &Subset,
    new_deletes_from_union: &Subset,
) -> (Rope, Rope) {
    // Delta that deletes the right bits from the text
    let del_delta = Delta::synthesize(
        tombstones,
        old_deletes_from_union,
        new_deletes_from_union,
    );
    let new_text = del_delta.apply(text);
    (
        new_text,
        shuffle_tombstones(
            text,
            tombstones,
            old_deletes_from_union,
            new_deletes_from_union,
        ),
    )
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
                if classify_boundary(prop, prop_next).is_start() {
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

    pub fn end_boundary(&mut self) -> Option<usize> {
        self.inner.next_codepoint();
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

    pub fn prev_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.prev_codepoint() {
            let prop_prev = get_word_property(prev);
            if prop_prev != WordProperty::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        candidate
    }

    pub fn next_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.next_codepoint() {
            let prop_prev = get_word_property(prev);
            if prop_prev != WordProperty::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        candidate
    }

    pub fn match_pairs(&mut self) -> Option<usize> {
        let c = self.inner.peek_next_codepoint()?;
        let other = matching_char(c)?;
        let left = matching_pair_direction(other)?;
        if left {
            self.previous_unmatched(other)
        } else {
            self.inner.next_codepoint();
            let offset = self.next_unmatched(other)?;
            Some(offset - 1)
        }
    }

    pub fn next_unmatched(&mut self, c: char) -> Option<usize> {
        let other = matching_char(c)?;
        let mut n = 0;
        while let Some(current) = self.inner.next_codepoint() {
            if current == c && n == 0 {
                return Some(self.inner.pos());
            }
            if current == other {
                n += 1;
            } else if current == c {
                n -= 1;
            }
        }
        None
    }

    pub fn previous_unmatched(&mut self, c: char) -> Option<usize> {
        let other = matching_char(c)?;
        let mut n = 0;
        while let Some(current) = self.inner.prev_codepoint() {
            if current == c && n == 0 {
                return Some(self.inner.pos());
            }
            if current == other {
                n += 1;
            } else if current == c {
                n -= 1;
            }
        }
        None
    }

    pub fn select_word(&mut self) -> (usize, usize) {
        let initial = self.inner.pos();
        let end = self.next_code_boundary();
        self.inner.set(initial);
        let start = self.prev_code_boundary();
        (start, end)
    }

    /// Return the selection for the word containing the current cursor. The
    /// cursor is moved to the end of that selection.
    pub fn select_word_old(&mut self) -> (usize, usize) {
        let initial = self.inner.pos();
        let init_prop_after = self.inner.next_codepoint().map(get_word_property);
        self.inner.set(initial);
        let init_prop_before = self.inner.prev_codepoint().map(get_word_property);
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
        (Lf, Lf) => Start,
        (Lf, Space) => Interior,
        (Cr, Lf) => Interior,
        (Space, Lf) => Interior,
        (Space, Cr) => Interior,
        (Space, Space) => Interior,
        (_, Space) => End,
        (Space, _) => Start,
        (Lf, _) => Start,
        (_, Cr) => End,
        (_, Lf) => End,
        (Punctuation, Other) => Both,
        (Other, Punctuation) => Both,
        _ => Interior,
    }
}

fn classify_boundary_initial(
    prev: WordProperty,
    next: WordProperty,
) -> WordBoundary {
    #[allow(clippy::match_single_binding)]
    match (prev, next) {
        // (Lf, Other) => Start,
        // (Other, Lf) => End,
        // (Lf, Space) => Interior,
        // (Lf, Punctuation) => Interior,
        // (Space, Lf) => Interior,
        // (Punctuation, Lf) => Interior,
        // (Space, Punctuation) => Interior,
        // (Punctuation, Space) => Interior,
        _ => classify_boundary(prev, next),
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum WordProperty {
    Cr,
    Lf,
    Space,
    Punctuation,
    Other, // includes letters and all of non-ascii unicode
}

pub fn get_word_property(codepoint: char) -> WordProperty {
    if codepoint <= ' ' {
        if codepoint == '\r' {
            return WordProperty::Cr;
        }
        if codepoint == '\n' {
            return WordProperty::Lf;
        }
        return WordProperty::Space;
    } else if codepoint <= '\u{3f}' {
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

pub fn has_unmatched_pair(line: &str) -> bool {
    let mut count = HashMap::new();
    let mut pair_first = HashMap::new();
    for c in line.chars().rev() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            let pair_count = *count.get(&key).unwrap_or(&0i32);
            pair_first.entry(key).or_insert(left);
            if left {
                count.insert(key, pair_count - 1);
            } else {
                count.insert(key, pair_count + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count < 0 {
            return true;
        }
    }
    for (_, left) in pair_first.iter() {
        if *left {
            return true;
        }
    }
    false
}

pub fn previous_has_unmatched_pair(line: &str, col: usize) -> bool {
    let mut count = HashMap::new();
    let mut pair_first = HashMap::new();
    for c in line[..col].chars().rev() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            let pair_count = *count.get(&key).unwrap_or(&0i32);
            pair_first.entry(key).or_insert(left);
            if left {
                count.insert(key, pair_count - 1);
            } else {
                count.insert(key, pair_count + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count < 0 {
            return true;
        }
    }
    for (_, left) in pair_first.iter() {
        if *left {
            return true;
        }
    }
    false
}

pub fn next_has_unmatched_pair(line: &str, col: usize) -> bool {
    let mut count = HashMap::new();
    for c in line[col..].chars() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            count.entry(key).or_insert(0i32);
            if left {
                count.insert(key, count.get(&key).unwrap_or(&0i32) - 1);
            } else {
                count.insert(key, count.get(&key).unwrap_or(&0i32) + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count > 0 {
            return true;
        }
    }
    false
}

pub fn matching_pair_direction(c: char) -> Option<bool> {
    Some(match c {
        '{' => true,
        '}' => false,
        '(' => true,
        ')' => false,
        '[' => true,
        ']' => false,
        _ => return None,
    })
}

pub fn matching_char(c: char) -> Option<char> {
    Some(match c {
        '{' => '}',
        '}' => '{',
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
}

pub fn char_width(c: char) -> usize {
    if c == '\t' {
        return 8;
    }
    if c.is_emoji_modifier_base() || c.is_emoji_modifier() {
        // treat modifier sequences as double wide
        return 2;
    }
    c.width().unwrap_or(0)
}

pub fn str_col(s: &str, tab_width: usize) -> usize {
    let mut total_width = 0;

    for c in s.chars() {
        let width = if c == '\t' {
            tab_width - total_width % tab_width
        } else {
            char_width(c)
        };

        total_width += width;
    }

    total_width
}

#[allow(dead_code)]
fn buffer_diff(
    left_rope: Rope,
    right_rope: Rope,
    rev: u64,
    atomic_rev: Arc<AtomicU64>,
) -> Option<Vec<DiffLines>> {
    let mut changes = Vec::new();
    let left_str = &left_rope.slice_to_cow(0..left_rope.len());
    let right_str = &right_rope.slice_to_cow(0..right_rope.len());
    let mut left_line = 0;
    let mut right_line = 0;
    for diff in diff::lines(left_str, right_str) {
        if atomic_rev.load(atomic::Ordering::Acquire) != rev {
            return None;
        }
        match diff {
            diff::Result::Left(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Left(r)) => r.end = left_line + 1,
                    _ => changes.push(DiffLines::Left(left_line..left_line + 1)),
                }
                left_line += 1;
            }
            diff::Result::Both(_, _) => {
                match changes.last_mut() {
                    Some(DiffLines::Both(l, r)) => {
                        l.end = left_line + 1;
                        r.end = right_line + 1;
                    }
                    _ => changes.push(DiffLines::Both(
                        left_line..left_line + 1,
                        right_line..right_line + 1,
                    )),
                }
                left_line += 1;
                right_line += 1;
            }
            diff::Result::Right(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Right(r)) => r.end = right_line + 1,
                    _ => changes.push(DiffLines::Right(right_line..right_line + 1)),
                }
                right_line += 1;
            }
        }
    }
    for (i, change) in changes.clone().iter().enumerate().rev() {
        if let DiffLines::Both(l, r) = change {
            if r.len() > 6 {
                changes[i] = DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                changes.insert(
                    i,
                    DiffLines::Skip(l.start + 3..l.end - 3, r.start + 3..r.end - 3),
                );
                changes.insert(
                    i,
                    DiffLines::Both(l.start..l.start + 3, r.start..r.start + 3),
                );
            }
        }
    }
    Some(changes)
}

pub fn rope_diff(
    left_rope: Rope,
    right_rope: Rope,
    rev: u64,
    atomic_rev: Arc<AtomicU64>,
) -> Option<Vec<DiffLines>> {
    let left_lines = left_rope.lines(..).collect::<Vec<Cow<str>>>();
    let right_lines = right_rope.lines(..).collect::<Vec<Cow<str>>>();

    let left_count = left_lines.len();
    let right_count = right_lines.len();
    let min_count = cmp::min(left_count, right_count);

    let leading_equals = left_lines
        .iter()
        .zip(right_lines.iter())
        .take_while(|p| p.0 == p.1)
        .count();
    let trailing_equals = left_lines
        .iter()
        .rev()
        .zip(right_lines.iter().rev())
        .take(min_count - leading_equals)
        .take_while(|p| p.0 == p.1)
        .count();

    let left_diff_size = left_count - leading_equals - trailing_equals;
    let right_diff_size = right_count - leading_equals - trailing_equals;

    let table: Vec<Vec<u32>> = {
        let mut table = vec![vec![0; right_diff_size + 1]; left_diff_size + 1];
        let left_skip = left_lines.iter().skip(leading_equals).take(left_diff_size);
        let right_skip = right_lines
            .iter()
            .skip(leading_equals)
            .take(right_diff_size);

        for (i, l) in left_skip.enumerate() {
            for (j, r) in right_skip.clone().enumerate() {
                if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                    return None;
                }
                table[i + 1][j + 1] = if l == r {
                    table[i][j] + 1
                } else {
                    std::cmp::max(table[i][j + 1], table[i + 1][j])
                };
            }
        }

        table
    };

    let diff = {
        let mut diff = Vec::with_capacity(left_diff_size + right_diff_size);
        let mut i = left_diff_size;
        let mut j = right_diff_size;
        let mut li = left_lines.iter().rev().skip(trailing_equals);
        let mut ri = right_lines.iter().skip(trailing_equals);

        loop {
            if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                return None;
            }
            if j > 0 && (i == 0 || table[i][j] == table[i][j - 1]) {
                j -= 1;
                diff.push(DiffResult::Right(ri.next().unwrap()));
            } else if i > 0 && (j == 0 || table[i][j] == table[i - 1][j]) {
                i -= 1;
                diff.push(DiffResult::Left(li.next().unwrap()));
            } else if i > 0 && j > 0 {
                i -= 1;
                j -= 1;
                diff.push(DiffResult::Both(li.next().unwrap(), ri.next().unwrap()));
            } else {
                break;
            }
        }

        diff
    };

    let mut changes = Vec::new();
    let mut left_line = 0;
    let mut right_line = 0;
    if leading_equals > 0 {
        changes.push(DiffLines::Both(0..leading_equals, 0..leading_equals));
    }
    left_line += leading_equals;
    right_line += leading_equals;

    for diff in diff.iter().rev() {
        if atomic_rev.load(atomic::Ordering::Acquire) != rev {
            return None;
        }
        match diff {
            DiffResult::Left(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Left(r)) => r.end = left_line + 1,
                    _ => changes.push(DiffLines::Left(left_line..left_line + 1)),
                }
                left_line += 1;
            }
            DiffResult::Both(_, _) => {
                match changes.last_mut() {
                    Some(DiffLines::Both(l, r)) => {
                        l.end = left_line + 1;
                        r.end = right_line + 1;
                    }
                    _ => changes.push(DiffLines::Both(
                        left_line..left_line + 1,
                        right_line..right_line + 1,
                    )),
                }
                left_line += 1;
                right_line += 1;
            }
            DiffResult::Right(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Right(r)) => r.end = right_line + 1,
                    _ => changes.push(DiffLines::Right(right_line..right_line + 1)),
                }
                right_line += 1;
            }
        }
    }

    if trailing_equals > 0 {
        changes.push(DiffLines::Both(
            left_count - trailing_equals..left_count,
            right_count - trailing_equals..right_count,
        ));
    }
    if !changes.is_empty() {
        let changes_last = changes.len() - 1;
        for (i, change) in changes.clone().iter().enumerate().rev() {
            if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                return None;
            }
            if let DiffLines::Both(l, r) = change {
                if i == 0 || i == changes_last {
                    if r.len() > 3 {
                        if i == 0 {
                            changes[i] =
                                DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                            changes.insert(
                                i,
                                DiffLines::Skip(
                                    l.start..l.end - 3,
                                    r.start..r.end - 3,
                                ),
                            );
                        } else {
                            changes[i] = DiffLines::Skip(
                                l.start + 3..l.end,
                                r.start + 3..r.end,
                            );
                            changes.insert(
                                i,
                                DiffLines::Both(
                                    l.start..l.start + 3,
                                    r.start..r.start + 3,
                                ),
                            );
                        }
                    }
                } else if r.len() > 6 {
                    changes[i] = DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                    changes.insert(
                        i,
                        DiffLines::Skip(
                            l.start + 3..l.end - 3,
                            r.start + 3..r.end - 3,
                        ),
                    );
                    changes.insert(
                        i,
                        DiffLines::Both(l.start..l.start + 3, r.start..r.start + 3),
                    );
                }
            }
        }
    }
    Some(changes)
}

#[allow(dead_code)]
fn iter_diff<I, T>(left: I, right: I) -> Vec<DiffResult<T>>
where
    I: Clone + Iterator<Item = T> + DoubleEndedIterator,
    T: PartialEq,
{
    let left_count = left.clone().count();
    let right_count = right.clone().count();
    let min_count = cmp::min(left_count, right_count);

    let leading_equals = left
        .clone()
        .zip(right.clone())
        .take_while(|p| p.0 == p.1)
        .count();
    let trailing_equals = left
        .clone()
        .rev()
        .zip(right.clone().rev())
        .take(min_count - leading_equals)
        .take_while(|p| p.0 == p.1)
        .count();

    let left_diff_size = left_count - leading_equals - trailing_equals;
    let right_diff_size = right_count - leading_equals - trailing_equals;

    let table: Vec<Vec<u32>> = {
        let mut table = vec![vec![0; right_diff_size + 1]; left_diff_size + 1];
        let left_skip = left.clone().skip(leading_equals).take(left_diff_size);
        let right_skip = right.clone().skip(leading_equals).take(right_diff_size);

        for (i, l) in left_skip.enumerate() {
            for (j, r) in right_skip.clone().enumerate() {
                table[i + 1][j + 1] = if l == r {
                    table[i][j] + 1
                } else {
                    std::cmp::max(table[i][j + 1], table[i + 1][j])
                };
            }
        }

        table
    };

    let diff = {
        let mut diff = Vec::with_capacity(left_diff_size + right_diff_size);
        let mut i = left_diff_size;
        let mut j = right_diff_size;
        let mut li = left.clone().rev().skip(trailing_equals);
        let mut ri = right.clone().rev().skip(trailing_equals);

        loop {
            if j > 0 && (i == 0 || table[i][j] == table[i][j - 1]) {
                j -= 1;
                diff.push(DiffResult::Right(ri.next().unwrap()));
            } else if i > 0 && (j == 0 || table[i][j] == table[i - 1][j]) {
                i -= 1;
                diff.push(DiffResult::Left(li.next().unwrap()));
            } else if i > 0 && j > 0 {
                i -= 1;
                j -= 1;
                diff.push(DiffResult::Both(li.next().unwrap(), ri.next().unwrap()));
            } else {
                break;
            }
        }

        diff
    };

    let diff_size = leading_equals + diff.len() + trailing_equals;
    let mut total_diff = Vec::with_capacity(diff_size);

    total_diff.extend(
        left.clone()
            .zip(right.clone())
            .take(leading_equals)
            .map(|(l, r)| DiffResult::Both(l, r)),
    );
    total_diff.extend(diff.into_iter().rev());
    total_diff.extend(
        left.skip(leading_equals + left_diff_size)
            .zip(right.skip(leading_equals + right_diff_size))
            .map(|(l, r)| DiffResult::Both(l, r)),
    );

    total_diff
}
// pub fn grapheme_column_width(s: &str) -> usize {
//     // Due to this issue:
//     // https://github.com/unicode-rs/unicode-width/issues/4
//     // we cannot simply use the unicode-width crate to compute
//     // the desired value.
//     // Let's check for emoji-ness for ourselves first
//     use xi_unicode::EmojiExt;
//     for c in s.chars() {
//         if c == '\t' {
//             return 8;
//         }
//         if c.is_emoji_modifier_base() || c.is_emoji_modifier() {
//             // treat modifier sequences as double wide
//             return 2;
//         }
//     }
//     UnicodeWidthStr::width(s)
// }
