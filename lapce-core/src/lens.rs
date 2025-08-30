use std::mem;

use lapce_xi_rope::{
    Cursor, Delta, Interval, Metric,
    interval::IntervalBounds,
    tree::{DefaultMetric, Leaf, Node, NodeInfo, TreeBuilder},
};

const MIN_LEAF: usize = 5;
const MAX_LEAF: usize = 10;

pub type LensNode = Node<LensInfo>;

#[derive(Clone)]
pub struct Lens(LensNode);

#[derive(Clone, Debug)]
pub struct LensInfo(usize);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LensData {
    len: usize,
    line_height: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LensLeaf {
    len: usize,
    data: Vec<LensData>,
    total_height: usize,
}

pub struct LensIter<'a> {
    cursor: Cursor<'a, LensInfo>,
    end: usize,
}

impl Lens {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn line_of_height(&self, height: usize) -> usize {
        let max_height = self.0.count::<LensMetric>(self.0.len());
        if height >= max_height {
            return self.0.len();
        }
        self.0.count_base_units::<LensMetric>(height)
    }

    pub fn height_of_line(&self, line: usize) -> usize {
        let line = self.0.len().min(line);
        self.0.count::<LensMetric>(line)
    }

    pub fn iter(&self) -> LensIter<'_> {
        LensIter {
            cursor: Cursor::new(&self.0, 0),
            end: self.len(),
        }
    }

    pub fn iter_chunks<I: IntervalBounds>(&self, range: I) -> LensIter<'_> {
        let Interval { start, end } = range.into_interval(self.len());

        LensIter {
            cursor: Cursor::new(&self.0, start),
            end,
        }
    }

    pub fn apply_delta<M: NodeInfo>(&mut self, _delta: &Delta<M>) {}
}

impl NodeInfo for LensInfo {
    type L = LensLeaf;

    fn accumulate(&mut self, other: &Self) {
        self.0 += other.0;
    }

    fn compute_info(l: &LensLeaf) -> LensInfo {
        LensInfo(l.total_height)
    }
}

impl Leaf for LensLeaf {
    fn len(&self) -> usize {
        self.len
    }

    fn is_ok_child(&self) -> bool {
        self.data.len() >= MIN_LEAF
    }

    fn push_maybe_split(
        &mut self,
        other: &LensLeaf,
        iv: Interval,
    ) -> Option<LensLeaf> {
        let (iv_start, iv_end) = iv.start_end();
        let mut accum = 0;
        let mut added_len = 0;
        let mut added_height = 0;
        for sec in &other.data {
            if accum + sec.len < iv_start {
                accum += sec.len;
                continue;
            }

            if accum + sec.len <= iv_end {
                accum += sec.len;
                self.data.push(LensData {
                    len: sec.len,
                    line_height: sec.line_height,
                });
                added_len += sec.len;
                added_height += sec.len * sec.line_height;
                continue;
            }

            let len = iv_end - (accum + sec.len);
            self.data.push(LensData {
                len,
                line_height: sec.line_height,
            });
            added_len += len;
            added_height += sec.len * sec.line_height;
            break;
        }
        self.len += added_len;
        self.total_height += added_height;

        if self.data.len() <= MAX_LEAF {
            None
        } else {
            let splitpoint = self.data.len() / 2; // number of spans
            let new = self.data.split_off(splitpoint);
            let new_len = new.iter().map(|d| d.len).sum();
            let new_height = new.iter().map(|d| d.len * d.line_height).sum();
            self.len -= new_len;
            self.total_height -= new_height;
            Some(LensLeaf {
                len: new_len,
                data: new,
                total_height: new_height,
            })
        }
    }
}

#[derive(Copy, Clone)]
pub struct LensMetric(());

impl Metric<LensInfo> for LensMetric {
    fn measure(info: &LensInfo, _len: usize) -> usize {
        info.0
    }

    fn to_base_units(l: &LensLeaf, in_measured_units: usize) -> usize {
        if in_measured_units > l.total_height {
            l.len
        } else if in_measured_units == 0 {
            0
        } else {
            let mut line = 0;
            let mut accum = 0;
            for data in l.data.iter() {
                let leaf_height = data.line_height * data.len;
                let accum_height = accum + leaf_height;
                if accum_height > in_measured_units {
                    return line + (in_measured_units - accum) / data.line_height;
                }
                accum = accum_height;
                line += data.len;
            }
            line
        }
    }

    fn from_base_units(l: &LensLeaf, in_base_units: usize) -> usize {
        let mut line = 0;
        let mut accum = 0;
        for data in l.data.iter() {
            if in_base_units < line + data.len {
                return accum + (in_base_units - line) * data.line_height;
            }
            accum += data.len * data.line_height;
            line += data.len;
        }
        accum
    }

    fn is_boundary(_l: &LensLeaf, _offset: usize) -> bool {
        true
    }

    fn prev(_l: &LensLeaf, offset: usize) -> Option<usize> {
        if offset == 0 { None } else { Some(offset - 1) }
    }

    fn next(l: &LensLeaf, offset: usize) -> Option<usize> {
        if offset < l.len {
            Some(offset + 1)
        } else {
            None
        }
    }

