use floem::keyboard::{Key, KeyLocation, NamedKey, PhysicalKey};

use super::keymap::KeyMapKey;

#[derive(Clone, Debug)]
pub(crate) enum KeyInput {
    Keyboard {
        physical: PhysicalKey,
        logical: Key,
        location: KeyLocation,
        key_without_modifiers: Key,
        repeat: bool,
    },
    Pointer(floem::pointer::PointerButton),
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
                    Key::Named(_) => {
                        KeyMapKey::Logical(key_without_modifiers.to_owned())
                    }
                    Key::Character(c) => {
                        if c == " " {
                            KeyMapKey::Logical(Key::Named(NamedKey::Space))
                        } else if c.len() == 1 && c.is_ascii() {
                            KeyMapKey::Logical(Key::Character(
                                c.to_lowercase().into(),
                            ))
                        } else {
                            KeyMapKey::Physical(*physical)
                        }
                    }
                    Key::Unidentified(_) => KeyMapKey::Physical(*physical),
                    Key::Dead(_) => KeyMapKey::Physical(*physical),
                }
            }
        })
    }

    /// For a key whose logical case doesn't need explicit-Shift letter
    /// handling (i.e. anything other than a plain alphabetic character key),
    /// returns a `KeyMapKey` built from the character actually produced by
    /// the user's keyboard layout (`logical`), rather than the unshifted
    /// base character (`key_without_modifiers`) that `keymap_key` uses.
    ///
    /// This makes bindings like `/` or `^` match on any layout:
    /// - On a layout where producing the character requires Shift (e.g. `/`
    ///   via Shift+7 on an Italian keyboard), `logical` already reflects
    ///   the real character while `key_without_modifiers` would not.
    /// - On a layout where the character is the composed result of a dead
    ///   key (e.g. `^` via a dead-key press followed by Space on an Italian
    ///   keyboard), `key_without_modifiers` for that second keypress
    ///   describes the plain key that was pressed (e.g. Space), which is
    ///   unrelated to the composed output - only `logical` has the real
    ///   character.
    ///
    /// Returns `None` for anything this doesn't apply to (alphabetic
    /// character keys, numpad keys, or a `logical` value that isn't a
    /// single ASCII character) - callers should fall back to `keymap_key`
    /// in that case.
    pub fn logical_symbol_key(&self) -> Option<KeyMapKey> {
        let KeyInput::Keyboard {
            key_without_modifiers,
            logical,
            location,
            ..
        } = self
        else {
            return None;
        };

        if matches!(location, KeyLocation::Numpad) {
            return None;
        }

        // Letters keep using key_without_modifiers + an explicit shift bit
        // (handled by keymap_key), since that convention is already
        // layout-portable for Latin-alphabet layouts and this fix doesn't
        // need to touch it.
        if let Key::Character(base) = key_without_modifiers {
            if base.len() == 1
                && base.is_ascii()
                && base.chars().next().unwrap().is_ascii_alphabetic()
            {
                return None;
            }
        }

        let Key::Character(actual) = logical else {
            return None;
        };
        if !(actual.len() == 1 && actual.is_ascii()) {
            return None;
        }

        Some(KeyMapKey::Logical(Key::Character(actual.to_lowercase().into())))
    }
}
