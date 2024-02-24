use std::fmt::Display;

use floem::keyboard::{Key, KeyCode, ModifiersState, PhysicalKey};
use tracing::warn;

use super::key::KeyInput;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub(super) key: KeyInput,
    pub(super) mods: ModifiersState,
}

impl KeyPress {
    pub fn to_lowercase(&self) -> Self {
        let key = match &self.key {
            KeyInput::Keyboard(Key::Character(c), key_code) => KeyInput::Keyboard(
                Key::Character(c.to_lowercase().into()),
                *key_code,
            ),
            _ => self.key.clone(),
        };
        Self {
            key,
            mods: self.mods,
        }
    }

    pub fn is_char(&self) -> bool {
        let mut mods = self.mods;
        mods.set(ModifiersState::SHIFT, false);
        if mods.is_empty() {
            if let KeyInput::Keyboard(Key::Character(_c), _) = &self.key {
                return true;
            }
        }
        false
    }

    pub fn is_modifiers(&self) -> bool {
        if let KeyInput::Keyboard(_, scancode) = &self.key {
            matches!(
                scancode,
                PhysicalKey::Code(KeyCode::Meta)
                    | PhysicalKey::Code(KeyCode::SuperLeft)
                    | PhysicalKey::Code(KeyCode::SuperRight)
                    | PhysicalKey::Code(KeyCode::ShiftLeft)
                    | PhysicalKey::Code(KeyCode::ShiftRight)
                    | PhysicalKey::Code(KeyCode::ControlLeft)
                    | PhysicalKey::Code(KeyCode::ControlRight)
                    | PhysicalKey::Code(KeyCode::AltLeft)
                    | PhysicalKey::Code(KeyCode::AltRight)
            )
        } else {
            false
        }
    }

    pub fn label(&self) -> String {
        let mut keys = String::from("");
        if self.mods.control_key() {
            keys.push_str("Ctrl+");
        }
        if self.mods.alt_key() {
            keys.push_str("Alt+");
        }
        if self.mods.super_key() {
            let keyname = match std::env::consts::OS {
                "macos" => "Cmd+",
                "windows" => "Win+",
                _ => "Meta+",
            };
            keys.push_str(keyname);
        }
        if self.mods.shift_key() {
            keys.push_str("Shift+");
        }
        keys.push_str(&self.key.to_string());
        keys.trim().to_string()
    }

    pub fn parse(key: &str) -> Vec<Self> {
        key.split(' ')
            .filter_map(|k| {
                let (modifiers, key) = match k.rsplit_once('+') {
                    Some(pair) => pair,
                    None => ("", k),
                };

                let key = match key.parse().ok() {
                    Some(key) => key,
                    None => {
                        // Skip past unrecognized key definitions
                        warn!("Unrecognized key: {key}");
                        return None;
                    }
                };

                let mut mods = ModifiersState::empty();
                for part in modifiers.to_lowercase().split('+') {
                    match part {
                        "ctrl" => mods.set(ModifiersState::CONTROL, true),
                        "meta" => mods.set(ModifiersState::SUPER, true),
                        "shift" => mods.set(ModifiersState::SHIFT, true),
                        "alt" => mods.set(ModifiersState::ALT, true),
                        "" => (),
                        other => warn!("Invalid key modifier: {}", other),
                    }
                }

                Some(KeyPress { key, mods })
            })
            .collect()
    }
}

impl Display for KeyPress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.mods.contains(ModifiersState::CONTROL) {
            let _ = f.write_str("Ctrl+");
        }
        if self.mods.contains(ModifiersState::ALT) {
            let _ = f.write_str("Alt+");
        }
        if self.mods.contains(ModifiersState::SUPER) {
            let _ = f.write_str("Meta+");
        }
        if self.mods.contains(ModifiersState::SHIFT) {
            let _ = f.write_str("Shift+");
        }
        f.write_str(&self.key.to_string())
    }
}
