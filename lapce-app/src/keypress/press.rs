use floem::keyboard::{Key, Modifiers, NamedKey};

use super::{key::KeyInput, keymap::KeyMapPress};

#[derive(Clone, Debug)]
pub struct KeyPress {
    pub(super) key: KeyInput,
    pub(super) mods: Modifiers,
}

impl KeyPress {
    #[tracing::instrument]
    pub fn keymap_press(&self) -> KeyMapPress {
        KeyMapPress {
            key: self.key.keymap_key(),
            mods: self.mods,
        }
    }

    pub fn only_shift(&self) -> bool {
        if let KeyInput::Keyboard {
            key_without_modifiers,
            ..
        } = &self.key
        {
            *key_without_modifiers == Key::Named(NamedKey::Shift)
        } else {
            false
        }
    }

    pub fn filter_out_key(&self) -> bool {
        if let KeyInput::Keyboard {
            key_without_modifiers,
            ..
        } = &self.key
        {
            *key_without_modifiers == Key::Named(NamedKey::Alt)
                || *key_without_modifiers == Key::Named(NamedKey::Control)
                || (*key_without_modifiers == Key::Named(NamedKey::Shift)
                    && !self.mods.contains(Modifiers::SHIFT))
        } else {
            false
        }
    }
}
