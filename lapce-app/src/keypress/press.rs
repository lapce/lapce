use floem::glazier::{KbKey, Modifiers};

use super::key::Key;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub(super) key: Key,
    pub(super) mods: Modifiers,
}

impl KeyPress {
    pub fn to_lowercase(&self) -> Self {
        let key = match &self.key {
            Key::Keyboard(KbKey::Character(c)) => {
                Key::Keyboard(KbKey::Character(c.to_lowercase()))
            }
            _ => self.key.clone(),
        };
        Self {
            key,
            mods: self.mods,
        }
    }

    pub fn is_char(&self) -> bool {
        let mut mods = self.mods;
        mods.set(Modifiers::SHIFT, false);
        if mods.is_empty() {
            if let Key::Keyboard(KbKey::Character(_c)) = &self.key {
                return true;
            }
        }
        false
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
                        log::warn!("Unrecognized key: {key}");
                        return None;
                    }
                };

                let mut mods = Modifiers::default();
                for part in modifiers.to_lowercase().split('+') {
                    match part {
                        "ctrl" => mods.set(Modifiers::CONTROL, true),
                        "meta" => mods.set(Modifiers::META, true),
                        "shift" => mods.set(Modifiers::SHIFT, true),
                        "alt" => mods.set(Modifiers::ALT, true),
                        "" => (),
                        other => log::warn!("Invalid key modifier: {}", other),
                    }
                }

                Some(KeyPress { key, mods })
            })
            .collect()
    }
}
