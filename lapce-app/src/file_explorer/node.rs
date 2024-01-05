use floem::views::VirtualVector;
use lapce_rpc::file::{FileNodeItem, FileNodeViewData};

use lapce_rpc::file::RenameState;

pub struct FileNodeVirtualList {
    file_node_item: FileNodeItem,
    rename_state: RenameState,
}

impl FileNodeVirtualList {
    pub fn new(file_node_item: FileNodeItem, rename_state: RenameState) -> Self {
        Self {
            file_node_item,
            rename_state,
        }
    }
}

impl VirtualVector<FileNodeViewData> for FileNodeVirtualList {
    fn total_len(&self) -> usize {
        self.file_node_item.children_open_count
    }

    fn slice(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = FileNodeViewData> {
        let min = range.start;
        let max = range.end;
        let mut i = 0;
        let mut view_items = Vec::new();
        for item in self.file_node_item.sorted_children() {
            i = item.append_view_slice(
                &mut view_items,
                &self.rename_state,
                min,
                max,
                i + 1,
                0,
            );
            if i > max {
                return view_items.into_iter();
            }
        }

        view_items.into_iter()
    }
}
