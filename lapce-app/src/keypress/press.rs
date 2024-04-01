use floem::keyboard::Modifiers;

use super::{key::KeyInput, keymap::KeyMapPress};

#[derive(Clone, Debug)]
pub struct KeyPress {
    pub(super) key: KeyInput,
    pub(super) mods: Modifiers,
}

impl KeyPress {
    pub fn keymap_press(&self) -> KeyMapPress {
        KeyMapPress {
            key: self.key.keymap_key(),
            mods: self.mods,
        }
    }
}
