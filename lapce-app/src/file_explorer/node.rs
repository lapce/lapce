use floem::views::VirtualVector;
use lapce_rpc::file::{FileNodeItem, FileNodeViewData, Naming};

pub struct FileNodeVirtualList {
    file_node_item: FileNodeItem,
    naming: Naming,
}

impl FileNodeVirtualList {
    pub fn new(file_node_item: FileNodeItem, naming: Naming) -> Self {
        Self {
            file_node_item,
            naming,
        }
    }
}

impl VirtualVector<FileNodeViewData> for FileNodeVirtualList {
    fn total_len(&self) -> usize {
        self.file_node_item.children_open_count + 1
    }

    fn slice(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = FileNodeViewData> {
        let naming = &self.naming;
        let root = &self.file_node_item;

        let min = range.start;
        let max = range.end;
        let mut view_items = Vec::new();

        root.append_view_slice(&mut view_items, naming, min, max, 0, 1);

        view_items.into_iter()
    }
}
