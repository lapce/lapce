use druid::Modifiers;

use lapce_core::mode::Modes;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::PathBuf;

#[derive(PartialEq)]
enum KeymapMatch {
    #[allow(dead_code)]
    Full,
    #[allow(dead_code)]
    Prefix,
}

#[derive(Clone, PartialEq)]
pub enum LapceFocus {
    Palette,
    Editor,
    FileExplorer,
    SourceControl,
}

#[derive(PartialEq, Eq, Hash, Default, Clone)]
pub struct KeyPress {
    pub key: druid::keyboard_types::Key,
    pub mods: Modifiers,
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Modes,
    pub when: Option<String>,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LapceWorkspaceType {
    Local,
    RemoteSSH(String, String),
    RemoteWSL,
}

impl LapceWorkspaceType {
    pub fn is_remote(&self) -> bool {
        matches!(
            self,
            LapceWorkspaceType::RemoteSSH(_, _) | LapceWorkspaceType::RemoteWSL
        )
    }
}

impl Display for LapceWorkspaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LapceWorkspaceType::Local => f.write_str("Local"),
            LapceWorkspaceType::RemoteSSH(user, host) => {
                write!(f, "ssh://{}@{}", user, host)
            }
            LapceWorkspaceType::RemoteWSL => f.write_str("WSL"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LapceWorkspace {
    pub kind: LapceWorkspaceType,
    pub path: Option<PathBuf>,
    pub last_open: u64,
}

impl Default for LapceWorkspace {
    fn default() -> Self {
        Self {
            kind: LapceWorkspaceType::Local,
            path: None,
            last_open: 0,
        }
    }
}

impl Display for LapceWorkspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.kind,
            self.path
                .as_ref()
                .and_then(|p| p.to_str())
                .map(|p| p.to_string())
                .unwrap_or_else(|| "".to_string())
        )
    }
}
