use xi_core_lib::selection::InsertDrift;
use xi_rope::{RopeDelta, Transformer};

use crate::{buffer::Buffer, state::Mode};
use std::cmp::{max, min};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ColPosition {
    FirstNonBlank,
    Start,
    End,
    Col(usize),
}

#[derive(Clone, Copy, PartialEq, Debug)]
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

#[derive(Clone, PartialEq)]
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

#[derive(Clone)]
pub enum LinePosition {
    First,
    Last,
    Line(usize),
}

#[derive(Clone)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
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

    pub fn update_selection(
        &self,
        selection: &Selection,
        buffer: &Buffer,
        count: usize,
        include_newline: bool,
        modify: bool,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in &selection.regions {
            let region =
                self.update_region(region, buffer, count, include_newline, modify);
            new_selection.add_region(region);
        }
        buffer.fill_horiz(&new_selection)
    }

    pub fn update_index(
        &self,
        index: usize,
        len: usize,
        count: usize,
        recursive: bool,
    ) -> usize {
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

    pub fn update_region(
        &self,
        region: &SelRegion,
        buffer: &Buffer,
        count: usize,
        include_newline: bool,
        modify: bool,
    ) -> SelRegion {
        let horiz = if let Some(horiz) = region.horiz {
            horiz
        } else {
            let (_, col) = buffer.offset_to_line_col(region.end);
            ColPosition::Col(col)
        };
        let (end, horiz) = match self {
            Movement::Left => {
                let end = region.end;
                let line = buffer.line_of_offset(end);
                let line_start_offset = buffer.offset_of_line(line);
                let new_end = if end < count {
                    0
                } else if end - count > line_start_offset {
                    end - count
                } else {
                    line_start_offset
                };
                let (_, col) = buffer.offset_to_line_col(new_end);

                (new_end, ColPosition::Col(col))
            }
            Movement::Right => {
                let end = region.end;
                let line_end = buffer.line_end_offset(end, include_newline);

                let mut new_end = end + count;
                if new_end > buffer.len() {
                    new_end = buffer.len()
                }
                if new_end > line_end {
                    new_end = line_end;
                }

                let (_, col) = buffer.offset_to_line_col(new_end);
                (new_end, ColPosition::Col(col))
            }
            Movement::Up => {
                let line = buffer.line_of_offset(region.end);
                let line = if line > count { line - count } else { 0 };
                let max_col = buffer.line_max_col(line, include_newline);
                let col = match horiz {
                    ColPosition::End => max_col,
                    ColPosition::Col(n) => match max_col > n {
                        true => n,
                        false => max_col,
                    },
                    _ => 0,
                };
                let new_end = buffer.offset_of_line(line) + col;
                (new_end, horiz)
            }
            Movement::Down => {
                let last_line = buffer.last_line();
                let line = buffer.line_of_offset(region.end) + count;
                let line = if line > last_line { last_line } else { line };
                let col = buffer.line_horiz_col(line, &horiz, include_newline);
                let new_end = buffer.offset_of_line(line) + col;
                (new_end, horiz)
            }
            Movement::StartOfLine => {
                let line = buffer.line_of_offset(region.end);
                let new_end = buffer.offset_of_line(line);
                (new_end, ColPosition::Start)
            }
            Movement::EndOfLine => {
                let new_end = buffer.line_end_offset(region.end, include_newline);
                (new_end, ColPosition::End)
            }
            Movement::Line(position) => {
                let line = match position {
                    LinePosition::Line(line) => {
                        let line = line - 1;
                        let last_line = buffer.last_line();
                        match line {
                            n if n > last_line => last_line,
                            n => n,
                        }
                    }
                    LinePosition::First => 0,
                    LinePosition::Last => buffer.last_line(),
                };
                let col = buffer.line_horiz_col(line, &horiz, include_newline);
                let new_end = buffer.offset_of_line(line) + col;
                (new_end, horiz)
            }
            Movement::Offset(offset) => {
                let new_end = *offset;
                let (_, col) = buffer.offset_to_line_col(new_end);
                (new_end, ColPosition::Col(col))
            }
            Movement::WordForward => {
                let mut new_end = region.end;
                for i in 0..count {
                    new_end = buffer.word_forward(new_end);
                }
                let (_, col) = buffer.offset_to_line_col(new_end);
                (new_end, ColPosition::Col(col))
            }
            Movement::WordEndForward => {
                let mut new_end = region.end;
                for i in 0..count {
                    new_end = buffer.word_end_forward(new_end);
                }
                let (_, col) = buffer.offset_to_line_col(new_end);
                (new_end, ColPosition::Col(col))
            }
            Movement::WordBackward => {
                let mut new_end = region.end;
                for i in 0..count {
                    new_end = buffer.word_backword(new_end);
                }
                let line_end_offset =
                    buffer.line_end_offset(new_end, include_newline);
                if new_end > line_end_offset {
                    new_end = line_end_offset;
                }
                let (_, col) = buffer.offset_to_line_col(new_end);
                (new_end, ColPosition::Col(col))
            }
            Movement::NextUnmatched(c) => {
                let mut end = region.end;
                for i in 0..count {
                    if let Some(new) = buffer.next_unmmatched(end + 1, *c) {
                        end = new - 1;
                    } else {
                        break;
                    }
                }
                let (_, col) = buffer.offset_to_line_col(end);
                (end, ColPosition::Col(col))
            }
            Movement::PreviousUnmatched(c) => {
                let mut end = region.end;
                for i in 0..count {
                    if let Some(new) = buffer.previous_unmmatched(end, *c) {
                        end = new;
                    } else {
                        break;
                    }
                }
                let (_, col) = buffer.offset_to_line_col(end);
                (end, ColPosition::Col(col))
            }
            Movement::MatchPairs => {
                let mut end = region.end;
                if let Some(new) = buffer.match_pairs(end) {
                    end = new;
                }
                let (_, col) = buffer.offset_to_line_col(end);
                (end, ColPosition::Col(col))
            }
        };

        let start = match modify {
            true => region.start,
            false => end,
        };

        SelRegion {
            start,
            end,
            horiz: Some(horiz),
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
