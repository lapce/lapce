use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use floem::{
    app::AppContext,
    glazier::KeyEvent,
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{
        create_effect, create_rw_signal, ReadSignal, RwSignal, SignalGetUntracked,
        SignalSet, SignalUpdate, SignalWith, SignalWithUntracked, WriteSignal,
    },
};
use lapce_core::{cursor::Cursor, register::Register};
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};
use serde::{Deserialize, Serialize};

use crate::{
    code_action::CodeActionData,
    command::InternalCommand,
    completion::CompletionData,
    config::LapceConfig,
    doc::Document,
    editor::{
        location::{EditorLocation, EditorPosition},
        EditorData,
    },
    editor_tab::{EditorTabChild, EditorTabData, EditorTabInfo},
    id::{EditorId, EditorTabId, SplitId},
    keypress::KeyPressData,
    window_tab::{CommonData, Focus, WindowTabData},
    workspace::WorkspaceInfo,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitContent {
    EditorTab(EditorTabId),
    Split(SplitId),
}

impl SplitContent {
    pub fn id(&self) -> u64 {
        match self {
            SplitContent::EditorTab(id) => id.to_raw(),
            SplitContent::Split(id) => id.to_raw(),
        }
    }

    pub fn content_info(&self, data: &WindowTabData) -> SplitContentInfo {
        match &self {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data = data
                    .main_split
                    .editor_tabs
                    .get_untracked()
                    .get(editor_tab_id)
                    .cloned()
                    .unwrap();
                SplitContentInfo::EditorTab(
                    editor_tab_data.get_untracked().tab_info(data),
                )
            }
            SplitContent::Split(split_id) => {
                let split_data = data
                    .main_split
                    .splits
                    .get_untracked()
                    .get(split_id)
                    .cloned()
                    .unwrap();
                SplitContentInfo::Split(split_data.get_untracked().split_info(data))
            }
        }
    }
}

#[derive(Clone)]
pub struct SplitData {
    pub parent_split: Option<SplitId>,
    pub split_id: SplitId,
    pub children: Vec<SplitContent>,
    pub direction: SplitDirection,
    pub window_origin: Point,
    pub layout_rect: Rect,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SplitInfo {
    pub children: Vec<SplitContentInfo>,
    pub direction: SplitDirection,
}

impl SplitInfo {
    pub fn to_data(
        &self,
        cx: AppContext,
        data: MainSplitData,
        parent_split: Option<SplitId>,
        split_id: SplitId,
    ) -> RwSignal<SplitData> {
        let split_data = SplitData {
            split_id,
            direction: self.direction,
            parent_split,
            children: self
                .children
                .iter()
                .map(|child| child.to_data(cx, data.clone(), split_id))
                .collect(),
            window_origin: Point::ZERO,
            layout_rect: Rect::ZERO,
        };
        let split_data = create_rw_signal(cx.scope, split_data);
        data.splits.update(|splits| {
            splits.insert(split_id, split_data);
        });
        split_data
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SplitContentInfo {
    EditorTab(EditorTabInfo),
    Split(SplitInfo),
}

impl SplitContentInfo {
    pub fn to_data(
        &self,
        cx: AppContext,
        data: MainSplitData,
        parent_split: SplitId,
    ) -> SplitContent {
        match &self {
            SplitContentInfo::EditorTab(tab_info) => {
                let tab_data = tab_info.to_data(cx, data, parent_split);
                SplitContent::EditorTab(
                    tab_data.with_untracked(|tab_data| tab_data.editor_tab_id),
                )
            }
            SplitContentInfo::Split(split_info) => {
                let split_id = SplitId::next();
                split_info.to_data(cx, data, Some(parent_split), split_id);
                SplitContent::Split(split_id)
            }
        }
    }
}

impl SplitData {
    pub fn split_info(&self, data: &WindowTabData) -> SplitInfo {
        let info = SplitInfo {
            direction: self.direction,
            children: self
                .children
                .iter()
                .map(|child| child.content_info(data))
                .collect(),
        };
        info
    }
}

#[derive(Clone)]
pub struct MainSplitData {
    pub root_split: SplitId,
    pub active_editor_tab: RwSignal<Option<EditorTabId>>,
    pub splits: RwSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    pub editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    pub editors: RwSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    pub docs: RwSignal<im::HashMap<PathBuf, RwSignal<Document>>>,
    locations: RwSignal<im::Vector<EditorLocation>>,
    current_location: RwSignal<usize>,
    pub common: CommonData,
}

impl MainSplitData {
    pub fn new(cx: AppContext, common: CommonData) -> Self {
        let splits = create_rw_signal(cx.scope, im::HashMap::new());
        let active_editor_tab = create_rw_signal(cx.scope, None);
        let editor_tabs = create_rw_signal(cx.scope, im::HashMap::new());
        let editors = create_rw_signal(cx.scope, im::HashMap::new());
        let docs = create_rw_signal(cx.scope, im::HashMap::new());
        let locations = create_rw_signal(cx.scope, im::Vector::new());
        let current_location = create_rw_signal(cx.scope, 0);

        Self {
            root_split: SplitId::next(),
            splits,
            active_editor_tab,
            editor_tabs,
            editors,
            docs,
            locations,
            current_location,
            common,
        }
    }

    pub fn key_down(
        &self,
        cx: AppContext,
        key_event: &KeyEvent,
        keypress: &mut KeyPressData,
    ) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
            editor_tabs.get(&active_editor_tab).copied()
        })?;
        let (_, child) = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;
        match child {
            EditorTabChild::Editor(editor_id) => {
                let editor = self
                    .editors
                    .with_untracked(|editors| editors.get(&editor_id).copied())?;
                let editor = editor.get_untracked();
                keypress.key_down(cx, key_event, &editor);
                editor.get_code_actions(cx);
            }
        }
        Some(())
    }

