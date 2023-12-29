use std::cmp::{max, min, Ordering};

use lapce_xi_rope::{RopeDelta, Transformer};
use serde::{Deserialize, Serialize};

use crate::cursor::ColPosition;

/// Indicate whether a delta should be applied inside, outside non-caret selection or
/// after a caret selection (see [`Selection::apply_delta`].
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
pub struct SelRegion {
    /// Region start offset
    pub start: usize,
    /// Region end offset
    pub end: usize,
    /// Horizontal rules for multiple selection
    pub horiz: Option<ColPosition>,
}

/// A selection holding one or more [`SelRegion`].
/// Regions are kept in order from the leftmost selection to the rightmost selection.
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
    /// Creates new [`SelRegion`] from `start` and `end` offset.
    pub fn new(start: usize, end: usize, horiz: Option<ColPosition>) -> SelRegion {
        SelRegion { start, end, horiz }
    }

    /// Creates a caret [`SelRegion`],
    /// i.e. `start` and `end` position are both set to `offset` value.
    pub fn caret(offset: usize) -> SelRegion {
        SelRegion {
            start: offset,
            end: offset,
            horiz: None,
        }
    }

    /// Return the minimum value between region's start and end position
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::SelRegion;
    /// let  region = SelRegion::new(1, 10, None);
    /// assert_eq!(region.min(), region.start);
    /// let  region = SelRegion::new(42, 1, None);
    /// assert_eq!(region.min(), region.end);
    /// ```
    pub fn min(self) -> usize {
        min(self.start, self.end)
    }

    /// Return the maximum value between region's start and end position.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::SelRegion;
    /// let  region = SelRegion::new(1, 10, None);
    /// assert_eq!(region.max(), region.end);
    /// let  region = SelRegion::new(42, 1, None);
    /// assert_eq!(region.max(), region.start);
    /// ```
    pub fn max(self) -> usize {
        max(self.start, self.end)
    }

    /// A [`SelRegion`] is considered to be a caret when its start and end position are equal.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::SelRegion;
    /// let  region = SelRegion::new(1, 1, None);
    /// assert!(region.is_caret());
    /// ```
    pub fn is_caret(self) -> bool {
        self.start == self.end
    }

    /// Merge two [`SelRegion`] into a single one.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::SelRegion;
    /// let  region = SelRegion::new(1, 2, None);
    /// let  other = SelRegion::new(3, 4, None);
    /// assert_eq!(region.merge_with(other), SelRegion::new(1, 4, None));
    /// ```
    pub fn merge_with(self, other: SelRegion) -> SelRegion {
        let is_forward = self.end >= self.start;
        let new_min = min(self.min(), other.min());
        let new_max = max(self.max(), other.max());
        let (start, end) = if is_forward {
            (new_min, new_max)
        } else {
            (new_max, new_min)
        };
        SelRegion::new(start, end, None)
    }

    fn should_merge(self, other: SelRegion) -> bool {
        other.min() < self.max()
            || ((self.is_caret() || other.is_caret()) && other.min() == self.max())
    }

    fn contains(&self, offset: usize) -> bool {
        self.min() <= offset && offset <= self.max()
    }
}

impl Selection {
    /// Creates a new empty [`Selection`]
    pub fn new() -> Selection {
        Selection {
            regions: Vec::new(),
            last_inserted: 0,
        }
    }

    /// Creates a caret [`Selection`], i.e. a selection with a single caret [`SelRegion`]
    pub fn caret(offset: usize) -> Selection {
        Selection {
            regions: vec![SelRegion::caret(offset)],
            last_inserted: 0,
        }
    }

    /// Creates a region [`Selection`], i.e. a selection with a single [`SelRegion`]
    /// from `start` to `end` position
    pub fn region(start: usize, end: usize) -> Self {
        Self::sel_region(SelRegion {
            start,
            end,
            horiz: None,
        })
    }

    /// Creates a [`Selection`], with a single [`SelRegion`] equal to `region`.
    pub fn sel_region(region: SelRegion) -> Self {
        Self {
            regions: vec![region],
            last_inserted: 0,
        }
    }

