use std::{
    fmt::{Display, Formatter, Result},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct SshHost {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<usize>,
}

impl SshHost {
    pub fn from_string(s: &str) -> Self {
        let mut whole_splits = s.split(':');
        let splits = whole_splits
            .next()
            .unwrap()
            .split('@')
            .collect::<Vec<&str>>();
        let mut splits = splits.iter().rev();
        let host = splits.next().unwrap().to_string();
        let user = splits.next().map(|s| s.to_string());
        let port = whole_splits.next().and_then(|s| s.parse::<usize>().ok());
        Self { user, host, port }
    }

    pub fn user_host(&self) -> String {
        if let Some(user) = self.user.as_ref() {
            format!("{user}@{}", self.host)
        } else {
            self.host.clone()
        }
    }
}

impl Display for SshHost {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        if let Some(user) = self.user.as_ref() {
            write!(f, "{user}@")?;
        }
        write!(f, "{}", self.host)?;
        if let Some(port) = self.port {
            write!(f, ":{}", port)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LapceWorkspaceType {
    Local,
    RemoteSSH(SshHost),
    RemoteWSL,
}

impl LapceWorkspaceType {
    pub fn is_remote(&self) -> bool {
        matches!(
            self,
            LapceWorkspaceType::RemoteSSH(_) | LapceWorkspaceType::RemoteWSL
        )
    }
}

impl Display for LapceWorkspaceType {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            LapceWorkspaceType::Local => f.write_str("Local"),
            LapceWorkspaceType::RemoteSSH(ssh) => {
                write!(f, "ssh://{ssh}")
            }
            LapceWorkspaceType::RemoteWSL => f.write_str("WSL"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
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
