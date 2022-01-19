use druid::{piet::PietText, Env, Point, Rect, Size};
use serde::{Deserialize, Serialize};
use xi_core_lib::selection::InsertDrift;
use xi_rope::{RopeDelta, Transformer};

use crate::{
    buffer::BufferNew,
    config::Config,
    data::RegisterData,
    state::{Mode, VisualMode},
    theme::OldLapceTheme,
};
use std::cmp::{max, min};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    pub mode: CursorMode,
    pub horiz: Option<ColPosition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CursorMode {
    Normal(usize),
    Visual {
        start: usize,
        end: usize,
        mode: VisualMode,
    },
    Insert(Selection),
}

impl Cursor {
    pub fn new(mode: CursorMode, horiz: Option<ColPosition>) -> Self {
        Self { mode, horiz }
    }

    pub fn offset(&self) -> usize {
        match &self.mode {
            CursorMode::Normal(offset) => *offset,
            CursorMode::Visual { start, end, mode } => *end,
            CursorMode::Insert(selection) => selection.get_cursor_offset(),
        }
    }

    pub fn is_normal(&self) -> bool {
        match &self.mode {
            CursorMode::Normal(_) => true,
            _ => false,
        }
    }

    pub fn is_insert(&self) -> bool {
        match &self.mode {
            CursorMode::Insert(_) => true,
            _ => false,
        }
    }

    pub fn is_visual(&self) -> bool {
        match &self.mode {
            CursorMode::Visual { .. } => true,
            _ => false,
        }
    }

    pub fn get_mode(&self) -> Mode {
        match &self.mode {
            CursorMode::Normal(_) => Mode::Normal,
            CursorMode::Visual { .. } => Mode::Visual,
            CursorMode::Insert(_) => Mode::Insert,
        }
    }

    pub fn current_line(&self, buffer: &BufferNew) -> usize {
        buffer.line_of_offset(self.offset())
    }

    pub fn current_char(
        &self,
        text: &mut PietText,
        buffer: &BufferNew,
        config: &Config,
    ) -> (f64, f64) {
        let offset = self.offset();
        let line = buffer.line_of_offset(self.offset());
        let next = buffer.next_grapheme_offset(
            offset,
            1,
            buffer.offset_line_end(offset, true),
        );

        let (_, x0) = buffer.offset_to_line_col(offset);
        let (_, x1) = buffer.offset_to_line_col(next);
        let width = config.editor_text_width(text, "W");
        let x0 = x0 as f64 * width;
        let x1 = x1 as f64 * width;
        (x0, x1)
    }

    pub fn lines(&self, buffer: &BufferNew) -> (usize, usize) {
        match &self.mode {
            CursorMode::Normal(offset) => {
                let line = buffer.line_of_offset(*offset);
                (line, line)
            }
            CursorMode::Visual { start, end, mode } => {
                let start_line = buffer.line_of_offset(*start.min(end));
                let end_line = buffer.line_of_offset(*start.max(end));
                (start_line, end_line)
            }
            CursorMode::Insert(selection) => (
                buffer.line_of_offset(selection.min_offset()),
                buffer.line_of_offset(selection.max_offset()),
            ),
        }
    }

