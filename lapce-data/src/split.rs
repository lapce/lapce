use crate::keypress::KeyPress;

use druid::Size;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

impl SplitDirection {
    pub fn main_size(self, size: Size) -> f64 {
        match self {
            SplitDirection::Vertical => size.width,
            SplitDirection::Horizontal => size.height,
        }
    }

    pub fn cross_size(self, size: Size) -> f64 {
        match self {
            SplitDirection::Vertical => size.height,
            SplitDirection::Horizontal => size.width,
        }
    }
}

pub fn keybinding_to_string(keypress: &KeyPress) -> String {
    let mut keymap_str = "".to_string();
    if keypress.mods.ctrl() {
        keymap_str += "Ctrl+";
    }
    if keypress.mods.alt() {
        keymap_str += "Alt+";
    }
    if keypress.mods.meta() {
        let keyname = match std::env::consts::OS {
            "macos" => "Cmd",
            "windows" => "Win",
            _ => "Meta",
        };
        keymap_str += keyname;
        keymap_str += "+";
    }
    if keypress.mods.shift() {
        keymap_str += "Shift+";
    }
    keymap_str += &keypress.key.to_string();
    keymap_str
}