    fn save_current_jump_locatoin(&self) {
        if let Some(editor) = self.active_editor() {
            let (doc, cursor, viewport) = editor.with_untracked(|editor| {
                (editor.doc, editor.cursor, editor.viewport)
            });
            let path = doc.with_untracked(|doc| doc.content.path().cloned());
            if let Some(path) = path {
                let offset = cursor.with_untracked(|c| c.offset());
                let scroll_offset = viewport.get_untracked().origin().to_vec2();
                self.save_jump_location(path, offset, scroll_offset);
            }
        }
    }

    fn save_jump_location(&self, path: PathBuf, offset: usize, scroll_offset: Vec2) {
        let mut locations = self.locations.get_untracked();
        if let Some(last_location) = locations.last() {
            if last_location.path == path
                && last_location.position == Some(EditorPosition::Offset(offset))
                && last_location.scroll_offset == Some(scroll_offset)
            {
                return;
            }
        }
        let location = EditorLocation {
            path,
            position: Some(EditorPosition::Offset(offset)),
            scroll_offset: Some(scroll_offset),
        };
        locations.push_back(location);
        let current_location = locations.len();
        self.locations.set(locations);
        self.current_location.set(current_location);
    }

    pub fn jump_to_location(&self, cx: AppContext, location: EditorLocation) {
        self.save_current_jump_locatoin();
        self.go_to_location(cx, location);
    }

    pub fn get_doc(
        &self,
        cx: AppContext,
        path: PathBuf,
    ) -> (RwSignal<Document>, bool) {
        let doc = self.docs.with_untracked(|docs| docs.get(&path).cloned());
        if let Some(doc) = doc {
            (doc, false)
        } else {
            let doc = Document::new(
                cx,
                path.clone(),
                self.common.proxy.clone(),
                self.common.config,
            );
            let doc = create_rw_signal(cx.scope, doc);
            self.docs.update(|docs| {
                docs.insert(path.clone(), doc);
            });

            {
                let proxy = self.common.proxy.clone();
                create_effect(cx.scope, move |last| {
                    let rev = doc.with(|doc| doc.buffer().rev());
                    if last == Some(rev) {
                        return rev;
                    }
                    Document::tigger_proxy_update(cx, doc, &proxy);
                    rev
                });
            }

            (doc, true)
        }
    }

    pub fn go_to_location(&self, cx: AppContext, location: EditorLocation) {
        let path = location.path.clone();
        let (doc, new_doc) = self.get_doc(cx, path.clone());

        let editor = self.get_editor_or_new(cx, doc, &path);
        let editor = editor.get_untracked();
        editor.go_to_location(cx, location, new_doc);
    }

