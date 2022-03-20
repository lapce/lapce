use serde::{Deserialize, Serialize};

use crate::keypress::KeyPress;

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
