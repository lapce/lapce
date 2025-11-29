use super::keymap::KeyMapKey;
use floem::ui_events::keyboard::{
    Code as KeyCode, Key, Location as KeyLocation, NamedKey,
};

#[derive(Clone, Debug)]
pub(crate) enum KeyInput {
    Keyboard {
        physical: KeyCode,
        logical: Key,
        location: KeyLocation,
        key_without_modifiers: Key,
        repeat: bool,
    },
    Pointer(floem::ui_events::pointer::PointerButton),
}

impl KeyInput {
    pub fn keymap_key(&self) -> Option<KeyMapKey> {
        if let KeyInput::Keyboard {
            repeat, logical, ..
        } = self
        {
            if *repeat
                && (matches!(
                    logical,
                    Key::Named(NamedKey::Meta)
                        | Key::Named(NamedKey::Shift)
                        | Key::Named(NamedKey::Alt)
                        | Key::Named(NamedKey::Control),
                ))
            {
                return None;
            }
        }

        Some(match self {
            KeyInput::Pointer(b) => KeyMapKey::Pointer(*b),
            KeyInput::Keyboard {
                physical,
                key_without_modifiers,
                logical,
                location,
                ..
            } => {
                #[allow(clippy::single_match)]
                match location {
                    KeyLocation::Numpad => {
                        return Some(KeyMapKey::Logical(logical.to_owned()));
                    }
                    _ => {}
                }

                match key_without_modifiers {
                    Key::Named(NamedKey::Dead | NamedKey::Unidentified) => {
                        KeyMapKey::Physical(*physical)
                    }
                    Key::Named(_) => {
                        KeyMapKey::Logical(key_without_modifiers.to_owned())
                    }
                    Key::Character(c) => {
                        if c == " " {
                            KeyMapKey::Logical(Key::Character(c.clone()))
                        } else if c.len() == 1 && c.is_ascii() {
                            KeyMapKey::Logical(Key::Character(c.to_lowercase()))
                        } else {
                            KeyMapKey::Physical(*physical)
                        }
                    }
                }
            }
        })
    }
}
