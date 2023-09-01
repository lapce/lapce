use floem::views::VirtualListVector;

use lapce_rpc::file::{FileNodeItem, FileNodeViewData};

pub struct FileNodeVirtualList(pub FileNodeItem);

impl VirtualListVector<FileNodeViewData> for FileNodeVirtualList {
    type ItemIterator = Box<dyn Iterator<Item = FileNodeViewData>>;

    fn total_len(&self) -> usize {
        self.0.children_open_count
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> Self::ItemIterator {
        let min = range.start;
        let max = range.end;
        let mut i = 0;
        let mut view_items = Vec::new();
        for item in self.0.sorted_children() {
            i = item.append_view_slice(&mut view_items, min, max, i + 1, 0);
            if i > max {
                return Box::new(view_items.into_iter());
            }
        }

        Box::new(view_items.into_iter())
    }
}