    /// Returns whether this [`Selection`], contains the given `offset` position or not.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::Selection;
    /// let  selection = Selection::region(0, 2);
    /// assert!(selection.contains(0));
    /// assert!(selection.contains(1));
    /// assert!(selection.contains(2));
    /// assert!(!selection.contains(3));
    /// ```
    pub fn contains(&self, offset: usize) -> bool {
        for region in self.regions.iter() {
            if region.contains(offset) {
                return true;
            }
        }
        false
    }

    /// Returns this selection regions
    pub fn regions(&self) -> &[SelRegion] {
        &self.regions
    }

    /// Returns a mutable reference to this selection regions
    pub fn regions_mut(&mut self) -> &mut [SelRegion] {
        &mut self.regions
    }

    /// Returns a copy of [`self`] with all regions converted to caret region at their respective
    /// [`SelRegion::min`] offset.
    ///
    /// **Examples:**
    ///
    /// ```rust
    /// # use lapce_core::selection::{Selection, SelRegion};
    /// let mut selection = Selection::new();
    /// selection.add_region(SelRegion::new(1, 3, None));
    /// selection.add_region(SelRegion::new(6, 12, None));
    /// selection.add_region(SelRegion::new(24, 48, None));
    ///
    /// assert_eq!(selection.min().regions(), vec![
    ///     SelRegion::caret(1),
    ///     SelRegion::caret(6),
    ///     SelRegion::caret(24)
    /// ]);
    pub fn min(&self) -> Selection {
        let mut selection = Self::new();
        for region in &self.regions {
            let new_region = SelRegion::new(region.min(), region.min(), None);
            selection.add_region(new_region);
        }
        selection
    }

    /// Get the leftmost [`SelRegion`] in this selection if present.
    pub fn first(&self) -> Option<&SelRegion> {
        self.regions.first()
    }

    /// Get the rightmost [`SelRegion`] in this selection if present.
    pub fn last(&self) -> Option<&SelRegion> {
        self.regions.get(self.len() - 1)
    }

    /// Get the last inserted [`SelRegion`] in this selection if present.
    pub fn last_inserted(&self) -> Option<&SelRegion> {
        self.regions.get(self.last_inserted)
    }

    /// Get a mutable reference to the last inserted [`SelRegion`] in this selection if present.
    pub fn last_inserted_mut(&mut self) -> Option<&mut SelRegion> {
        self.regions.get_mut(self.last_inserted)
    }

    /// The number of [`SelRegion`] in this selection.
    pub fn len(&self) -> usize {
        self.regions.len()
    }

    /// A [`Selection`] is considered to be a caret if it contains
    /// only caret [`SelRegion`] (see [`SelRegion::is_caret`])
    pub fn is_caret(&self) -> bool {
        self.regions.iter().all(|region| region.is_caret())
    }

    /// Returns `true` if `self` has zero [`SelRegion`]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the minimal offset across all region of this selection.
    ///
    /// This function panics if the selection is empty.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::{Selection, SelRegion};
    /// let mut selection = Selection::new();
    /// selection.add_region(SelRegion::caret(4));
    /// selection.add_region(SelRegion::new(0, 12, None));
    /// selection.add_region(SelRegion::new(24, 48, None));
    /// assert_eq!(selection.min_offset(), 0);
    /// ```
    pub fn min_offset(&self) -> usize {
        let mut offset = self.regions()[0].min();
        for region in &self.regions {
            offset = offset.min(region.min());
        }
        offset
    }

    /// Returns the maximal offset across all region of this selection.
    ///
    /// This function panics if the selection is empty.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::{Selection, SelRegion};
    /// let mut selection = Selection::new();
    /// selection.add_region(SelRegion::caret(4));
    /// selection.add_region(SelRegion::new(0, 12, None));
    /// selection.add_region(SelRegion::new(24, 48, None));
    /// assert_eq!(selection.max_offset(), 48);
    /// ```
    pub fn max_offset(&self) -> usize {
        let mut offset = self.regions()[0].max();
        for region in &self.regions {
            offset = offset.max(region.max());
        }
        offset
    }

