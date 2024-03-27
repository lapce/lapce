use floem::keyboard::{Key, NamedKey, PhysicalKey};

use super::keymap::KeyMapKey;

#[derive(Clone, Debug)]
pub(crate) enum KeyInput {
    Keyboard {
        physical: PhysicalKey,
        logical: Key,
        key_without_modifiers: Key,
    },
    Pointer(floem::pointer::PointerButton),
}

impl KeyInput {
    pub fn keymap_key(&self) -> KeyMapKey {
        match self {
            KeyInput::Pointer(b) => KeyMapKey::Pointer(*b),
            KeyInput::Keyboard {
                physical,
                key_without_modifiers,
                ..
            } => match key_without_modifiers {
                Key::Named(_) => {
                    KeyMapKey::Logical(key_without_modifiers.to_owned())
                }
                Key::Character(c) => {
                    if c == " " {
                        KeyMapKey::Logical(Key::Named(NamedKey::Space))
                    } else if c.len() == 1 && c.is_ascii() {
                        KeyMapKey::Logical(Key::Character(c.to_lowercase().into()))
                    } else {
                        KeyMapKey::Physical(*physical)
                    }
                }
                Key::Unidentified(_) => KeyMapKey::Physical(*physical),
                Key::Dead(_) => KeyMapKey::Physical(*physical),
            },
        }
    }
}
