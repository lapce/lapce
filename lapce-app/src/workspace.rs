use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{debug::LapceBreakpoint, main_split::SplitInfo, panel::data::PanelInfo};

pub mod custom;
pub mod gh;
pub mod ssh;
pub mod ts;
#[cfg(windows)]
pub mod wsl;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LapceWorkspaceType {
    Local,
    RemoteCustom(custom::Host),
    RemoteGH(gh::Host),
    RemoteSSH(ssh::Host),
    RemoteTS(ts::Host),
    #[cfg(windows)]
    RemoteWSL(wsl::Host),
}

impl LapceWorkspaceType {
    pub fn is_local(&self) -> bool {
        matches!(self, LapceWorkspaceType::Local)
    }

    pub fn is_remote(&self) -> bool {
        use LapceWorkspaceType::*;

        #[cfg(not(windows))]
        return matches!(self, RemoteSSH(_) | RemoteGH(_) | RemoteTS(_));

        #[cfg(windows)]
        return matches!(
            self,
            RemoteSSH(_) | RemoteGH(_) | RemoteTS(_) | RemoteWSL(_)
        );
    }

    // pub fn display_name(&self) -> String {
    //     match self {
    //         Self::Local => String::new(),
    //         v => v..display_name(),
    //     }
    // }
}

impl std::fmt::Display for LapceWorkspaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LapceWorkspaceType::Local => f.write_str("Local"),
            LapceWorkspaceType::RemoteCustom(remote) => {
                write!(f, "{remote} (custom)")
            }
            LapceWorkspaceType::RemoteGH(remote) => {
                write!(f, "{remote} (GitHub Codespaces)")
            }
            LapceWorkspaceType::RemoteSSH(remote) => {
                write!(f, "ssh://{remote}")
            }
            LapceWorkspaceType::RemoteTS(remote) => {
                write!(f, "{remote} (Tailscale)")
            }
            #[cfg(windows)]
            LapceWorkspaceType::RemoteWSL(remote) => {
                write!(f, "{remote} (WSL)")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LapceWorkspace {
    pub kind: LapceWorkspaceType,
    pub path: Option<PathBuf>,
    pub last_open: u64,
}

impl LapceWorkspace {
    pub fn display(&self) -> Option<String> {
        let path = self.path.as_ref()?;
        let path = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .to_string();
        let remote = match &self.kind {
            LapceWorkspaceType::Local => String::new(),
            LapceWorkspaceType::RemoteCustom(remote) => {
                format!(" [Custom: {}]", remote)
            }
            LapceWorkspaceType::RemoteGH(remote) => {
                format!(" [GH: {}]", remote)
            }
            LapceWorkspaceType::RemoteSSH(remote) => {
                format!(" [SSH: {}]", remote.host)
            }
            LapceWorkspaceType::RemoteTS(remote) => {
                format!(" [TS: {}]", remote.host)
            }
            #[cfg(windows)]
            LapceWorkspaceType::RemoteWSL(remote) => {
                format!(" [WSL: {}]", remote.host)
            }
        };
        Some(format!("{path} {remote}"))
    }
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

impl std::fmt::Display for LapceWorkspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.kind,
            self.path
                .as_ref()
                .and_then(|p| p.to_str())
                .unwrap_or_default()
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub split: SplitInfo,
    pub panel: PanelInfo,
    pub breakpoints: HashMap<PathBuf, Vec<LapceBreakpoint>>,
}
