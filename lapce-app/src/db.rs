use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Sender};
use floem::peniko::kurbo::Vec2;
use lapce_core::directory::Directory;
use lapce_rpc::plugin::VoltID;

use crate::{
    app::{AppData, AppInfo},
    doc::DocInfo,
    panel::{data::PanelOrder, kind::PanelKind, position::PanelPosition},
    window::{WindowData, WindowInfo},
    window_tab::WindowTabData,
    workspace::{LapceWorkspace, WorkspaceInfo},
};

pub enum SaveEvent {
    App(AppInfo),
    Workspace(LapceWorkspace, WorkspaceInfo),
    RecentWorkspace(LapceWorkspace),
    Doc(DocInfo),
    DisabledVolts(Vec<VoltID>),
    WorkspaceDisabledVolts(Arc<LapceWorkspace>, Vec<VoltID>),
    PanelOrder(PanelOrder),
}

#[derive(Clone)]
pub struct LapceDb {
    save_tx: Sender<SaveEvent>,
    sled_db: Option<sled::Db>,
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
                    SaveEvent::App(info) => {
                        let _ = local_db.insert_app_info(info);
                    }
                    SaveEvent::Workspace(workspace, info) => {
                        let _ = local_db.insert_workspace(&workspace, &info);
                    }
                    SaveEvent::RecentWorkspace(workspace) => {
                        let _ = local_db.insert_recent_workspace(workspace);
                    }
                    SaveEvent::Doc(info) => {
                        let _ = local_db.insert_doc(&info);
                    }
                    SaveEvent::DisabledVolts(volts) => {
                        let _ = local_db.insert_disabled_volts(volts);
                    }
                    SaveEvent::WorkspaceDisabledVolts(workspace, volts) => {
                        let _ = local_db
                            .insert_workspace_disabled_volts(workspace, volts);
                    }
                    SaveEvent::PanelOrder(order) => {
                        let _ = local_db.insert_panel_orders(&order);
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

    pub fn get_disabled_volts(&self) -> Result<Vec<VoltID>> {
        let sled_db = self.get_db()?;
        let volts = sled_db
            .get("disabled_volts")?
            .ok_or_else(|| anyhow!("can't find disable volts"))?;
        let volts = std::str::from_utf8(&volts)?;
        let volts: Vec<VoltID> = serde_json::from_str(volts)?;
        Ok(volts)
    }

    pub fn save_disabled_volts(&self, volts: Vec<VoltID>) {
        let _ = self.save_tx.send(SaveEvent::DisabledVolts(volts));
    }

    pub fn save_workspace_disabled_volts(
        &self,
        workspace: Arc<LapceWorkspace>,
        volts: Vec<VoltID>,
    ) {
        let _ = self
            .save_tx
            .send(SaveEvent::WorkspaceDisabledVolts(workspace, volts));
    }

    pub fn insert_disabled_volts(&self, volts: Vec<VoltID>) -> Result<()> {
        let sled_db = self.get_db()?;
        let volts = serde_json::to_string(&volts)?;
        sled_db.insert(b"disabled_volts", volts.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn insert_workspace_disabled_volts(
        &self,
        workspace: Arc<LapceWorkspace>,
        volts: Vec<VoltID>,
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

    pub fn recent_workspaces(&self) -> Result<Vec<LapceWorkspace>> {
        let sled_db = self.get_db()?;
        let workspaces = sled_db
            .get("recent_workspaces")?
            .ok_or_else(|| anyhow!("can't find disable volts"))?;
        let workspaces = std::str::from_utf8(&workspaces)?;
        let workspaces: Vec<LapceWorkspace> = serde_json::from_str(workspaces)?;
        Ok(workspaces)
    }

    pub fn update_recent_workspace(&self, workspace: &LapceWorkspace) -> Result<()> {
        if workspace.path.is_none() {
            return Ok(());
        }
        self.save_tx
            .send(SaveEvent::RecentWorkspace(workspace.clone()))?;
        Ok(())
    }

    fn insert_doc(&self, info: &DocInfo) -> Result<()> {
        let key = format!("{}:{}", info.workspace, info.path.to_str().unwrap_or(""));
        let info = serde_json::to_string(info)?;
        let sled_db = self.get_db()?;
        sled_db.insert(key.as_str(), info.as_str())?;
        sled_db.flush()?;
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

    pub fn save_window_tab(&self, data: Rc<WindowTabData>) -> Result<()> {
        let workspace = (*data.workspace).clone();
        let workspace_info = data.workspace_info();

        self.save_tx
            .send(SaveEvent::Workspace(workspace, workspace_info))?;
        // self.insert_unsaved_buffer(main_split)?;

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

    pub fn save_app(&self, data: &AppData) -> Result<()> {
        let windows = data.windows.get_untracked();
        for (_, window) in &windows {
            let _ = self.save_window(window.clone());
        }

        let info = AppInfo {
            windows: windows
                .iter()
                .map(|(_, window_data)| window_data.info())
                .collect(),
        };
        if info.windows.is_empty() {
            return Ok(());
        }

        self.save_tx.send(SaveEvent::App(info))?;

        Ok(())
    }

    pub fn insert_app_info(&self, info: AppInfo) -> Result<()> {
        let info = serde_json::to_string(&info)?;
        let sled_db = self.get_db()?;
        sled_db.insert("app", info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn insert_app(&self, data: AppData) -> Result<()> {
        let windows = data.windows.get_untracked();
        if windows.is_empty() {
            // insert_app is called after window is closed, so we don't want to store it
            return Ok(());
        }
        for (_, window) in &windows {
            let _ = self.insert_window(window.clone());
        }
        let info = AppInfo {
            windows: windows
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

    pub fn get_window(&self) -> Result<WindowInfo> {
        let sled_db = self.get_db()?;
        let info = sled_db
            .get("window")?
            .ok_or_else(|| anyhow!("can't find app info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: WindowInfo = serde_json::from_str(info)?;
        Ok(info)
    }

    pub fn save_window(&self, data: WindowData) -> Result<()> {
        for (_, window_tab) in data.window_tabs.get_untracked().into_iter() {
            let _ = self.save_window_tab(window_tab);
        }
        Ok(())
    }

    pub fn insert_window(&self, data: WindowData) -> Result<()> {
        for (_, window_tab) in data.window_tabs.get_untracked().into_iter() {
            let _ = self.insert_window_tab(window_tab);
        }
        let info = data.info();
        let info = serde_json::to_string(&info)?;
        let sled_db = self.get_db()?;
        sled_db.insert("window", info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn insert_window_tab(&self, data: Rc<WindowTabData>) -> Result<()> {
        let workspace = (*data.workspace).clone();
        let workspace_info = data.workspace_info();

        self.insert_workspace(&workspace, &workspace_info)?;
        // self.insert_unsaved_buffer(main_split)?;

        Ok(())
    }

    pub fn get_panel_orders(&self) -> Result<PanelOrder> {
        let sled_db = self.get_db()?;
        let panel_orders = sled_db
            .get("panel_orders")?
            .ok_or_else(|| anyhow!("can't find panel orders"))?;
        let panel_orders = std::str::from_utf8(&panel_orders)?;
        let mut panel_orders: PanelOrder = serde_json::from_str(panel_orders)?;

        use strum::IntoEnumIterator;
        for kind in PanelKind::iter() {
            if kind.position(&panel_orders).is_none() {
                let panels = panel_orders.entry(PanelPosition::LeftTop).or_default();
                panels.push_back(kind);
            }
        }

        Ok(panel_orders)
    }

    pub fn save_panel_orders(&self, order: PanelOrder) {
        let _ = self.save_tx.send(SaveEvent::PanelOrder(order));
    }

    fn insert_panel_orders(&self, order: &PanelOrder) -> Result<()> {
        let info = serde_json::to_string(order)?;
        let sled_db = self.get_db()?;
        sled_db.insert("panel_orders", info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn save_doc_position(
        &self,
        workspace: &LapceWorkspace,
        path: PathBuf,
        cursor_offset: usize,
        scroll_offset: Vec2,
    ) {
        let info = DocInfo {
            workspace: workspace.clone(),
            path,
            scroll_offset: (scroll_offset.x, scroll_offset.y),
            cursor_offset,
        };
        let _ = self.save_tx.send(SaveEvent::Doc(info));
    }

    pub fn get_doc_info(
        &self,
        workspace: &LapceWorkspace,
        path: &Path,
    ) -> Result<DocInfo> {
        let key = format!("{}:{}", workspace, path.to_str().unwrap_or(""));
        let sled_db = self.get_db()?;
        let info = sled_db
            .get(key.as_str())?
            .ok_or_else(|| anyhow!("can't find workspace info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: DocInfo = serde_json::from_str(info)?;
        Ok(info)
    }
}
