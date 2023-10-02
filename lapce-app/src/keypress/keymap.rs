use lapce_core::mode::Modes;

use super::KeyPress;

#[derive(PartialEq, Debug)]
pub(super) enum KeymapMatch {
    Full(String),
    Multiple(Vec<String>),
    Prefix,
    None,
}

/// Single keybinding entry in settings
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Modes,
    pub when: Option<String>,
    pub command: String,
}
