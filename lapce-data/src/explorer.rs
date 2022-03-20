
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use druid::ExtEventSink;
use druid::{Target, WidgetId};

use include_dir::{include_dir, Dir};
use lapce_proxy::dispatch::FileNodeItem;

use crate::proxy::LapceProxy;
use crate::state::LapceWorkspace;

use crate::{command::LapceUICommand, command::LAPCE_UI_COMMAND};

#[allow(dead_code)]
const ICONS_DIR: Dir = include_dir!("../icons");

#[derive(Clone)]
pub struct FileExplorerData {
    pub tab_id: WidgetId,
    pub widget_id: WidgetId,
    pub workspace: Option<FileNodeItem>,
    pub index: usize,

    #[allow(dead_code)]
    count: usize,
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
            let index = 0;
            let path = path.clone();
            std::thread::spawn(move || {
                proxy.read_dir(
                    &path.clone(),
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            let resp: Result<Vec<FileNodeItem>, serde_json::Error> =
                                serde_json::from_value(res);
                            if let Ok(items) = resp {
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::UpdateExplorerItems(
                                        index,
                                        path.clone(),
                                        items,
                                    ),
                                    Target::Widget(tab_id),
                                );
                            }
                        }
                    }),
                );
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
            index: 0,
            count: 0,
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
