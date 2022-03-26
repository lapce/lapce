use bitflags::bitflags;
use druid::Modifiers;

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Write};
use std::path::PathBuf;
use std::sync::atomic;
use std::sync::atomic::AtomicU64;

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

#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy, Deserialize, Serialize)]
pub enum VisualMode {
    Normal,
    Linewise,
    Blockwise,
}

impl Default for VisualMode {
    fn default() -> Self {
        VisualMode::Normal
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy, PartialOrd, Ord)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    Terminal,
}

bitflags! {
    pub struct Modes: u32 {
        const NORMAL = 0x01;
        const INSERT = 0x2;
        const VISUAL = 0x4;
        const TERMINAL = 0x8;
    }
}

impl From<Mode> for Modes {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Normal => Self::NORMAL,
            Mode::Insert => Self::INSERT,
            Mode::Visual => Self::VISUAL,
            Mode::Terminal => Self::TERMINAL,
        }
    }
}

impl Modes {
    pub fn parse(modes_str: &str) -> Self {
        let mut this = Self::empty();

        for c in modes_str.chars() {
            match c {
                'i' | 'I' => this.set(Self::INSERT, true),
                'n' | 'N' => this.set(Self::NORMAL, true),
                'v' | 'V' => this.set(Self::VISUAL, true),
                't' | 'T' => this.set(Self::TERMINAL, true),
                _ => log::warn!("Not an editor mode: {c}"),
            }
        }

        this
    }
}

impl std::fmt::Display for Modes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bits = [
            (Self::INSERT, 'i'),
            (Self::NORMAL, 'n'),
            (Self::VISUAL, 'v'),
            (Self::TERMINAL, 't'),
        ];
        for (bit, chr) in bits {
            if self.contains(bit) {
                f.write_char(chr)?;
            }
        }

        Ok(())
    }
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

pub struct Counter(AtomicU64);

impl Counter {
    pub const fn new() -> Counter {
        Counter(AtomicU64::new(1))
    }

    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_check_condition() {}
}
