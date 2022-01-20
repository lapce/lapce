use std::path::PathBuf;

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use directories::ProjectDirs;
use druid::Vec2;
use lsp_types::Position;
use serde::{Deserialize, Serialize};

use crate::{
    buffer::BufferContent,
    data::{EditorContent, EditorType, LapceData, LapceTabData, LapceWindowData},
    movement::Cursor,
    state::LapceWorkspace,
};

pub enum SaveEvent {
    Workspace(LapceWorkspace, WorkspaceInfo),
    Tabs(TabsInfo),
}

#[derive(Clone)]
pub struct LapceDb {
    path: PathBuf,
    save_tx: Sender<SaveEvent>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub active_editor: usize,
    pub editors: Vec<EditorInfo>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TabsInfo {
    pub active_tab: usize,
    pub workspaces: Vec<Option<LapceWorkspace>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorInfo {
    pub content: BufferContent,
    pub scroll_offset: (f64, f64),
    pub position: Option<Position>,
}

impl LapceDb {
    pub fn new() -> Result<Self> {
        let proj_dirs = ProjectDirs::from("", "", "Lapce")
            .ok_or(anyhow!("can't find project dirs"))?;
        let path = proj_dirs.config_dir().join("lapce.db");
        let (save_tx, save_rx) = unbounded();

        let db = Self { path, save_tx };
        let local_db = db.clone();
        std::thread::spawn(move || -> Result<()> {
            loop {
                let event = save_rx.recv()?;
                match event {
                    SaveEvent::Workspace(workspace, info) => {
                        local_db.insert_workspace(&workspace, &info);
                    }
                    SaveEvent::Tabs(info) => {
                        local_db.insert_tabs(&info);
                    }
                }
            }
        });
        Ok(db)
    }

    pub fn get_db(&self) -> Result<sled::Db> {
        let db = sled::Config::default()
            .path(&self.path)
            .flush_every_ms(None)
            .open()?;
        Ok(db)
    }

    pub fn save_recent_workspaces(
        &self,
        workspaces: Vec<LapceWorkspace>,
    ) -> Result<()> {
        let db = self.get_db()?;
        let workspaces = serde_json::to_string(&workspaces)?;
        let key = "recent_workspaces";
        db.insert(key, workspaces.as_str())?;
        Ok(())
    }

    pub fn get_recent_workspaces(&self) -> Result<Vec<LapceWorkspace>> {
        let db = self.get_db()?;
        let key = "recent_workspaces";
        let workspaces = db
            .get(&key)?
            .ok_or(anyhow!("can't find recent workspaces"))?;
        let workspaces = std::str::from_utf8(&workspaces)?;
        let workspaces: Vec<LapceWorkspace> = serde_json::from_str(workspaces)?;
        Ok(workspaces)
    }

    pub fn get_workspace_info(
        &self,
        workspace: &LapceWorkspace,
    ) -> Result<WorkspaceInfo> {
        let db = self.get_db()?;
        let workspace = workspace.to_string();
        let info = db
            .get(&workspace)?
            .ok_or(anyhow!("can't find workspace info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: WorkspaceInfo = serde_json::from_str(info)?;
        Ok(info)
    }

    fn insert_tabs(&self, info: &TabsInfo) -> Result<()> {
        let tabs_info = serde_json::to_string(info)?;
        let db = self.get_db()?;
        db.insert(b"tabs", tabs_info.as_str())?;
        db.flush()?;
        Ok(())
    }

    fn insert_workspace(
        &self,
        workspace: &LapceWorkspace,
        info: &WorkspaceInfo,
    ) -> Result<()> {
        let workspace = workspace.to_string();
        let workspace_info = serde_json::to_string(info)?;
        let db = self.get_db()?;
        db.insert(workspace.as_str(), workspace_info.as_str())?;
        db.flush()?;
        Ok(())
    }

    pub fn workspace_info(
        &self,
        data: &LapceTabData,
    ) -> Result<(LapceWorkspace, WorkspaceInfo)> {
        let workspace = data.workspace.as_ref().ok_or(anyhow!("no workspace"))?;

        let mut active_editor = 0;
        let editors = data
            .main_split
            .editors_order
            .iter()
            .enumerate()
            .map(|(i, view_id)| {
                if *data.main_split.active == Some(*view_id) {
                    active_editor = i;
                }
                let editor = data.main_split.editors.get(view_id).unwrap();
                EditorInfo {
                    content: editor.content.clone(),
                    scroll_offset: (editor.scroll_offset.x, editor.scroll_offset.y),
                    position: if let BufferContent::File(path) = &editor.content {
                        let buffer =
                            data.main_split.open_files.get(path).unwrap().clone();
                        Some(buffer.offset_to_position(editor.cursor.offset()))
                    } else {
                        None
                    },
                }
            })
            .collect();
        let workspace_info = WorkspaceInfo {
            editors,
            active_editor,
        };
        Ok(((**workspace).clone(), workspace_info))
    }

    pub fn save_workspace(&self, data: &LapceTabData) -> Result<()> {
        let (workspace, workspace_info) = self.workspace_info(data)?;

        self.insert_workspace(&workspace, &workspace_info)?;
        Ok(())
    }

    pub fn save_workspace_async(&self, data: &LapceTabData) -> Result<()> {
        let (workspace, workspace_info) = self.workspace_info(data)?;

        self.save_tx
            .send(SaveEvent::Workspace(workspace, workspace_info))?;
        Ok(())
    }

    pub fn get_tabs_info(&self) -> Result<TabsInfo> {
        let db = self.get_db()?;
        let tabs = db.get(b"tabs")?.ok_or(anyhow!("can't find tabs info"))?;
        let tabs = std::str::from_utf8(&tabs)?;
        let tabs = serde_json::from_str(tabs)?;
        Ok(tabs)
    }

    pub fn save_tabs(&self, data: &LapceWindowData) -> Result<()> {
        let mut active_tab = 0;
        let workspaces: Vec<Option<LapceWorkspace>> = data
            .tabs_order
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let tab = data.tabs.get(w).unwrap();
                if tab.id == data.active_id {
                    active_tab = i;
                }
                tab.workspace.clone().map(|w| (*w).clone())
            })
            .collect();
        let info = TabsInfo {
            active_tab,
            workspaces,
        };
        self.save_tx.send(SaveEvent::Tabs(info))?;
        Ok(())
    }
}
