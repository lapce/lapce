use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use druid::{
    widget::SvgData, Color, Data, Env, EventCtx, ExtEventSink, KeyEvent, Lens,
    Modifiers, Target, WidgetId, WindowId,
};
use git2::Oid;
use git2::Repository;
use im;
use lapce_proxy::dispatch::NewBufferResponse;
use lazy_static::lazy_static;
use lsp_types::Position;
use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::Value;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::{
    collections::HashMap, fs::File, io::Read, path::PathBuf, str::FromStr,
    sync::Arc, thread,
};
use std::{io::BufReader, sync::atomic::AtomicU64};
use std::{path::Path, sync::atomic};
use toml;
use xi_rpc::Handler;
use xi_rpc::RpcLoop;
use xi_rpc::RpcPeer;
use xi_trace::enable_tracing;

#[derive(PartialEq)]
enum KeymapMatch {
    Full,
    Prefix,
}

#[derive(Clone, PartialEq)]
pub enum LapceFocus {
    Palette,
    Editor,
    FileExplorer,
    SourceControl,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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

#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy)]
pub enum Mode {
    Insert,
    Visual,
    Normal,
}

#[derive(PartialEq, Eq, Hash, Default, Clone)]
pub struct KeyPress {
    pub key: druid::keyboard_types::Key,
    pub mods: Modifiers,
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Vec<Mode>,
    pub when: Option<String>,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LapceWorkspaceType {
    Local,
    RemoteSSH(String, String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LapceWorkspace {
    pub kind: LapceWorkspaceType,
    pub path: PathBuf,
}

impl Default for LapceWorkspace {
    fn default() -> Self {
        Self {
            kind: LapceWorkspaceType::Local,
            path: directories::UserDirs::new()
                .unwrap()
                .home_dir()
                .to_path_buf(),
        }
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

pub fn hex_to_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches("#");
    let (r, g, b, a) = match hex.len() {
        3 => (
            format!("{}{}", &hex[0..0], &hex[0..0]),
            format!("{}{}", &hex[1..1], &hex[1..1]),
            format!("{}{}", &hex[2..2], &hex[2..2]),
            "ff".to_string(),
        ),
        6 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            "ff".to_string(),
        ),
        8 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            hex[6..8].to_string(),
        ),
        _ => return Err(anyhow!("invalid hex color")),
    };
    Ok(Color::rgba8(
        u8::from_str_radix(&r, 16)?,
        u8::from_str_radix(&g, 16)?,
        u8::from_str_radix(&b, 16)?,
        u8::from_str_radix(&a, 16)?,
    ))
}

#[cfg(test)]
mod tests {
    use xi_rope::Rope;

    use super::*;

    #[test]
    fn test_check_condition() {
        // let rope = Rope::from_str("abc\nabc\n").unwrap();
        // assert_eq!(rope.len(), 9);
        // assert_eq!(rope.offset_of_line(1), 1);
        // assert_eq!(rope.line_of_offset(rope.len()), 9);
    }
}
