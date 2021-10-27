use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use druid::Vec2;
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
    pub cursor: Cursor,
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
                    cursor: editor.cursor.clone(),
                }),
                _ => None,
            })
            .collect();
        let workspace_info = WorkspaceInfo { editors };
        let workspace_info = serde_json::to_string(&workspace_info)?;
        let workspace = workspace.to_string();
        self.db
            .insert(workspace.as_bytes(), workspace_info.as_bytes())?;
        self.db.flush()?;
        println!("db save workspace done {} {}", workspace, workspace_info);
        Ok(())
    }
}
