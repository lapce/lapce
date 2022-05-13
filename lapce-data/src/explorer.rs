use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use druid::ExtEventSink;
use druid::{Target, WidgetId};

use lapce_rpc::file::FileNodeItem;
use lapce_rpc::proxy::ReadDirResponse;

use crate::data::LapceWorkspace;
use crate::proxy::LapceProxy;

use crate::{command::LapceUICommand, command::LAPCE_UI_COMMAND};

#[derive(Clone)]
pub struct FileExplorerData {
    pub tab_id: WidgetId,
    pub widget_id: WidgetId,
    pub workspace: Option<FileNodeItem>,
    pub active_selected: Option<PathBuf>,
}

impl FileExplorerData {
    pub fn new(
        tab_id: WidgetId,
        workspace: LapceWorkspace,
        proxy: Arc<LapceProxy>,
        event_sink: ExtEventSink,
    ) -> Self {
        let mut items = Vec::new();
        let widget_id = WidgetId::next();
        if let Some(path) = workspace.path.as_ref() {
            items.push(FileNodeItem {
                path_buf: path.clone(),
                is_dir: true,
                read: false,
                open: false,
                children: HashMap::new(),
                children_open_count: 0,
            });
            let path = path.clone();
            std::thread::spawn(move || {
                Self::read_dir(&path, true, tab_id, &proxy, event_sink);
            });
        }
        Self {
            tab_id,
            widget_id,
            workspace: workspace.path.as_ref().map(|p| FileNodeItem {
                path_buf: p.clone(),
                is_dir: true,
                read: false,
                open: false,
                children: HashMap::new(),
                children_open_count: 0,
            }),
            active_selected: None,
        }
    }

    pub fn update_node_count(&mut self, path: &Path) -> Option<()> {
        let node = self.get_node_mut(path)?;
        if node.is_dir {
            if node.open {
                node.children_open_count = node
                    .children
                    .iter()
                    .map(|(_, item)| item.children_open_count + 1)
                    .sum::<usize>();
            } else {
                node.children_open_count = 0;
            }
        }
        None
    }

    pub fn node_tree(&mut self, path: &Path) -> Option<Vec<PathBuf>> {
        let root = &self.workspace.as_ref()?.path_buf;
        let path = path.strip_prefix(root).ok()?;
        Some(
            path.ancestors()
                .map(|p| root.join(p))
                .collect::<Vec<PathBuf>>(),
        )
    }

    pub fn get_node_by_index(&mut self, index: usize) -> Option<&mut FileNodeItem> {
        let (_, node) = get_item_children_mut(0, index, self.workspace.as_mut()?);
        node
    }

    pub fn get_node_mut(&mut self, path: &Path) -> Option<&mut FileNodeItem> {
        let mut node = self.workspace.as_mut()?;
        if node.path_buf == path {
            return Some(node);
        }
        let root = node.path_buf.clone();
        let path = path.strip_prefix(&root).ok()?;
        for path in path.ancestors().collect::<Vec<&Path>>().iter().rev() {
            if path.to_str()?.is_empty() {
                continue;
            }
            node = node.children.get_mut(&root.join(path))?;
        }
        Some(node)
    }

    pub fn update_children(
        &mut self,
        path: &Path,
        children: HashMap<PathBuf, FileNodeItem>,
        expand: bool,
    ) -> Option<()> {
        let node = self.workspace.as_mut()?.get_file_node_mut(path)?;

        let removed_paths: Vec<PathBuf> = node
            .children
            .keys()
            .filter(|p| !children.contains_key(*p))
            .map(PathBuf::from)
            .collect();
        for path in removed_paths {
            node.children.remove(&path);
        }

        for (path, child) in children.into_iter() {
            if !node.children.contains_key(&path) {
                node.children.insert(child.path_buf.clone(), child);
            }
        }

        node.read = true;
        if expand {
            node.open = true;
        }

        for p in path.ancestors() {
            self.update_node_count(p);
        }

        Some(())
    }

    pub fn read_dir(
        path: &Path,
        expand: bool,
        tab_id: WidgetId,
        proxy: &LapceProxy,
        event_sink: ExtEventSink,
    ) {
        let path = PathBuf::from(path);
        let local_path = path.clone();
        proxy.read_dir(
            &local_path,
            Box::new(move |result| {
                if let Ok(res) = result {
                    let path = path.clone();
                    let resp: Result<ReadDirResponse, serde_json::Error> =
                        serde_json::from_value(res);
                    if let Ok(resp) = resp {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateExplorerItems(
                                path, resp.items, expand,
                            ),
                            Target::Widget(tab_id),
                        );
                    }
                }
            }),
        );
    }
}

pub fn get_item_children(
    i: usize,
    index: usize,
    item: &FileNodeItem,
) -> (usize, Option<&FileNodeItem>) {
    if i == index {
        return (i, Some(item));
    }
    let mut i = i;
    if item.open {
        for child in item.sorted_children() {
            let count = child.children_open_count;
            if i + count + 1 >= index {
                let (new_index, node) = get_item_children(i + 1, index, child);
                if new_index == index {
                    return (new_index, node);
                }
            }
            i += count + 1;
        }
    }
    (i, None)
}

pub fn get_item_children_mut(
    i: usize,
    index: usize,
    item: &mut FileNodeItem,
) -> (usize, Option<&mut FileNodeItem>) {
    if i == index {
        return (i, Some(item));
    }
    let mut i = i;
    if item.open {
        for child in item.sorted_children_mut() {
            let count = child.children_open_count;
            if i + count + 1 >= index {
                let (new_index, node) = get_item_children_mut(i + 1, index, child);
                if new_index == index {
                    return (new_index, node);
                }
            }
            i += count + 1;
        }
    }
    (i, None)
}