    pub fn yank(&self, buffer: &BufferNew) -> RegisterData {
        let content = match &self.mode {
            CursorMode::Insert(selection) => selection
                .regions()
                .iter()
                .map(|r| buffer.slice_to_cow(r.min()..r.max()).to_string())
                .collect::<Vec<String>>()
                .join("\n"),
            CursorMode::Normal(offset) => {
                let new_offset =
                    buffer.next_grapheme_offset(*offset, 1, buffer.len());
                buffer.slice_to_cow(*offset..new_offset).to_string()
            }
            CursorMode::Visual { start, end, mode } => match mode {
                VisualMode::Normal => buffer
                    .slice_to_cow(
                        *start.min(end)
                            ..buffer.next_grapheme_offset(
                                *start.max(end),
                                1,
                                buffer.len(),
                            ),
                    )
                    .to_string(),
                VisualMode::Linewise => {
                    let start_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.min(end)));
                    let end_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.max(end)) + 1);
                    buffer.slice_to_cow(start_offset..end_offset).to_string()
                }
                VisualMode::Blockwise => {
                    let mut lines = Vec::new();
                    let (start_line, start_col) =
                        buffer.offset_to_line_col(*start.min(end));
                    let (end_line, end_col) =
                        buffer.offset_to_line_col(*start.max(end));
                    let left = start_col.min(end_col);
                    let right = start_col.max(end_col) + 1;
                    for line in start_line..end_line + 1 {
                        let max_col = buffer.line_end_col(line, true);
                        if left > max_col {
                            lines.push("".to_string());
                        } else {
                            let right = match &self.horiz {
                                Some(ColPosition::End) => max_col,
                                _ => {
                                    if right > max_col {
                                        max_col
                                    } else {
                                        right
                                    }
                                }
                            };
                            let left = buffer.offset_of_line_col(line, left);
                            let right = buffer.offset_of_line_col(line, right);
                            lines.push(buffer.slice_to_cow(left..right).to_string());
                        }
                    }
                    lines.join("\n") + "\n"
                }
            },
        };
        let mode = match &self.mode {
            CursorMode::Normal(_) | CursorMode::Insert { .. } => VisualMode::Normal,
            CursorMode::Visual { mode, .. } => mode.clone(),
        };
        RegisterData { content, mode }
    }

    pub fn edit_selection(&self, buffer: &BufferNew) -> Selection {
        match &self.mode {
            CursorMode::Insert(selection) => selection.clone(),
            CursorMode::Normal(offset) => Selection::region(
                *offset,
                buffer.next_grapheme_offset(*offset, 1, buffer.len()),
            ),
            CursorMode::Visual { start, end, mode } => match mode {
                VisualMode::Normal => Selection::region(
                    *start.min(end),
                    buffer.next_grapheme_offset(*start.max(end), 1, buffer.len()),
                ),
                VisualMode::Linewise => {
                    let start_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.min(end)));
                    let end_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.max(end)) + 1);
                    Selection::region(start_offset, end_offset)
                }
                VisualMode::Blockwise => {
                    let mut selection = Selection::new();
                    let (start_line, start_col) =
                        buffer.offset_to_line_col(*start.min(end));
                    let (end_line, end_col) =
                        buffer.offset_to_line_col(*start.max(end));
                    let left = start_col.min(end_col);
                    let right = start_col.max(end_col) + 1;
                    for line in start_line..end_line + 1 {
                        let max_col = buffer.line_end_col(line, true);
                        if left > max_col {
                            continue;
                        }
                        let right = match &self.horiz {
                            Some(ColPosition::End) => max_col,
                            _ => {
                                if right > max_col {
                                    max_col
                                } else {
                                    right
                                }
                            }
                        };
                        let left = buffer.offset_of_line_col(line, left);
                        let right = buffer.offset_of_line_col(line, right);
                        selection.add_region(SelRegion::new(left, right, None));
                    }
                    selection
                }
            },
        }
    }

    pub fn apply_delta(&mut self, delta: &RopeDelta) {
        match &self.mode {
            CursorMode::Normal(offset) => {
                let mut transformer = Transformer::new(delta);
                let new_offset = transformer.transform(*offset, true);
                self.mode = CursorMode::Normal(new_offset);
            }
            CursorMode::Visual { start, end, mode } => {
                let mut transformer = Transformer::new(delta);
                let start = transformer.transform(*start, false);
                let end = transformer.transform(*end, true);
                self.mode = CursorMode::Visual {
                    start,
                    end,
                    mode: mode.clone(),
                };
            }
            CursorMode::Insert(selection) => {
                let selection =
                    selection.apply_delta(delta, true, InsertDrift::Default);
                self.mode = CursorMode::Insert(selection);
            }
        }
        self.horiz = None;
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum ColPosition {
    FirstNonBlank,
    Start,
    End,
    Col(usize),
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct SelRegion {
    start: usize,
    end: usize,
    horiz: Option<ColPosition>,
}

impl SelRegion {
    pub fn new(start: usize, end: usize, horiz: Option<ColPosition>) -> SelRegion {
        SelRegion { start, end, horiz }
    }

    pub fn caret(offset: usize) -> SelRegion {
        SelRegion {
            start: offset,
            end: offset,
            horiz: None,
        }
    }

    pub fn min(self) -> usize {
        min(self.start, self.end)
    }

    pub fn max(self) -> usize {
        max(self.start, self.end)
    }

    pub fn start(self) -> usize {
        self.start
    }

    pub fn end(self) -> usize {
        self.end
    }

    pub fn horiz(&self) -> Option<&ColPosition> {
        self.horiz.as_ref()
    }

    pub fn is_caret(self) -> bool {
        self.start == self.end
    }

    fn should_merge(self, other: SelRegion) -> bool {
        other.min() < self.max()
            || ((self.is_caret() || other.is_caret()) && other.min() == self.max())
    }

    fn merge_with(self, other: SelRegion) -> SelRegion {
        let is_forward = self.end > self.start || other.end > other.start;
        let new_min = min(self.min(), other.min());
        let new_max = max(self.max(), other.max());
        let (start, end) = if is_forward {
            (new_min, new_max)
        } else {
            (new_max, new_min)
        };
        // Could try to preserve horiz/affinity from one of the
        // sources, but very likely not worth it.
        SelRegion::new(start, end, None)
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Selection {
    regions: Vec<SelRegion>,
}

impl Selection {
    pub fn new() -> Selection {
        Selection {
            regions: Vec::new(),
        }
    }

    pub fn new_simple() -> Selection {
        Selection {
            regions: vec![SelRegion {
                start: 0,
                end: 0,
                horiz: None,
            }],
        }
    }

    pub fn caret(offset: usize) -> Selection {
        Selection {
            regions: vec![SelRegion {
                start: offset,
                end: offset,
                horiz: None,
            }],
        }
    }

    pub fn region(start: usize, end: usize) -> Selection {
        Selection {
            regions: vec![SelRegion {
                start,
                end,
                horiz: None,
            }],
        }
    }

    pub fn first(&self) -> Option<&SelRegion> {
        if self.len() == 0 {
            return None;
        }
        Some(&self.regions[0])
    }

    pub fn last(&self) -> Option<&SelRegion> {
        if self.len() == 0 {
            return None;
        }
        Some(&self.regions[self.len() - 1])
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn min_offset(&self) -> usize {
        let mut offset = self.regions()[0].min();
        for region in &self.regions {
            offset = offset.min(region.min());
        }
        offset
    }

    pub fn max_offset(&self) -> usize {
        let mut offset = self.regions()[0].max();
        for region in &self.regions {
            offset = offset.max(region.max());
        }
        offset
    }

    pub fn min(&self) -> Selection {
        let mut selection = Self::new();
        for region in &self.regions {
            let new_region = SelRegion::new(region.min(), region.min(), None);
            selection.add_region(new_region);
        }
        selection
    }

    pub fn expand(&self) -> Selection {
        let mut selection = Self::new();
        for region in &self.regions {
            let new_region = SelRegion::new(
                region.min(),
                region.max() + 1,
                region.horiz.map(|h| h.clone()),
            );
            selection.add_region(new_region);
        }
        selection
    }

    pub fn collapse(&self) -> Selection {
        let mut selection = Self::new();
        selection.add_region(self.regions[0].clone());
        selection
    }

    pub fn add_region(&mut self, region: SelRegion) {
        let mut ix = self.search(region.min());
        if ix == self.regions.len() {
            self.regions.push(region);
            return;
        }
        let mut region = region;
        let mut end_ix = ix;
        if self.regions[ix].min() <= region.min() {
            if self.regions[ix].should_merge(region) {
                region = self.regions[ix].merge_with(region);
            } else {
                ix += 1;
            }
            end_ix += 1;
        }
        while end_ix < self.regions.len()
            && region.should_merge(self.regions[end_ix])
        {
            region = region.merge_with(self.regions[end_ix]);
            end_ix += 1;
        }
        if ix == end_ix {
            self.regions.insert(ix, region);
        } else {
            self.regions[ix] = region;
            remove_n_at(&mut self.regions, ix + 1, end_ix - ix - 1);
        }
    }

    pub fn get_cursor_offset(&self) -> usize {
        self.regions[0].end
    }

    pub fn is_caret(&self) -> bool {
        for region in &self.regions {
            if !region.is_caret() {
                return false;
            }
        }
        true
    }

    // pub fn min(&self) -> usize {
    //     self.regions[self.regions.len() - 1].min()
    // }

    pub fn regions(&self) -> &[SelRegion] {
        &self.regions
    }

    pub fn to_start_caret(&self) -> Selection {
        let region = self.regions[0];
        Selection {
            regions: vec![SelRegion {
                start: region.start,
                end: region.start,
                horiz: None,
            }],
        }
    }

    pub fn to_caret(&self) -> Selection {
        let region = self.regions[0];
        Selection {
            regions: vec![SelRegion {
                start: region.end,
                end: region.end,
                horiz: region.horiz,
            }],
        }
    }

    pub fn search(&self, offset: usize) -> usize {
        if self.regions.is_empty() || offset > self.regions.last().unwrap().max() {
            return self.regions.len();
        }
        match self.regions.binary_search_by(|r| r.max().cmp(&offset)) {
            Ok(ix) => ix,
            Err(ix) => ix,
        }
    }

    pub fn delete_range(&mut self, start: usize, end: usize, delete_adjacent: bool) {
        let mut first = self.search(start);
        let mut last = self.search(end);
        if first >= self.regions.len() {
            return;
        }
        if !delete_adjacent && self.regions[first].max() == start {
            first += 1;
        }
        if last < self.regions.len()
            && ((delete_adjacent && self.regions[last].min() <= end)
                || (!delete_adjacent && self.regions[last].min() < end))
        {
            last += 1;
        }
        remove_n_at(&mut self.regions, first, last - first);
    }

    pub fn regions_in_range(&self, start: usize, end: usize) -> &[SelRegion] {
        let first = self.search(start);
        let mut last = self.search(end);
        if last < self.regions.len() && self.regions[last].min() <= end {
            last += 1;
        }
        &self.regions[first..last]
    }

    pub fn search_min(&self, offset: usize) -> usize {
        if self.regions.is_empty() || offset > self.regions.last().unwrap().max() {
            return self.regions.len();
        }
        match self
            .regions
            .binary_search_by(|r| r.min().cmp(&(offset + 1)))
        {
            Ok(ix) => ix,
            Err(ix) => ix,
        }
    }

    pub fn full_regions_in_range(&self, start: usize, end: usize) -> &[SelRegion] {
        let first = self.search_min(start);
        let mut last = self.search_min(end);
        if last < self.regions.len() && self.regions[last].min() <= end {
            last += 1;
        }
        &self.regions[first..last]
    }

    pub fn apply_delta(
        &self,
        delta: &RopeDelta,
        after: bool,
        drift: InsertDrift,
    ) -> Selection {
        let mut result = Selection::new();
        let mut transformer = Transformer::new(delta);
        for region in self.regions() {
            let is_caret = region.start == region.end;
            let is_region_forward = region.start < region.end;

            let (start_after, end_after) = match (drift, is_caret) {
                (InsertDrift::Inside, false) => {
                    (!is_region_forward, is_region_forward)
                }
                (InsertDrift::Outside, false) => {
                    (is_region_forward, !is_region_forward)
                }
                _ => (after, after),
            };

            let new_region = SelRegion::new(
                transformer.transform(region.start, start_after),
                transformer.transform(region.end, end_after),
                None,
            );
            result.add_region(new_region);
        }
        result
    }
}

#[derive(Clone, Debug)]
pub enum LinePosition {
    First,
    Last,
    Line(usize),
}

#[derive(Clone, Debug)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    FirstNonBlank,
    StartOfLine,
    EndOfLine,
    Line(LinePosition),
    Offset(usize),
    WordEndForward,
    WordForward,
    WordBackward,
    NextUnmatched(char),
    PreviousUnmatched(char),
    MatchPairs,
}

impl PartialEq for Movement {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Movement {
    pub fn is_vertical(&self) -> bool {
        match self {
            Movement::Up | Movement::Down | Movement::Line(_) => true,
            _ => false,
        }
    }

    pub fn is_inclusive(&self) -> bool {
        match self {
            Movement::WordEndForward => true,
            _ => false,
        }
    }

    pub fn is_jump(&self) -> bool {
        match self {
            Movement::Line(_) => true,
            Movement::Offset(_) => true,
            _ => false,
        }
    }

    pub fn update_index(
        &self,
        index: usize,
        len: usize,
        count: usize,
        recursive: bool,
    ) -> usize {
        if len == 0 {
            return 0;
        }
        match self {
            Movement::Up => {
                format_index(index as i64 - count as i64, len, recursive)
            }
            Movement::Down => {
                format_index(index as i64 + count as i64, len, recursive)
            }
            Movement::Line(position) => match position {
                LinePosition::Line(n) => format_index(*n as i64, len, recursive),
                LinePosition::First => 0,
                LinePosition::Last => len - 1,
            },
            _ => index,
        }
    }
}

pub fn remove_n_at<T>(v: &mut Vec<T>, index: usize, n: usize) {
    if n == 1 {
        v.remove(index);
    } else if n > 1 {
        v.splice(index..index + n, std::iter::empty());
    }
}

fn format_index(index: i64, len: usize, recursive: bool) -> usize {
    if recursive {
        if index >= len as i64 {
            (index % len as i64) as usize
        } else if index < 0 {
            len - (-index % len as i64) as usize
        } else {
            index as usize
        }
    } else {
        if index >= len as i64 {
            len - 1
        } else if index < 0 {
            0
        } else {
            index as usize
        }
    }
}