    fn get_editor_or_new(
        &self,
        cx: AppContext,
        doc: RwSignal<Document>,
        path: &Path,
    ) -> RwSignal<EditorData> {
        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        let editors = self.editors.get_untracked();
        let splits = self.splits.get_untracked();

        let active_editor_tab = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .cloned();

        // first check if the file exists in active editor tab or there's unconfirmed editor
        if let Some(editor_tab) = active_editor_tab {
            if let Some((index, editor)) = editor_tab.with_untracked(|editor_tab| {
                editor_tab
                    .get_editor(&editors, path)
                    .or_else(|| editor_tab.get_unconfirmed_editor(&editors))
            }) {
                if editor.with_untracked(|editor| {
                    editor.doc.with_untracked(|doc| doc.buffer_id)
                        != doc.with_untracked(|doc| doc.buffer_id)
                }) {
                    editor.update(|editor| {
                        editor.doc = doc;
                    });
                    editor.with_untracked(|editor| {
                        editor.cursor.set(Cursor::origin(
                            self.common.config.with_untracked(|c| c.core.modal),
                        ));
                    });
                }
                editor_tab.update(|editor_tab| {
                    editor_tab.active = index;
                });
                return editor;
            }
        }

        // check file exists in non active editor tabs
        for (editor_tab_id, editor_tab) in &editor_tabs {
            if Some(*editor_tab_id) != active_editor_tab_id {
                if let Some((index, editor)) =
                    editor_tab.with_untracked(|editor_tab| {
                        editor_tab.get_editor(&editors, path)
                    })
                {
                    self.active_editor_tab.set(Some(*editor_tab_id));
                    editor_tab.update(|editor_tab| {
                        editor_tab.active = index;
                    });
                    return editor;
                }
            }
        }

        let editor = if editor_tabs.is_empty() {
            // the main split doens't have anything
            let editor_tab_id = EditorTabId::next();

            // create the new editor
            let editor_id = EditorId::next();
            let editor = EditorData::new(
                cx,
                Some(editor_tab_id),
                editor_id,
                doc,
                self.common.clone(),
            );
            let editor = create_rw_signal(cx.scope, editor);
            self.editors.update(|editors| {
                editors.insert(editor_id, editor);
            });

            let editor_tab = EditorTabData {
                split: self.root_split,
                active: 0,
                editor_tab_id,
                children: vec![(
                    create_rw_signal(cx.scope, 0),
                    EditorTabChild::Editor(editor_id),
                )],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
            };
            let editor_tab = create_rw_signal(cx.scope, editor_tab);
            self.editor_tabs.update(|editor_tabs| {
                editor_tabs.insert(editor_tab_id, editor_tab);
            });
            let root_split = splits.get(&self.root_split).unwrap();
            root_split.update(|root_split| {
                root_split.children = vec![SplitContent::EditorTab(editor_tab_id)];
            });
            self.active_editor_tab.set(Some(editor_tab_id));
            editor
        } else {
            let editor_tab = if let Some(editor_tab) = active_editor_tab {
                editor_tab
            } else {
                let (editor_tab_id, editor_tab) = editor_tabs.iter().next().unwrap();
                self.active_editor_tab.set(Some(*editor_tab_id));
                *editor_tab
            };

            let editor_tab_id =
                editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

            // create the new editor
            let editor_id = EditorId::next();
            let editor = EditorData::new(
                cx,
                Some(editor_tab_id),
                editor_id,
                doc,
                self.common.clone(),
            );
            let editor = create_rw_signal(cx.scope, editor);
            self.editors.update(|editors| {
                editors.insert(editor_id, editor);
            });

            editor_tab.update(|editor_tab| {
                let active = editor_tab
                    .active
                    .min(editor_tab.children.len().saturating_sub(1));
                editor_tab.children.insert(
                    active + 1,
                    (
                        create_rw_signal(cx.scope, 0),
                        EditorTabChild::Editor(editor_id),
                    ),
                );
                editor_tab.active = active + 1;
            });
            editor
        };

        editor
    }

    pub fn jump_location_backward(&self, cx: AppContext) {
        let locations = self.locations.get_untracked();
        let current_location = self.current_location.get_untracked();
        if current_location < 1 {
            return;
        }

        if current_location >= locations.len() {
            self.save_current_jump_locatoin();
            self.current_location.update(|l| {
                *l -= 1;
            });
        }

        self.current_location.update(|l| {
            *l -= 1;
        });
        let current_location = self.current_location.get_untracked();
        let location = locations[current_location].clone();
        self.current_location.set(current_location);
        self.go_to_location(cx, location);
    }

