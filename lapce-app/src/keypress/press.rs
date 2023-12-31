use std::fmt::Display;

use floem::keyboard::{Key, ModifiersState, NamedKey};
use tracing::warn;

use super::key::KeyInput;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub(super) key: KeyInput,
    pub(super) mods: ModifiersState,
}

impl KeyPress {
    /// By using floem::keyboard::Key the shift modifier is already applied on characters
    /// There are layouts where $, for example, is not shift+4, therefore we shouldn't rely on phyisical keys
    /// So instead, if we have a key modified by shift, we remove the shift modifier
    pub fn remove_shift_modifier_if_symbol(&self) -> Self {
        let mut mods = self.mods;
        match self.key {
            KeyInput::Keyboard(Key::Character(_)) => {
                mods.set(ModifiersState::SHIFT, false);
            }
            _ => {}
        };

        Self {
            key: self.key.clone(),
            mods,
        }
    }

    pub fn is_char(&self) -> bool {
        let mut mods = self.mods;
        mods.set(ModifiersState::SHIFT, false);
        if mods.is_empty() {
            if let KeyInput::Keyboard(Key::Character(_c)) = &self.key {
                return true;
            }
        }
        false
    }

    pub fn if_is_letter_remove_shift(&self) -> Self {
        let mut mods = self.mods;
        let is_letter = match &self.key {
            KeyInput::Keyboard(Key::Character(c)) => {
                let is_letter = c.to_lowercase() != c.to_uppercase();
                is_letter
            }
            _ => false,
        };
        if is_letter {
            mods.set(ModifiersState::SHIFT, false);
            return Self {
                key: self.key.clone(),
                mods,
            };
        } else {
            return self.clone();
        }
    }

    pub fn is_modifiers(&self) -> bool {
        if let KeyInput::Keyboard(key) = &self.key {
            matches!(
                key,
                Key::Named(NamedKey::Meta)
                    | Key::Named(NamedKey::Super)
                    | Key::Named(NamedKey::Shift)
                    | Key::Named(NamedKey::Control)
                    | Key::Named(NamedKey::Alt)
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

                let key: KeyInput = match key.parse().ok() {
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
