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
        self.file_node_item.children_open_count
    }

    fn slice(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = FileNodeViewData> {
        let naming = &self.naming;
        let root = &self.file_node_item;

        let min = range.start;
        let max = range.end;
        let mut i = 0;
        let mut view_items = Vec::new();

        let mut naming_on_root = naming.extra_node(root.is_dir, 0, &root.path);

        let naming_is_dir =
            naming_on_root.as_ref().map(|n| n.is_dir).unwrap_or(false);
        // Immediately put the naming entry first if it's a directory
        if naming_is_dir {
            if let Some(node) = naming_on_root.take() {
                // Actually add the node if it's within the range
                if 0 >= min {
                    view_items.push(node);
                    i += 1;
                }
            }
        }

        let mut after_dirs = false;

        for item in root.sorted_children() {
            // If we're naming a file at the root, then wait until we've added the directories
            // before adding the input node
            if naming_on_root.is_some()
                && !naming_is_dir
                && !item.is_dir
                && !after_dirs
            {
                after_dirs = true;

                // If we're creating a new file node, then we show it after the directories
                // TODO(minor): should this be i >= min or i + 1 >= min?
                if i >= min {
                    if let Some(node) = naming_on_root.take() {
                        view_items.push(node);
                        i += 1;
                    }
                }
            }

            i = item.append_view_slice(&mut view_items, naming, min, max, i + 1, 0);

            if i > max {
                break;
            }
        }

        if i >= min {
            if let Some(node) = naming_on_root {
                view_items.push(node);
                // i += 1;
            }
        }

        view_items.into_iter()
    }
}
