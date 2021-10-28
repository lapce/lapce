use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use druid::Vec2;
use lsp_types::Position;
use serde::{Deserialize, Serialize};

use crate::{
    data::{EditorContent, EditorType, LapceTabData},
    movement::Cursor,
    state::LapceWorkspace,
};

pub struct LapceDb {
    db: sled::Db,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub editors: Vec<EditorInfo>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorInfo {
    pub content: EditorContent,
    pub scroll_offset: (f64, f64),
    pub position: Option<Position>,
}

impl LapceDb {
    pub fn new() -> Result<Self> {
        let proj_dirs = ProjectDirs::from("", "", "Lapce")
            .ok_or(anyhow!("can't find project dirs"))?;
        let path = proj_dirs.config_dir().join("lapce.db");
        let db = sled::Config::default()
            .path(path)
            .flush_every_ms(None)
            .open()?;
        Ok(Self { db })
    }

    pub fn save_recent_workspaces(
        &self,
        workspaces: Vec<LapceWorkspace>,
    ) -> Result<()> {
        let workspaces = serde_json::to_string(&workspaces)?;
        let key = "recent_workspaces";
        self.db.insert(key, workspaces.as_str())?;
        Ok(())
    }

    pub fn get_recent_workspaces(&self) -> Result<Vec<LapceWorkspace>> {
        let key = "recent_workspaces";
        let workspaces = self
            .db
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
        let workspace = workspace.to_string();
        let info = self
            .db
            .get(&workspace)?
            .ok_or(anyhow!("can't find workspace info"))?;
        let info = std::str::from_utf8(&info)?;
        let info: WorkspaceInfo = serde_json::from_str(info)?;
        Ok(info)
    }

    pub fn save_workspace(&self, data: &LapceTabData) -> Result<()> {
        let workspace = data.workspace.as_ref().ok_or(anyhow!("no workspace"))?;
        let editors = data
            .main_split
            .editors
            .iter()
            .filter_map(|(_, editor)| match &editor.editor_type {
                EditorType::Normal => Some(EditorInfo {
                    content: editor.content.clone(),
                    scroll_offset: (editor.scroll_offset.x, editor.scroll_offset.y),
                    position: if let EditorContent::Buffer(path) = &editor.content {
                        let buffer =
                            data.main_split.open_files.get(path).unwrap().clone();
                        Some(buffer.offset_to_position(editor.cursor.offset()))
                    } else {
                        None
                    },
                }),
                _ => None,
            })
            .collect();
        let workspace_info = WorkspaceInfo { editors };
        let workspace_info = serde_json::to_string(&workspace_info)?;
        let workspace = workspace.to_string();
        self.db
            .insert(workspace.as_str(), workspace_info.as_str())?;
        self.db.flush()?;
        Ok(())
    }
}
