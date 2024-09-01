use floem::keyboard::Modifiers;

use super::{key::KeyInput, keymap::KeyMapPress};

#[derive(Clone, Debug)]
pub struct KeyPress {
    pub(super) key: KeyInput,
    pub(super) mods: Modifiers,
}

impl KeyPress {
    pub fn keymap_press(&self) -> Option<KeyMapPress> {
        self.key.keymap_key().map(|key| KeyMapPress {
            key,
            mods: self.mods,
        })
    }
}