    /// Returns regions in [`self`] overlapping or fully enclosed in the provided
    /// `start` to `end` range.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::{Selection, SelRegion};
    /// let mut selection = Selection::new();
    /// selection.add_region(SelRegion::new(0, 3, None));
    /// selection.add_region(SelRegion::new(3, 6, None));
    /// selection.add_region(SelRegion::new(7, 8, None));
    /// selection.add_region(SelRegion::new(9, 11, None));
    /// let regions = selection.regions_in_range(5, 10);
    /// assert_eq!(regions, vec![
    ///     SelRegion::new(3, 6, None),
    ///     SelRegion::new(7, 8, None),
    ///     SelRegion::new(9, 11, None)
    /// ]);
    /// ```
    pub fn regions_in_range(&self, start: usize, end: usize) -> &[SelRegion] {
        let first = self.search(start);
        let mut last = self.search(end);
        if last < self.regions.len() && self.regions[last].min() <= end {
            last += 1;
        }
        &self.regions[first..last]
    }

    /// Returns regions in [`self`] starting between `start` to `end` range.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::{Selection, SelRegion};
    /// let mut selection = Selection::new();
    /// selection.add_region(SelRegion::new(0, 3, None));
    /// selection.add_region(SelRegion::new(3, 6, None));
    /// selection.add_region(SelRegion::new(7, 8, None));
    /// selection.add_region(SelRegion::new(9, 11, None));
    /// let regions = selection.full_regions_in_range(5, 10);
    /// assert_eq!(regions, vec![
    ///     SelRegion::new(7, 8, None),
    ///     SelRegion::new(9, 11, None)
    /// ]);
    /// ```
    pub fn full_regions_in_range(&self, start: usize, end: usize) -> &[SelRegion] {
        let first = self.search_min(start);
        let mut last = self.search_min(end);
        if last < self.regions.len() && self.regions[last].min() <= end {
            last += 1;
        }
        &self.regions[first..last]
    }

    /// Deletes regions in [`self`] overlapping or enclosing in `start` to `end` range.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::{Selection, SelRegion};
    /// let mut selection = Selection::new();
    /// selection.add_region(SelRegion::new(0, 3, None));
    /// selection.add_region(SelRegion::new(3, 6, None));
    /// selection.add_region(SelRegion::new(7, 8, None));
    /// selection.add_region(SelRegion::new(9, 11, None));
    /// selection.delete_range(5, 10);
    /// assert_eq!(selection.regions(), vec![SelRegion::new(0, 3, None)]);
    /// ```
    pub fn delete_range(&mut self, start: usize, end: usize) {
        let mut first = self.search(start);
        let mut last = self.search(end);
        if first >= self.regions.len() {
            return;
        }
        if self.regions[first].max() == start {
            first += 1;
        }
        if last < self.regions.len() && self.regions[last].min() < end {
            last += 1;
        }
        remove_n_at(&mut self.regions, first, last - first);
    }

    /// Add a regions to [`self`]. Note that if provided region overlap
    /// on of the selection regions they will be merged in a single region.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::selection::{Selection, SelRegion};
    /// let mut selection = Selection::new();
    /// // Overlapping
    /// selection.add_region(SelRegion::new(0, 4, None));
    /// selection.add_region(SelRegion::new(3, 6, None));
    /// assert_eq!(selection.regions(), vec![SelRegion::new(0, 6, None)]);
    /// // Non-overlapping
    /// let mut selection = Selection::new();
    /// selection.add_region(SelRegion::new(0, 3, None));
    /// selection.add_region(SelRegion::new(3, 6, None));
    /// assert_eq!(selection.regions(), vec![
    ///     SelRegion::new(0, 3, None),
    ///     SelRegion::new(3, 6, None)
    /// ]);
    /// ```
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

    /// Add a region to the selection. This method does not merge regions and does not allow
    /// ambiguous regions (regions that overlap).
    ///
    /// On ambiguous regions, the region with the lower start position wins. That is, in such a
    /// case, the new region is either not added at all, because there is an ambiguous region with
    /// a lower start position, or existing regions that intersect with the new region but do
    /// not start before the new region, are deleted.
    pub fn add_range_distinct(&mut self, region: SelRegion) -> (usize, usize) {
        let mut ix = self.search(region.min());

        if ix < self.regions.len() && self.regions[ix].max() == region.min() {
            ix += 1;
        }

        if ix < self.regions.len() {
            // in case of ambiguous regions the region closer to the left wins
            let occ = &self.regions[ix];
            let is_eq = occ.min() == region.min() && occ.max() == region.max();
            let is_intersect_before =
                region.min() >= occ.min() && occ.max() > region.min();
            if is_eq || is_intersect_before {
                return (occ.min(), occ.max());
            }
        }

        // delete ambiguous regions to the right
        let mut last = self.search(region.max());
        if last < self.regions.len() && self.regions[last].min() < region.max() {
            last += 1;
        }
        remove_n_at(&mut self.regions, ix, last - ix);

        if ix == self.regions.len() {
            self.regions.push(region);
        } else {
            self.regions.insert(ix, region);
        }

        (self.regions[ix].min(), self.regions[ix].max())
    }

