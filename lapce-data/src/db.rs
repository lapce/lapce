use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Sender};
use druid::{ExtEventSink, Point, Rect, Size, Vec2, WidgetId};
use lapce_core::directory::Directory;
use lapce_rpc::plugin::VoltID;
use lapce_xi_rope::Rope;
use serde::{Deserialize, Serialize};

use crate::{
    config::LapceConfig,
    data::{
        EditorTabChild, LapceData, LapceEditorData, LapceEditorTabData,
        LapceMainSplitData, LapceTabData, LapceWindowData, LapceWorkspace,
        SplitContent, SplitData,
    },
    document::{BufferContent, Document, LocalBufferKind},
    editor::EditorLocation,
    panel::{PanelData, PanelOrder},
    split::SplitDirection,
};

pub enum SaveEvent {
    Workspace(LapceWorkspace, WorkspaceInfo),
    Tabs(TabsInfo),
    Buffer(BufferInfo),
    RecentWorkspace(LapceWorkspace),
}

#[derive(Clone)]
pub struct LapceDb {
    save_tx: Sender<SaveEvent>,
    sled_db: Option<sled::Db>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SplitContentInfo {
    EditorTab(EditorTabInfo),
    Split(SplitInfo),
}

impl SplitContentInfo {
    pub fn to_data(
        &self,
        data: &mut LapceMainSplitData,
        parent_split: Option<WidgetId>,
        editor_positions: &mut HashMap<PathBuf, Vec<(WidgetId, EditorLocation)>>,
        tab_id: WidgetId,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) -> SplitContent {
        match &self {
            SplitContentInfo::EditorTab(tab_info) => {
                let tab_data = tab_info.to_data(
                    data,
                    parent_split.unwrap(),
                    editor_positions,
                    tab_id,
                    config,
                    event_sink,
                );
                SplitContent::EditorTab(tab_data.widget_id)
            }
            SplitContentInfo::Split(split_info) => {
                let split_data = split_info.to_data(
                    data,
                    parent_split,
                    editor_positions,
                    tab_id,
                    config,
                    event_sink,
                );
                SplitContent::Split(split_data.widget_id)
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorTabInfo {
    pub active: usize,
    pub is_focus: bool,
    pub children: Vec<EditorTabChildInfo>,
}

impl EditorTabInfo {
    pub fn to_data(
        &self,
        data: &mut LapceMainSplitData,
        split: WidgetId,
        editor_positions: &mut HashMap<PathBuf, Vec<(WidgetId, EditorLocation)>>,
        tab_id: WidgetId,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) -> LapceEditorTabData {
        let editor_tab_id = WidgetId::next();
        let editor_tab_data = LapceEditorTabData {
            widget_id: editor_tab_id,
            split,
            active: self.active,
            children: self
                .children
                .iter()
                .map(|child| {
                    child.to_data(
                        data,
                        editor_tab_id,
                        editor_positions,
                        tab_id,
                        config,
                        event_sink.clone(),
                    )
                })
                .collect(),
            layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
            content_is_hot: Rc::new(RefCell::new(false)),
        };
        if self.is_focus {
            data.active = Arc::new(Some(
                editor_tab_data.children[editor_tab_data.active].widget_id(),
            ));
            data.active_tab = Arc::new(Some(editor_tab_data.widget_id));
        }
        data.editor_tabs
            .insert(editor_tab_id, Arc::new(editor_tab_data.clone()));
        editor_tab_data
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum EditorTabChildInfo {
    Editor(EditorInfo),
    Settings,
    Plugin { volt_id: VoltID, volt_name: String },
}

impl EditorTabChildInfo {
    pub fn to_data(
        &self,
        data: &mut LapceMainSplitData,
        editor_tab_id: WidgetId,
        editor_positions: &mut HashMap<PathBuf, Vec<(WidgetId, EditorLocation)>>,
        tab_id: WidgetId,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) -> EditorTabChild {
        match &self {
            EditorTabChildInfo::Editor(editor_info) => {
                let editor_data = editor_info.to_data(
                    data,
                    editor_tab_id,
                    editor_positions,
                    tab_id,
                    config,
                    event_sink,
                );
                EditorTabChild::Editor(
                    editor_data.view_id,
                    editor_data.editor_id,
                    editor_data.find_view_id,
                )
            }
            EditorTabChildInfo::Settings => {
                let editor = LapceEditorData::new(
                    None,
                    None,
                    None,
                    BufferContent::Local(LocalBufferKind::Keymap),
                    config,
                );
                let keymap_input_view_id = editor.view_id;
                data.editors.insert(editor.view_id, Arc::new(editor));

                EditorTabChild::Settings {
                    settings_widget_id: WidgetId::next(),
                    editor_tab_id,
                    keymap_input_view_id,
                }
            }
            EditorTabChildInfo::Plugin { volt_id, volt_name } => {
                EditorTabChild::Plugin {
                    widget_id: WidgetId::next(),
                    volt_id: volt_id.clone(),
                    volt_name: volt_name.to_string(),
                    editor_tab_id,
                }
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SplitInfo {
    pub children: Vec<SplitContentInfo>,
    pub direction: SplitDirection,
}

impl SplitInfo {
    pub fn to_data(
        &self,
        data: &mut LapceMainSplitData,
        parent_split: Option<WidgetId>,
        editor_positions: &mut HashMap<PathBuf, Vec<(WidgetId, EditorLocation)>>,
        tab_id: WidgetId,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) -> SplitData {
        let split_id = WidgetId::next();
        let split_data = SplitData {
            parent_split,
            direction: self.direction,
            widget_id: split_id,
            children: self
                .children
                .iter()
                .map(|child| {
                    child.to_data(
                        data,
                        Some(split_id),
                        editor_positions,
                        tab_id,
                        config,
                        event_sink.clone(),
                    )
                })
                .collect(),
            layout_rect: Rc::new(RefCell::new(Rect::ZERO)),
        };
        data.splits.insert(split_id, Arc::new(split_data.clone()));
        split_data
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub split: SplitInfo,
    pub panel: PanelData,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub size: Size,
    pub pos: Point,
    pub maximised: bool,
    pub tabs: TabsInfo,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TabsInfo {
    pub active_tab: usize,
    pub workspaces: Vec<LapceWorkspace>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BufferInfo {
    pub workspace: LapceWorkspace,
    pub path: PathBuf,
    pub scroll_offset: (f64, f64),
    pub cursor_offset: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorInfo {
    pub content: BufferContent,
    pub unsaved: Option<String>,
    pub scroll_offset: (f64, f64),
    pub position: Option<usize>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub windows: Vec<WindowInfo>,
}

impl EditorInfo {
    pub fn to_data(
        &self,
        data: &mut LapceMainSplitData,
        editor_tab_id: WidgetId,
        editor_positions: &mut HashMap<PathBuf, Vec<(WidgetId, EditorLocation)>>,
        tab_id: WidgetId,
        config: &LapceConfig,
        event_sink: ExtEventSink,
    ) -> LapceEditorData {
        let editor_data = LapceEditorData::new(
            None,
            None,
            Some(editor_tab_id),
            self.content.clone(),
            config,
        );
        if let BufferContent::File(path) = &self.content {
            if !editor_positions.contains_key(path) {
                editor_positions.insert(path.clone(), vec![]);
            }

            editor_positions.get_mut(path).unwrap().push((
                editor_data.view_id,
                EditorLocation {
                    path: path.clone(),
                    position: self.position,
                    scroll_offset: Some(Vec2::new(
                        self.scroll_offset.0,
                        self.scroll_offset.1,
                    )),
                    history: None,
                },
            ));

            if !data.open_docs.contains_key(path) {
                data.open_docs.insert(
                    path.clone(),
                    Arc::new(Document::new(
                        BufferContent::File(path.clone()),
                        tab_id,
                        event_sink,
                        data.proxy.clone(),
                    )),
                );
            }
        } else if let BufferContent::Scratch(id, _) = &self.content {
            if !data.scratch_docs.contains_key(id) {
                let mut doc = Document::new(
                    self.content.clone(),
                    tab_id,
                    event_sink,
                    data.proxy.clone(),
                );
                if let Some(text) = &self.unsaved {
                    doc.reload(Rope::from(text), false);
                }
                data.scratch_docs.insert(*id, Arc::new(doc));
            }
        }
        data.insert_editor(Arc::new(editor_data.clone()), config);
        editor_data
    }
}

impl LapceDb {
    pub fn new() -> Result<Self> {
        let path = Directory::config_directory()
            .ok_or_else(|| anyhow!("can't get config directory"))?
            .join("lapce.db");
        let (save_tx, save_rx) = unbounded();

        let sled_db = sled::Config::default()
            .path(path)
            .flush_every_ms(None)
            .open()
            .ok();

        let db = Self { save_tx, sled_db };
        let local_db = db.clone();
        std::thread::spawn(move || -> Result<()> {
            loop {
                let event = save_rx.recv()?;
                match event {
                    SaveEvent::Workspace(workspace, info) => {
                        let _ = local_db.insert_workspace(&workspace, &info);
                    }
                    SaveEvent::Tabs(info) => {
                        let _ = local_db.insert_tabs(&info);
                    }
                    SaveEvent::Buffer(info) => {
                        let _ = local_db.insert_buffer(&info);
                    }
                    SaveEvent::RecentWorkspace(workspace) => {
                        let _ = local_db.insert_recent_workspace(workspace);
                    }
                }
            }
        });
        Ok(db)
    }

    fn get_db(&self) -> Result<&sled::Db> {
        self.sled_db
            .as_ref()
            .ok_or_else(|| anyhow!("didn't open sled db"))
    }

    pub fn save_app(&self, data: &LapceData) -> Result<()> {
        for (_, window) in data.windows.iter() {
            for (_, tab) in window.tabs.iter() {
                let _ = self.save_workspace(tab);
            }
        }
        let info = AppInfo {
            windows: data
                .windows
                .iter()
                .map(|(_, window_data)| window_data.info())
                .collect(),
        };
        let info = serde_json::to_string(&info)?;
        let sled_db = self.get_db()?;
        sled_db.insert("app", info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn get_app(&self) -> Result<AppInfo> {
        let sled_db = self.get_db()?;
        let info = sled_db
            .get("app")?
            .ok_or_else(|| anyhow!("can't find app info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: AppInfo = serde_json::from_str(info)?;
        Ok(info)
    }

    pub fn save_recent_workspaces(
        &self,
        workspaces: Vec<LapceWorkspace>,
    ) -> Result<()> {
        let workspaces = serde_json::to_string(&workspaces)?;
        let key = "recent_workspaces";
        let sled_db = self.get_db()?;
        sled_db.insert(key, workspaces.as_str())?;
        Ok(())
    }

    pub fn get_recent_workspaces(&self) -> Result<Vec<LapceWorkspace>> {
        let key = "recent_workspaces";
        let sled_db = self.get_db()?;
        let workspaces = sled_db
            .get(key)?
            .ok_or_else(|| anyhow!("can't find recent workspaces"))?;
        let workspaces = std::str::from_utf8(&workspaces)?;
        let workspaces: Vec<LapceWorkspace> = serde_json::from_str(workspaces)?;
        Ok(workspaces)
    }

    pub fn get_workspace_info(
        &self,
        workspace: &LapceWorkspace,
    ) -> Result<WorkspaceInfo> {
        let workspace = workspace.to_string();
        let sled_db = self.get_db()?;
        let info = sled_db
            .get(workspace)?
            .ok_or_else(|| anyhow!("can't find workspace info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: WorkspaceInfo = serde_json::from_str(info)?;
        Ok(info)
    }

    /// fetches all the files stored in the database that are tagged as `unsaved_buffer`.<br>
    /// Returns a hashmap of the form HashMap<path, file_content><br>
    /// *Note: The file is deleted from the database right after as to prevent "ghost" buffers*
    pub fn get_unsaved_buffers(&self) -> Result<im::HashMap<String, String>> {
        let sled_db = self.get_db()?;
        let mut buffers = im::HashMap::new();

        let res = match sled_db.get("unsaved_buffers") {
            Ok(val) => match val {
                Some(buffers) => buffers,
                None => return Ok(buffers),
            },
            Err(err) => return Err(anyhow!(err)),
        };

        let res = String::from_utf8(res.to_vec())
            .expect("invalid utf-8 sequence retrieving unsaved buffer");

        let res: Vec<String> = serde_json::from_str(&res)?;
        if res.len() % 2 != 0 {
            return Err(anyhow!("Deserialized Unsaved buffer size is not even: this should never happen"));
        }

        for i in (0..res.len()).step_by(2) {
            let key = res.get(i).unwrap().clone();
            let value = res.get(i + 1).unwrap().clone();
            buffers.insert(key, value);
        }
        sled_db.remove("unsaved_buffers")?;
        Ok(buffers)
    }

    pub fn get_buffer_info(
        &self,
        workspace: &LapceWorkspace,
        path: &Path,
    ) -> Result<BufferInfo> {
        let key = format!("{}:{}", workspace, path.to_str().unwrap_or(""));
        let sled_db = self.get_db()?;
        let info = sled_db
            .get(key.as_str())?
            .ok_or_else(|| anyhow!("can't find workspace info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: BufferInfo = serde_json::from_str(info)?;
        Ok(info)
    }

    fn insert_buffer(&self, info: &BufferInfo) -> Result<()> {
        let key = format!("{}:{}", info.workspace, info.path.to_str().unwrap_or(""));
        let info = serde_json::to_string(info)?;
        let sled_db = self.get_db()?;
        sled_db.insert(key.as_str(), info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    fn insert_tabs(&self, info: &TabsInfo) -> Result<()> {
        let tabs_info = serde_json::to_string(info)?;
        let sled_db = self.get_db()?;
        sled_db.insert(b"tabs", tabs_info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn save_disabled_volts(&self, volts: Vec<&VoltID>) -> Result<()> {
        let sled_db = self.get_db()?;
        let volts = serde_json::to_string(&volts)?;
        sled_db.insert(b"disabled_volts", volts.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn get_disabled_volts(&self) -> Result<Vec<VoltID>> {
        let sled_db = self.get_db()?;
        let volts = sled_db
            .get("disabled_volts")?
            .ok_or_else(|| anyhow!("can't find disable volts"))?;
        let volts = std::str::from_utf8(&volts)?;
        let volts: Vec<VoltID> = serde_json::from_str(volts)?;
        Ok(volts)
    }

    pub fn save_workspace_disabled_volts(
        &self,
        workspace: &LapceWorkspace,
        volts: Vec<&VoltID>,
    ) -> Result<()> {
        let sled_db = self.get_db()?;
        let volts = serde_json::to_string(&volts)?;
        sled_db.insert(format!("disabled_volts:{workspace}"), volts.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn get_workspace_disabled_volts(
        &self,
        workspace: &LapceWorkspace,
    ) -> Result<Vec<VoltID>> {
        let sled_db = self.get_db()?;
        let volts = sled_db
            .get(format!("disabled_volts:{workspace}"))?
            .ok_or_else(|| anyhow!("can't find disable volts"))?;
        let volts = std::str::from_utf8(&volts)?;
        let volts: Vec<VoltID> = serde_json::from_str(volts)?;
        Ok(volts)
    }

    pub fn save_last_window(&self, window: &LapceWindowData) {
        let info = window.info();
        let _ = self.insert_last_window_info(info);
    }

    fn insert_last_window_info(&self, info: WindowInfo) -> Result<()> {
        let info = serde_json::to_string(&info)?;
        let sled_db = self.get_db()?;
        sled_db.insert("last_window", info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn get_last_window_info(&self) -> Result<WindowInfo> {
        let sled_db = self.get_db()?;
        let info = sled_db
            .get("last_window")?
            .ok_or_else(|| anyhow!("can't find last window info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: WindowInfo = serde_json::from_str(info)?;
        Ok(info)
    }

    pub fn get_panel_orders(&self) -> Result<PanelOrder> {
        let sled_db = self.get_db()?;
        let panel_orders = sled_db
            .get("panel_orders")?
            .ok_or_else(|| anyhow!("can't find panel orders"))?;
        let panel_orders = std::str::from_utf8(&panel_orders)?;
        let panel_orders: PanelOrder = serde_json::from_str(panel_orders)?;
        Ok(panel_orders)
    }

    pub fn save_panel_orders(&self, order: &PanelOrder) -> Result<()> {
        let info = serde_json::to_string(order)?;
        let sled_db = self.get_db()?;
        sled_db.insert("panel_orders", info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    fn insert_workspace(
        &self,
        workspace: &LapceWorkspace,
        info: &WorkspaceInfo,
    ) -> Result<()> {
        let workspace = workspace.to_string();
        let workspace_info = serde_json::to_string(info)?;
        let sled_db = self.get_db()?;
        sled_db.insert(workspace.as_str(), workspace_info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn save_workspace(&self, data: &LapceTabData) -> Result<()> {
        let workspace = (*data.workspace).clone();
        let workspace_info = data.workspace_info();

        // Buffer for auto save on quit
        let main_split = &data.main_split;

        self.insert_workspace(&workspace, &workspace_info)?;
        self.insert_unsaved_buffer(main_split)?;

        Ok(())
    }

    fn insert_unsaved_buffer(&self, main_split: &LapceMainSplitData) -> Result<()> {
        let sled_db = self.get_db()?;
        // Vec of all unsaved buffers of format path_buff, file_content
        let mut unsaved_buffers = Vec::new();

        for (path, doc) in &main_split.open_docs {
            if !doc.buffer().is_pristine() && doc.content().is_file() {
                let path_str = path.to_str().unwrap();
                let buf_text = doc.buffer().to_string();
                unsaved_buffers.push(path_str.to_string());
                unsaved_buffers.push(buf_text);
            }
        }
        if !unsaved_buffers.is_empty() {
            let tmp = serde_json::to_string(&unsaved_buffers).unwrap();
            sled_db.insert("unsaved_buffers", tmp.as_str())?;
            sled_db.flush()?;
        }

        Ok(())
    }

    pub fn save_workspace_async(&self, data: &LapceTabData) -> Result<()> {
        let workspace = (*data.workspace).clone();
        let workspace_info = data.workspace_info();

        self.save_tx
            .send(SaveEvent::Workspace(workspace, workspace_info))?;
        Ok(())
    }

    pub fn save_doc_position(&self, workspace: &LapceWorkspace, doc: &Document) {
        if let BufferContent::File(path) = doc.content() {
            let info = BufferInfo {
                workspace: workspace.clone(),
                path: path.clone(),
                scroll_offset: (doc.scroll_offset.x, doc.scroll_offset.y),
                cursor_offset: doc.cursor_offset,
            };
            let _ = self.save_tx.send(SaveEvent::Buffer(info));
        }
    }

    pub fn get_tabs_info(&self) -> Result<TabsInfo> {
        let sled_db = self.get_db()?;
        let tabs = sled_db
            .get(b"tabs")?
            .ok_or_else(|| anyhow!("can't find tabs info"))?;
        let tabs = std::str::from_utf8(&tabs)?;
        let tabs = serde_json::from_str(tabs)?;
        Ok(tabs)
    }

    pub fn save_tabs_async(&self, data: &LapceWindowData) -> Result<()> {
        let mut active_tab = 0;
        let workspaces: Vec<LapceWorkspace> = data
            .tabs_order
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let tab = data.tabs.get(w).unwrap();
                if tab.id == *data.active_id {
                    active_tab = i;
                }
                (*tab.workspace).clone()
            })
            .collect();
        let info = TabsInfo {
            active_tab,
            workspaces,
        };
        self.save_tx.send(SaveEvent::Tabs(info))?;
        Ok(())
    }

    pub fn recent_workspaces(&self) -> Result<Vec<LapceWorkspace>> {
        let sled_db = self.get_db()?;
        let workspaces = sled_db
            .get("recent_workspaces")?
            .ok_or_else(|| anyhow!("can't find disable volts"))?;
        let workspaces = std::str::from_utf8(&workspaces)?;
        let workspaces: Vec<LapceWorkspace> = serde_json::from_str(workspaces)?;
        Ok(workspaces)
    }

    pub fn update_recent_workspace(&self, workspace: LapceWorkspace) -> Result<()> {
        if workspace.path.is_none() {
            return Ok(());
        }
        self.save_tx.send(SaveEvent::RecentWorkspace(workspace))?;
        Ok(())
    }

    fn insert_recent_workspace(&self, workspace: LapceWorkspace) -> Result<()> {
        let sled_db = self.get_db()?;

        let mut workspaces = self.recent_workspaces().unwrap_or_default();

        let mut exits = false;
        for w in workspaces.iter_mut() {
            if w.path == workspace.path && w.kind == workspace.kind {
                w.last_open = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                exits = true;
                break;
            }
        }
        if !exits {
            let mut workspace = workspace;
            workspace.last_open = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            workspaces.push(workspace);
        }
        workspaces.sort_by_key(|w| -(w.last_open as i64));
        let workspaces = serde_json::to_string(&workspaces)?;

        sled_db.insert("recent_workspaces", workspaces.as_str())?;
        sled_db.flush()?;

        Ok(())
    }
}
