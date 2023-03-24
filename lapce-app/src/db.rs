use std::sync::Arc;

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Sender};
use floem::reactive::SignalGetUntracked;
use lapce_core::directory::Directory;
use lapce_rpc::plugin::VoltID;

use crate::{
    window::{WindowData, WindowInfo},
    window_tab::WindowTabData,
    workspace::{LapceWorkspace, WorkspaceInfo},
};

pub enum SaveEvent {
    RecentWorkspace(LapceWorkspace),
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

    pub fn get_disabled_volts(&self) -> Result<Vec<VoltID>> {
        let sled_db = self.get_db()?;
        let volts = sled_db
            .get("disabled_volts")?
            .ok_or_else(|| anyhow!("can't find disable volts"))?;
        let volts = std::str::from_utf8(&volts)?;
        let volts: Vec<VoltID> = serde_json::from_str(volts)?;
        Ok(volts)
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
        for window_tab in data.window_tabs.get_untracked().into_iter() {
            let _ = self.save_window_tab(window_tab);
        }
        let info = data.info();
        let info = serde_json::to_string(&info)?;
        let sled_db = self.get_db()?;
        sled_db.insert("window", info.as_str())?;
        sled_db.flush()?;
        Ok(())
    }

    pub fn save_window_tab(&self, data: Arc<WindowTabData>) -> Result<()> {
        let workspace = (*data.workspace).clone();
        let workspace_info = data.workspace_info();

        self.insert_workspace(&workspace, &workspace_info)?;
        // self.insert_unsaved_buffer(main_split)?;

        Ok(())
    }
}