    pub fn jump_location_forward(&self, cx: AppContext) {
        let locations = self.locations.get_untracked();
        let current_location = self.current_location.get_untracked();
        if locations.is_empty() {
            return;
        }
        if current_location >= locations.len() - 1 {
            return;
        }
        self.current_location.set(current_location + 1);
        let location = locations[current_location + 1].clone();
        self.go_to_location(cx, location);
    }

    pub fn split(
        &self,
        cx: AppContext,
        direction: SplitDirection,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        let split_direction = split.with_untracked(|split| split.direction);

        let (index, children_len) = split.with_untracked(|split| {
            split
                .children
                .iter()
                .position(|c| c == &SplitContent::EditorTab(editor_tab_id))
                .map(|index| (index, split.children.len()))
        })?;

        if split_direction == direction {
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(cx, split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
            split.update(|split| {
                split
                    .children
                    .insert(index + 1, SplitContent::EditorTab(new_editor_tab_id));
            });
        } else if children_len == 1 {
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(cx, split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
            split.update(|split| {
                split.direction = direction;
                split
                    .children
                    .push(SplitContent::EditorTab(new_editor_tab_id));
            });
        } else {
            let new_split_id = SplitId::next();

            editor_tab.update(|editor_tab| {
                editor_tab.split = new_split_id;
            });
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(cx, new_split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

            let new_split = SplitData {
                parent_split: Some(split_id),
                split_id: new_split_id,
                children: vec![
                    SplitContent::EditorTab(editor_tab_id),
                    SplitContent::EditorTab(new_editor_tab_id),
                ],
                direction,
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
            };
            let new_split = create_rw_signal(cx.scope, new_split);
            self.splits.update(|splits| {
                splits.insert(new_split_id, new_split);
            });
            split.update(|split| {
                split.children[index] = SplitContent::Split(new_split_id);
            });
        }

        Some(())
    }

    fn split_editor_tab(
        &self,
        cx: AppContext,
        split_id: SplitId,
        editor_tab: &EditorTabData,
    ) -> Option<RwSignal<EditorTabData>> {
        let (_, child) = editor_tab.children.get(editor_tab.active)?;

        let editor_tab_id = EditorTabId::next();

        let new_child = match child {
            EditorTabChild::Editor(editor_id) => {
                let new_editor_id = EditorId::next();
                let editor =
                    self.editors.get_untracked().get(editor_id)?.with_untracked(
                        |editor| editor.copy(cx, Some(editor_tab_id), new_editor_id),
                    );
                let editor = create_rw_signal(cx.scope, editor);
                self.editors.update(|editors| {
                    editors.insert(new_editor_id, editor);
                });
                EditorTabChild::Editor(new_editor_id)
            }
        };

        let editor_tab = EditorTabData {
            split: split_id,
            editor_tab_id,
            active: 0,
            children: vec![(create_rw_signal(cx.scope, 0), new_child)],
            window_origin: Point::ZERO,
            layout_rect: Rect::ZERO,
        };
        let editor_tab = create_rw_signal(cx.scope, editor_tab);
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab);
        });
        Some(editor_tab)
    }

    pub fn split_move(
        &self,
        cx: AppContext,
        direction: SplitMoveDirection,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let rect = editor_tab.with_untracked(|editor_tab| {
            editor_tab.layout_rect.with_origin(editor_tab.window_origin)
        });

        match direction {
            SplitMoveDirection::Up => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (current_rect.y1 - rect.y0).abs() < 3.0
                        && current_rect.x0 <= rect.x0
                        && rect.x0 < current_rect.x1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Down => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (current_rect.y0 - rect.y1).abs() < 3.0
                        && current_rect.x0 <= rect.x0
                        && rect.x0 < current_rect.x1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Right => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (rect.x1 - current_rect.x0).abs() < 3.0
                        && current_rect.y0 <= rect.y0
                        && rect.y0 < current_rect.y1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Left => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (current_rect.x1 - rect.x0).abs() < 3.0
                        && current_rect.y0 <= rect.y0
                        && rect.y0 < current_rect.y1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
        }

        Some(())
    }

    pub fn split_exchange(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;

        split.update(|split| {
            let index = split
                .children
                .iter()
                .position(|c| c == &SplitContent::EditorTab(editor_tab_id));
            if let Some(index) = index {
                if index < split.children.len() - 1 {
                    split.children.swap(index, index + 1);
                }
                self.split_content_focus(cx, &split.children[index]);
            }
        });

        Some(())
    }

