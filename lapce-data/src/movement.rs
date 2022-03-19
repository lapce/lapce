use serde::{Deserialize, Serialize};
use xi_rope::{RopeDelta, Transformer};

use crate::{
    buffer::Buffer,
    config::Config,
    data::RegisterData,
    state::{Mode, VisualMode},
};
use std::cmp::{max, min};

#[derive(Copy, Clone)]
pub enum InsertDrift {
    /// Indicates this edit should happen within any (non-caret) selections if possible.
    Inside,
    /// Indicates this edit should happen outside any selections if possible.
    Outside,
    /// Indicates to do whatever the `after` bool says to do
    Default,
}

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
            #[allow(unused_variables)]
            CursorMode::Visual { start, end, mode } => *end,
            CursorMode::Insert(selection) => selection.get_cursor_offset(),
        }
    }

    pub fn is_normal(&self) -> bool {
        matches!(&self.mode, CursorMode::Normal(_))
    }

    pub fn is_insert(&self) -> bool {
        matches!(&self.mode, CursorMode::Insert(_))
    }

    pub fn is_visual(&self) -> bool {
        matches!(&self.mode, CursorMode::Visual { .. })
    }

    pub fn get_mode(&self) -> Mode {
        match &self.mode {
            CursorMode::Normal(_) => Mode::Normal,
            CursorMode::Visual { .. } => Mode::Visual,
            CursorMode::Insert(_) => Mode::Insert,
        }
    }

    pub fn current_line(&self, buffer: &Buffer) -> usize {
        buffer.line_of_offset(self.offset())
    }

    pub fn set_offset(&self, offset: usize, modify: bool, new_cursor: bool) -> Self {
        match &self.mode {
            CursorMode::Normal(old_offset) => {
                if modify && *old_offset != offset {
                    Cursor::new(
                        CursorMode::Visual {
                            start: *old_offset,
                            end: offset,
                            mode: VisualMode::Normal,
                        },
                        None,
                    )
                } else {
                    Cursor::new(CursorMode::Normal(offset), None)
                }
            }
            CursorMode::Visual {
                start,
                end: _,
                mode: _,
            } => {
                if modify {
                    Cursor::new(
                        CursorMode::Visual {
                            start: *start,
                            end: offset,
                            mode: VisualMode::Normal,
                        },
                        None,
                    )
                } else {
                    Cursor::new(CursorMode::Normal(offset), None)
                }
            }
            CursorMode::Insert(selection) => {
                if new_cursor {
                    let mut new_selection = selection.clone();
                    if modify {
                        if let Some(region) = new_selection.last_inserted_mut() {
                            region.end = offset;
                        } else {
                            new_selection.add_region(SelRegion::caret(offset));
                        }
                        Cursor::new(CursorMode::Insert(new_selection), None)
                    } else {
                        let mut new_selection = selection.clone();
                        new_selection.add_region(SelRegion::caret(offset));
                        Cursor::new(CursorMode::Insert(new_selection), None)
                    }
                } else if modify {
                    let mut new_selection = Selection::new();
                    if let Some(region) = selection.first() {
                        let new_regoin =
                            SelRegion::new(region.start(), offset, None);
                        new_selection.add_region(new_regoin);
                    } else {
                        new_selection
                            .add_region(SelRegion::new(offset, offset, None));
                    }
                    Cursor::new(CursorMode::Insert(new_selection), None)
                } else {
                    Cursor::new(CursorMode::Insert(Selection::caret(offset)), None)
                }
            }
        }
    }

    pub fn add_region(
        &self,
        start: usize,
        end: usize,
        modify: bool,
        new_cursor: bool,
    ) -> Self {
        match &self.mode {
            CursorMode::Normal(_offset) => Cursor::new(
                CursorMode::Visual {
                    start,
                    end: end - 1,
                    mode: VisualMode::Normal,
                },
                None,
            ),
            CursorMode::Visual {
                start: old_start,
                end: old_end,
                mode: _,
            } => {
                let forward = old_end >= old_start;
                let new_start = (*old_start).min(*old_end).min(start).min(end - 1);
                let new_end = (*old_start).max(*old_end).max(start).max(end - 1);
                let (new_start, new_end) = if forward {
                    (new_start, new_end)
                } else {
                    (new_end, new_start)
                };
                Cursor::new(
                    CursorMode::Visual {
                        start: new_start,
                        end: new_end,
                        mode: VisualMode::Normal,
                    },
                    None,
                )
            }
            CursorMode::Insert(selection) => {
                let new_selection = if new_cursor {
                    let mut new_selection = selection.clone();
                    if modify {
                        let new_region =
                            if let Some(last_inserted) = selection.last_inserted() {
                                last_inserted
                                    .merge_with(SelRegion::new(start, end, None))
                            } else {
                                SelRegion::new(start, end, None)
                            };
                        new_selection.replace_last_inserted_region(new_region);
                    } else {
                        new_selection.add_region(SelRegion::new(start, end, None));
                    }
                    new_selection
                } else if modify {
                    let mut new_selection = selection.clone();
                    new_selection.add_region(SelRegion::new(start, end, None));
                    new_selection
                } else {
                    Selection::region(start, end)
                };
                Cursor::new(CursorMode::Insert(new_selection), None)
            }
        }
    }

    pub fn current_char(
        &self,
        buffer: &Buffer,
        char_width: f64,
        config: &Config,
    ) -> (f64, f64) {
        let offset = self.offset();
        let _line = buffer.line_of_offset(self.offset());
        let next = buffer.next_grapheme_offset(
            offset,
            1,
            buffer.offset_line_end(offset, true),
        );

        let (_, x0) = buffer.offset_to_line_col(offset, config.editor.tab_width);
        let (_, x1) = buffer.offset_to_line_col(next, config.editor.tab_width);
        let x0 = x0 as f64 * char_width;
        let x1 = x1 as f64 * char_width;
        (x0, x1)
    }

    pub fn lines(&self, buffer: &Buffer) -> (usize, usize) {
        match &self.mode {
            CursorMode::Normal(offset) => {
                let line = buffer.line_of_offset(*offset);
                (line, line)
            }
            #[allow(unused_variables)]
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

    pub fn yank(&self, buffer: &Buffer, tab_width: usize) -> RegisterData {
        let (content, mode) = match &self.mode {
            CursorMode::Insert(selection) => {
                let mut mode = VisualMode::Normal;
                let mut content = "".to_string();
                for region in selection.regions() {
                    let region_content = if region.is_caret() {
                        mode = VisualMode::Linewise;
                        let line = buffer.line_of_offset(region.start);
                        buffer.line_content(line)
                    } else {
                        buffer.slice_to_cow(region.min()..region.max())
                    };
                    if content.is_empty() {
                        content = region_content.to_string();
                    } else if content.ends_with('\n') {
                        content += &region_content;
                    } else {
                        content += "\n";
                        content += &region_content;
                    }
                }
                (content, mode)
            }
            CursorMode::Normal(offset) => {
                let new_offset =
                    buffer.next_grapheme_offset(*offset, 1, buffer.len());
                (
                    buffer.slice_to_cow(*offset..new_offset).to_string(),
                    VisualMode::Normal,
                )
            }
            CursorMode::Visual { start, end, mode } => match mode {
                VisualMode::Normal => (
                    buffer
                        .slice_to_cow(
                            *start.min(end)
                                ..buffer.next_grapheme_offset(
                                    *start.max(end),
                                    1,
                                    buffer.len(),
                                ),
                        )
                        .to_string(),
                    VisualMode::Normal,
                ),
                VisualMode::Linewise => {
                    let start_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.min(end)));
                    let end_offset = buffer
                        .offset_of_line(buffer.line_of_offset(*start.max(end)) + 1);
                    (
                        buffer.slice_to_cow(start_offset..end_offset).to_string(),
                        VisualMode::Linewise,
                    )
                }
                VisualMode::Blockwise => {
                    let mut lines = Vec::new();
                    let (start_line, start_col) =
                        buffer.offset_to_line_col(*start.min(end), tab_width);
                    let (end_line, end_col) =
                        buffer.offset_to_line_col(*start.max(end), tab_width);
                    let left = start_col.min(end_col);
                    let right = start_col.max(end_col) + 1;
                    for line in start_line..end_line + 1 {
                        let max_col = buffer.line_end_col(line, true, tab_width);
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
                            let left =
                                buffer.offset_of_line_col(line, left, tab_width);
                            let right =
                                buffer.offset_of_line_col(line, right, tab_width);
                            lines.push(buffer.slice_to_cow(left..right).to_string());
                        }
                    }
                    (lines.join("\n") + "\n", VisualMode::Blockwise)
                }
            },
        };
        RegisterData { content, mode }
    }

    pub fn edit_selection(&self, buffer: &Buffer, tab_width: usize) -> Selection {
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
                        buffer.offset_to_line_col(*start.min(end), tab_width);
                    let (end_line, end_col) =
                        buffer.offset_to_line_col(*start.max(end), tab_width);
                    let left = start_col.min(end_col);
                    let right = start_col.max(end_col) + 1;
                    for line in start_line..end_line + 1 {
                        let max_col = buffer.line_end_col(line, true, tab_width);
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
                        let left = buffer.offset_of_line_col(line, left, tab_width);
                        let right =
                            buffer.offset_of_line_col(line, right, tab_width);
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
                    mode: *mode,
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
    pub start: usize,
    pub end: usize,
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
        let is_forward = self.end >= self.start;
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
    last_inserted: usize,
}

impl Selection {
    pub fn new() -> Selection {
        Selection {
            regions: Vec::new(),
            last_inserted: 0,
        }
    }

    pub fn new_simple() -> Selection {
        Selection {
            regions: vec![SelRegion {
                start: 0,
                end: 0,
                horiz: None,
            }],
            last_inserted: 0,
        }
    }

    pub fn caret(offset: usize) -> Selection {
        Selection {
            regions: vec![SelRegion {
                start: offset,
                end: offset,
                horiz: None,
            }],
            last_inserted: 0,
        }
    }

    pub fn region(start: usize, end: usize) -> Selection {
        Selection {
            regions: vec![SelRegion {
                start,
                end,
                horiz: None,
            }],
            last_inserted: 0,
        }
    }

    pub fn first(&self) -> Option<&SelRegion> {
        if self.is_empty() {
            return None;
        }
        Some(&self.regions[0])
    }

    pub fn last(&self) -> Option<&SelRegion> {
        if self.is_empty() {
            return None;
        }
        Some(&self.regions[self.len() - 1])
    }

    pub fn last_inserted(&self) -> Option<&SelRegion> {
        if self.is_empty() {
            return None;
        }
        Some(&self.regions[self.last_inserted])
    }

    fn last_inserted_mut(&mut self) -> Option<&mut SelRegion> {
        if self.is_empty() {
            return None;
        }
        Some(&mut self.regions[self.last_inserted])
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
            let new_region =
                SelRegion::new(region.min(), region.max() + 1, region.horiz);
            selection.add_region(new_region);
        }
        selection
    }

    pub fn collapse(&self) -> Selection {
        let mut selection = Self::new();
        selection.add_region(self.regions[0]);
        selection
    }

    pub fn replace_last_inserted_region(&mut self, region: SelRegion) {
        if self.is_empty() {
            self.add_region(region);
            return;
        }

        self.regions.remove(self.last_inserted);
        self.add_region(region);
    }

    pub fn add_region(&mut self, region: SelRegion) {
        let mut ix = self.search(region.min());
        if ix == self.regions.len() {
            self.regions.push(region);
            self.last_inserted = self.regions.len() - 1;
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
            region = self.regions[end_ix].merge_with(region);
            end_ix += 1;
        }
        if ix == end_ix {
            self.regions.insert(ix, region);
            self.last_inserted = ix;
        } else {
            self.regions[ix] = region;
            remove_n_at(&mut self.regions, ix + 1, end_ix - ix - 1);
        }
    }

    pub fn get_cursor_offset(&self) -> usize {
        if self.is_empty() {
            return 0;
        }
        self.regions[self.last_inserted].end
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

    pub fn regions_mut(&mut self) -> &mut [SelRegion] {
        &mut self.regions
    }

    pub fn to_start_caret(&self) -> Selection {
        let region = self.regions[0];
        Selection {
            regions: vec![SelRegion {
                start: region.start,
                end: region.start,
                horiz: None,
            }],
            last_inserted: 0,
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
            last_inserted: 0,
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

impl Default for Selection {
    fn default() -> Self {
        Self::new()
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
        matches!(self, Movement::Up | Movement::Down | Movement::Line(_))
    }

    pub fn is_inclusive(&self) -> bool {
        matches!(self, Movement::WordEndForward)
    }

    pub fn is_jump(&self) -> bool {
        matches!(self, Movement::Line(_) | Movement::Offset(_))
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
    } else if index >= len as i64 {
        len - 1
    } else if index < 0 {
        0
    } else {
        index as usize
    }
}
