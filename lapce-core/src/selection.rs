use serde::{Deserialize, Serialize};
use std::cmp::{max, min, Ordering};
use xi_rope::{RopeDelta, Transformer};

use crate::{buffer::Buffer, mode::Mode, movement::Movement, syntax::Syntax};

#[derive(Copy, Clone)]
pub enum InsertDrift {
    /// Indicates this edit should happen within any (non-caret) selections if possible.
    Inside,
    /// Indicates this edit should happen outside any selections if possible.
    Outside,
    /// Indicates to do whatever the `after` bool says to do
    Default,
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
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Selection {
    regions: Vec<SelRegion>,
    last_inserted: usize,
}

impl AsRef<Selection> for Selection {
    fn as_ref(&self) -> &Selection {
        self
    }
}

impl SelRegion {
    pub fn new(start: usize, end: usize) -> SelRegion {
        SelRegion { start, end }
    }

    pub fn caret(offset: usize) -> SelRegion {
        SelRegion {
            start: offset,
            end: offset,
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
        SelRegion::new(start, end)
    }

    pub fn do_movement(
        &self,
        movement: &Movement,
        mode: Mode,
        count: usize,
        modify: bool,
        buffer: &Buffer,
        syntax: Option<&Syntax>,
        horiz: Option<&ColPosition>,
    ) -> SelRegion {
        let (end, _) =
            buffer.move_offset(self.end, horiz, count, movement, mode, syntax);
        let start = match modify {
            true => self.start(),
            false => end,
        };
        SelRegion::new(start, end)
    }
}

impl Selection {
    pub fn new() -> Selection {
        Selection {
            regions: Vec::new(),
            last_inserted: 0,
        }
    }

    pub fn caret(offset: usize) -> Selection {
        Selection {
            regions: vec![SelRegion::caret(offset)],
            last_inserted: 0,
        }
    }

    pub fn region(start: usize, end: usize) -> Selection {
        Selection {
            regions: vec![SelRegion { start, end }],
            last_inserted: 0,
        }
    }

    pub fn regions(&self) -> &[SelRegion] {
        &self.regions
    }

    pub fn regions_mut(&mut self) -> &mut [SelRegion] {
        &mut self.regions
    }

    pub fn min(&self) -> Selection {
        let mut selection = Self::new();
        for region in &self.regions {
            let new_region = SelRegion::new(region.min(), region.min());
            selection.add_region(new_region);
        }
        selection
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

    pub fn search(&self, offset: usize) -> usize {
        if self.regions.is_empty() || offset > self.regions.last().unwrap().max() {
            return self.regions.len();
        }
        match self.regions.binary_search_by(|r| r.max().cmp(&offset)) {
            Ok(ix) => ix,
            Err(ix) => ix,
        }
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
            );
            result.add_region(new_region);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub fn do_movement(
        &self,
        movement: &Movement,
        mode: Mode,
        count: usize,
        modify: bool,
        buffer: &Buffer,
        syntax: Option<&Syntax>,
        horiz: Option<&ColPosition>,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in self.regions() {
            new_selection.add_region(
                region.do_movement(
                    movement, mode, count, modify, buffer, syntax, horiz,
                ),
            );
        }
        new_selection
    }

    pub fn get_cursor_offset(&self) -> usize {
        if self.is_empty() {
            return 0;
        }
        self.regions[self.last_inserted].end
    }
}

fn remove_n_at<T>(v: &mut Vec<T>, index: usize, n: usize) {
    match n.cmp(&1) {
        Ordering::Equal => {
            v.remove(index);
        }
        Ordering::Greater => {
            v.drain(index..index + n);
        }
        _ => (),
    };
}