    fn split_content_focus(&self, cx: AppContext, content: &SplitContent) {
        match content {
            SplitContent::EditorTab(editor_tab_id) => {
                self.active_editor_tab.set(Some(*editor_tab_id));
            }
            SplitContent::Split(split_id) => {
                self.split_focus(cx, *split_id);
            }
        }
    }

    fn split_focus(&self, cx: AppContext, split_id: SplitId) -> Option<()> {
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;

        let split_chilren = split.with_untracked(|split| split.children.clone());
        let content = split_chilren.get(0)?;
        self.split_content_focus(cx, content);

        Some(())
    }

    fn split_remove(&self, cx: AppContext, split_id: SplitId) -> Option<()> {
        if split_id == self.root_split {
            return Some(());
        }

        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        self.splits.update(|splits| {
            splits.remove(&split_id);
        });
        let parent_split_id = split.with_untracked(|split| split.parent_split)?;
        let parent_split = splits.get(&parent_split_id).copied()?;

        let split_len = parent_split
            .try_update(|split| {
                split
                    .children
                    .retain(|c| c != &SplitContent::Split(split_id));
                split.children.len()
            })
            .unwrap();
        if split_len == 0 {
            self.split_remove(cx, split_id);
        }

        Some(())
    }

    fn editor_tab_remove(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.remove(&editor_tab_id);
        });

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        let parent_split_id = split.with_untracked(|split| split.parent_split);

        let is_active = self
            .active_editor_tab
            .with_untracked(|active| active == &Some(editor_tab_id));

        let index = split.with_untracked(|split| {
            split
                .children
                .iter()
                .position(|c| c == &SplitContent::EditorTab(editor_tab_id))
        })?;
        split.update(|split| {
            split.children.remove(index);
        });
        let split_children = split.with_untracked(|split| split.children.clone());

        if is_active {
            if split_children.is_empty() {
                let new_focus = parent_split_id
                    .and_then(|split_id| splits.get(&split_id))
                    .and_then(|split| {
                        let index = split.with_untracked(|split| {
                            split
                                .children
                                .iter()
                                .position(|c| c == &SplitContent::Split(split_id))
                        })?;
                        Some((index, split))
                    })
                    .and_then(|(index, split)| {
                        split.with_untracked(|split| {
                            if index == split.children.len() - 1 {
                                if index > 0 {
                                    Some(split.children[index - 1])
                                } else {
                                    None
                                }
                            } else {
                                Some(split.children[index + 1])
                            }
                        })
                    });
                if let Some(content) = new_focus {
                    self.split_content_focus(cx, &content);
                }
            } else {
                let content = split_children[index.min(split_children.len() - 1)];
                self.split_content_focus(cx, &content);
            }
        }

        if split_children.is_empty() {
            self.split_remove(cx, split_id);
        }

        Some(())
    }

    pub fn editor_tab_close(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
        let editor_tab = editor_tab.get_untracked();
        for (_, child) in editor_tab.children {
            self.editor_tab_child_close(cx, editor_tab_id, child);
        }

        Some(())
    }

    pub fn editor_tab_child_close(
        &self,
        cx: AppContext,
        editor_tab_id: EditorTabId,
        child: EditorTabChild,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let index = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.iter().position(|(_, c)| c == &child)
        })?;

        let editor_tab_children_len = editor_tab
            .try_update(|editor_tab| {
                editor_tab.children.remove(index);
                editor_tab.active =
                    index.min(editor_tab.children.len().saturating_sub(1));
                editor_tab.children.len()
            })
            .unwrap();

        match child {
            EditorTabChild::Editor(editor_id) => {
                self.editors.update(|editors| {
                    editors.remove(&editor_id);
                });
            }
        }

        if editor_tab_children_len == 0 {
            self.editor_tab_remove(cx, editor_tab_id);
        }

        Some(())
    }

    pub fn editor_tab_update_layout(
        &self,
        editor_tab_id: &EditorTabId,
        window_origin: Point,
        rect: Rect,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(editor_tab_id).copied()?;
        editor_tab.update(|editor_tab| {
            editor_tab.window_origin = window_origin;
            editor_tab.layout_rect = rect;
        });
        Some(())
    }

    pub fn active_editor(&self) -> Option<RwSignal<EditorData>> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
            editor_tabs.get(&active_editor_tab).copied()
        })?;
        let (_, child) = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;

        let editor = match child {
            EditorTabChild::Editor(editor_id) => self
                .editors
                .with_untracked(|editors| editors.get(&editor_id).copied())?,
        };

        Some(editor)
    }
}