    /// Apply [`xi_rope::RopeDelta`] to this selection.
    /// Typically used to apply an edit to a buffer and update its selections
    /// **Parameters*:*
    /// - `delta`[`xi_rope::RopeDelta`]
    /// - `after` parameter indicate if the delta should be applied before or after the selection
    /// - `drift` see [`InsertDrift`]
    pub fn apply_delta(
        &self,
        delta: &RopeDelta,
        after: bool,
        drift: InsertDrift,
    ) -> Selection {
        let mut result = Selection::new();
        let mut transformer = Transformer::new(delta);
        for region in self.regions() {
            let is_region_forward = region.start < region.end;

            let (start_after, end_after) = match (drift, region.is_caret()) {
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

    /// Returns cursor position, which corresponds to last inserted region `end` offset,
    pub fn get_cursor_offset(&self) -> usize {
        if self.is_empty() {
            return 0;
        }
        self.regions[self.last_inserted].end
    }

    /// Replaces last inserted [`SelRegion`] of this selection with the provided one.
    pub fn replace_last_inserted_region(&mut self, region: SelRegion) {
        if self.is_empty() {
            self.add_region(region);
            return;
        }

        self.regions.remove(self.last_inserted);
        self.add_region(region);
    }

    fn search(&self, offset: usize) -> usize {
        if self.regions.is_empty() || offset > self.regions.last().unwrap().max() {
            return self.regions.len();
        }
        match self.regions.binary_search_by(|r| r.max().cmp(&offset)) {
            Ok(ix) => ix,
            Err(ix) => ix,
        }
    }

    fn search_min(&self, offset: usize) -> usize {
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
}

impl Default for Selection {
    fn default() -> Self {
        Self::new()
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

#[cfg(test)]
mod test {
    use crate::{
        buffer::Buffer,
        editor::EditType,
        selection::{InsertDrift, SelRegion, Selection},
    };

    #[test]
    fn should_return_selection_region_min() {
        let region = SelRegion::new(1, 10, None);
        assert_eq!(region.min(), region.start);

        let region = SelRegion::new(42, 1, None);
        assert_eq!(region.min(), region.end);
    }

    #[test]
    fn should_return_selection_region_max() {
        let region = SelRegion::new(1, 10, None);
        assert_eq!(region.max(), region.end);

        let region = SelRegion::new(42, 1, None);
        assert_eq!(region.max(), region.start);
    }

    #[test]
    fn is_caret_should_return_true() {
        let region = SelRegion::new(1, 10, None);
        assert!(!region.is_caret());
    }

    #[test]
    fn is_caret_should_return_false() {
        let region = SelRegion::new(1, 1, None);
        assert!(region.is_caret());
    }

    #[test]
    fn should_merge_regions() {
        let region = SelRegion::new(1, 2, None);
        let other = SelRegion::new(3, 4, None);
        assert_eq!(region.merge_with(other), SelRegion::new(1, 4, None));

        let region = SelRegion::new(2, 1, None);
        let other = SelRegion::new(4, 3, None);
        assert_eq!(region.merge_with(other), SelRegion::new(4, 1, None));

        let region = SelRegion::new(1, 1, None);
        let other = SelRegion::new(6, 6, None);
        assert_eq!(region.merge_with(other), SelRegion::new(1, 6, None));
    }

    #[test]
    fn selection_should_be_caret() {
        let mut selection = Selection::new();
        selection.add_region(SelRegion::caret(1));
        selection.add_region(SelRegion::caret(6));
        assert!(selection.is_caret());
    }

    #[test]
    fn selection_should_not_be_caret() {
        let mut selection = Selection::new();
        selection.add_region(SelRegion::caret(1));
        selection.add_region(SelRegion::new(4, 6, None));
        assert!(!selection.is_caret());
    }

    #[test]
    fn should_return_min_selection() {
        let mut selection = Selection::new();
        selection.add_region(SelRegion::new(1, 3, None));
        selection.add_region(SelRegion::new(4, 6, None));
        assert_eq!(
            selection.min().regions,
            vec![SelRegion::caret(1), SelRegion::caret(4)]
        );
    }

    #[test]
    fn selection_should_contains_region() {
        let selection = Selection::region(0, 2);
        assert!(selection.contains(0));
        assert!(selection.contains(1));
        assert!(selection.contains(2));
        assert!(!selection.contains(3));
    }

    #[test]
    fn should_return_last_inserted_region() {
        let mut selection = Selection::region(5, 6);
        selection.add_region(SelRegion::caret(1));
        assert_eq!(selection.last_inserted(), Some(&SelRegion::caret(1)));
    }

    #[test]
    fn should_return_last_region() {
        let mut selection = Selection::region(5, 6);
        selection.add_region(SelRegion::caret(1));
        assert_eq!(selection.last(), Some(&SelRegion::new(5, 6, None)));
    }

    #[test]
    fn should_return_first_region() {
        let mut selection = Selection::region(5, 6);
        selection.add_region(SelRegion::caret(1));
        assert_eq!(selection.first(), Some(&SelRegion::caret(1)));
    }

    #[test]
    fn should_return_regions_in_range() {
        let mut selection = Selection::new();
        selection.add_region(SelRegion::new(0, 3, None));
        selection.add_region(SelRegion::new(3, 6, None));
        selection.add_region(SelRegion::new(7, 8, None));
        selection.add_region(SelRegion::new(9, 11, None));

        let regions = selection.regions_in_range(5, 10);

        assert_eq!(
            regions,
            vec![
                SelRegion::new(3, 6, None),
                SelRegion::new(7, 8, None),
                SelRegion::new(9, 11, None),
            ]
        );
    }

    #[test]
    fn should_return_regions_in_full_range() {
        let mut selection = Selection::new();
        selection.add_region(SelRegion::new(0, 3, None));
        selection.add_region(SelRegion::new(3, 6, None));
        selection.add_region(SelRegion::new(7, 8, None));
        selection.add_region(SelRegion::new(9, 11, None));

        let regions = selection.full_regions_in_range(5, 10);

        assert_eq!(
            regions,
            vec![SelRegion::new(7, 8, None), SelRegion::new(9, 11, None),]
        );
    }

    #[test]
    fn should_delete_regions() {
        let mut selection = Selection::new();
        selection.add_region(SelRegion::new(0, 3, None));
        selection.add_region(SelRegion::new(3, 6, None));
        selection.add_region(SelRegion::new(7, 8, None));
        selection.add_region(SelRegion::new(9, 11, None));
        selection.delete_range(5, 10);
        assert_eq!(selection.regions(), vec![SelRegion::new(0, 3, None)]);
    }

    #[test]
    fn should_add_regions() {
        let mut selection = Selection::new();
        selection.add_region(SelRegion::new(0, 3, None));
        selection.add_region(SelRegion::new(3, 6, None));
        assert_eq!(
            selection.regions(),
            vec![SelRegion::new(0, 3, None), SelRegion::new(3, 6, None),]
        );
    }

    #[test]
    fn should_add_and_merge_regions() {
        let mut selection = Selection::new();

        selection.add_region(SelRegion::new(0, 4, None));
        selection.add_region(SelRegion::new(3, 6, None));
        assert_eq!(selection.regions(), vec![SelRegion::new(0, 6, None)]);
    }

    #[test]
    fn should_apply_delta_after_insertion() {
        let selection = Selection::caret(0);

        let (mock_delta, _, _) = {
            let mut buffer = Buffer::new("");
            buffer.edit(&[(selection.clone(), "Hello")], EditType::InsertChars)
        };

        assert_eq!(
            selection.apply_delta(&mock_delta, true, InsertDrift::Inside),
            Selection::caret(5)
        );
    }
}