    fn can_fragment() -> bool {
        false
    }
}

impl DefaultMetric for LensInfo {
    type DefaultMetric = LensBaseMetric;
}

#[derive(Copy, Clone)]
pub struct LensBaseMetric(());

impl Metric<LensInfo> for LensBaseMetric {
    fn measure(_: &LensInfo, len: usize) -> usize {
        len
    }

    fn to_base_units(_: &LensLeaf, in_measured_units: usize) -> usize {
        in_measured_units
    }

    fn from_base_units(_: &LensLeaf, in_base_units: usize) -> usize {
        in_base_units
    }

    fn is_boundary(l: &LensLeaf, offset: usize) -> bool {
        LensMetric::is_boundary(l, offset)
    }

    fn prev(l: &LensLeaf, offset: usize) -> Option<usize> {
        LensMetric::prev(l, offset)
    }

    fn next(l: &LensLeaf, offset: usize) -> Option<usize> {
        LensMetric::next(l, offset)
    }

    fn can_fragment() -> bool {
        false
    }
}

pub struct LensBuilder {
    b: TreeBuilder<LensInfo>,
    leaf: LensLeaf,
}

impl Default for LensBuilder {
    fn default() -> LensBuilder {
        LensBuilder {
            b: TreeBuilder::new(),
            leaf: LensLeaf::default(),
        }
    }
}

impl LensBuilder {
    pub fn new() -> LensBuilder {
        LensBuilder::default()
    }

    pub fn add_section(&mut self, len: usize, line_height: usize) {
        if self.leaf.data.len() == MAX_LEAF {
            let leaf = mem::take(&mut self.leaf);
            self.b.push(Node::from_leaf(leaf));
        }
        self.leaf.len += len;
        self.leaf.total_height += len * line_height;
        self.leaf.data.push(LensData { len, line_height });
    }

    pub fn build(mut self) -> Lens {
        self.b.push(Node::from_leaf(self.leaf));
        Lens(self.b.build())
    }
}

impl Iterator for LensIter<'_> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor.pos() >= self.end {
            return None;
        }
        if let Some((leaf, leaf_pos)) = self.cursor.get_leaf() {
            if leaf.data.is_empty() {
                return None;
            }
            let line = self.cursor.pos();
            self.cursor.next::<LensMetric>();

            let mut lines = 0;
            for data in leaf.data.iter() {
                if leaf_pos < data.len + lines {
                    return Some((line, data.line_height));
                }
                lines += data.len;
            }
            return None;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lens_metric() {
        let mut builder = LensBuilder::new();
        builder.add_section(10, 2);
        builder.add_section(1, 25);
        builder.add_section(20, 3);
        let lens = builder.build();

        assert_eq!(31, lens.len());
        assert_eq!(0, lens.height_of_line(0));
        assert_eq!(2, lens.height_of_line(1));
        assert_eq!(20, lens.height_of_line(10));
        assert_eq!(45, lens.height_of_line(11));
        assert_eq!(48, lens.height_of_line(12));
        assert_eq!(105, lens.height_of_line(31));
        assert_eq!(105, lens.height_of_line(32));
        assert_eq!(105, lens.height_of_line(62));

        assert_eq!(0, lens.line_of_height(0));
        assert_eq!(0, lens.line_of_height(1));
        assert_eq!(1, lens.line_of_height(2));
        assert_eq!(1, lens.line_of_height(3));
        assert_eq!(2, lens.line_of_height(4));
        assert_eq!(2, lens.line_of_height(5));
        assert_eq!(3, lens.line_of_height(6));
        assert_eq!(10, lens.line_of_height(20));
        assert_eq!(10, lens.line_of_height(44));
        assert_eq!(11, lens.line_of_height(45));
        assert_eq!(11, lens.line_of_height(46));
        assert_eq!(31, lens.line_of_height(105));
        assert_eq!(31, lens.line_of_height(106));
    }

    #[test]
    fn test_lens_iter() {
        let mut builder = LensBuilder::new();
        builder.add_section(10, 2);
        builder.add_section(1, 25);
        builder.add_section(2, 3);
        let lens = builder.build();

        let mut iter = lens.iter();
        assert_eq!(Some((0, 2)), iter.next());
        assert_eq!(Some((1, 2)), iter.next());
        assert_eq!(Some((2, 2)), iter.next());
        for _ in 0..7 {
            iter.next();
        }
        assert_eq!(Some((10, 25)), iter.next());
        assert_eq!(Some((11, 3)), iter.next());
        assert_eq!(Some((12, 3)), iter.next());
        assert_eq!(None, iter.next());

        let mut iter = lens.iter_chunks(9..12);
        assert_eq!(Some((9, 2)), iter.next());
        assert_eq!(Some((10, 25)), iter.next());
        assert_eq!(Some((11, 3)), iter.next());
        assert_eq!(None, iter.next());

        let mut iter = lens.iter_chunks(9..15);
        assert_eq!(Some((9, 2)), iter.next());
        assert_eq!(Some((10, 25)), iter.next());
        assert_eq!(Some((11, 3)), iter.next());
        assert_eq!(Some((12, 3)), iter.next());
        assert_eq!(None, iter.next());
    }
}
