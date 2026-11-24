use floem::keyboard::Modifiers;

use super::{key::KeyInput, keymap::KeyMapPress};

#[derive(Clone, Debug)]
pub struct KeyPress {
    pub(super) key: KeyInput,
    pub(super) mods: Modifiers,
}

impl KeyPress {
    /// The preferred key representation for matching against keymaps.
    ///
    /// For non-alphabetic character keys (digits, punctuation) this is
    /// built from the character actually produced by the user's keyboard
    /// layout, with Shift/AltGr absorbed into the character rather than
    /// tracked as modifiers - so e.g. `/` matches on any layout, not just
    /// ones where `/` happens to be unshifted. Everything else (letters,
    /// named keys) is unchanged from `legacy_keymap_press`.
    pub fn keymap_press(&self) -> Option<KeyMapPress> {
        if let Some(key) = self.key.logical_symbol_key() {
            let mut mods = self.mods;
            mods.set(Modifiers::SHIFT, false);
            mods.set(Modifiers::ALTGR, false);
            return Some(KeyMapPress { key, mods });
        }

        self.legacy_keymap_press()
    }

    /// The pre-existing key representation: the key's unshifted base
    /// character (or named key) plus the full modifier set. Kept as a
    /// fallback so custom `keymaps.toml` entries written with the old
    /// `shift+<digit>` convention for symbols keep matching.
    pub fn legacy_keymap_press(&self) -> Option<KeyMapPress> {
        self.key.keymap_key().map(|key| KeyMapPress {
            key,
            mods: self.mods,
        })
    }
}

#[cfg(test)]
mod tests {
    use floem::keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey};

    use super::*;
    use crate::keypress::keymap::KeyMapKey;

    fn char_key(
        key_without_modifiers: &str,
        logical: &str,
        mods: Modifiers,
    ) -> KeyPress {
        KeyPress {
            key: KeyInput::Keyboard {
                physical: PhysicalKey::Code(KeyCode::Digit7),
                logical: Key::Character(logical.into()),
                location: KeyLocation::Standard,
                key_without_modifiers: Key::Character(key_without_modifiers.into()),
                repeat: false,
            },
            mods,
        }
    }

    #[test]
    fn slash_matches_via_actual_character_on_shifted_layout() {
        // Italian layout: "/" is produced by Shift+7.
        let press = char_key("7", "/", Modifiers::SHIFT);

        let matched = press.keymap_press().unwrap();
        assert_eq!(matched.key, KeyMapKey::Logical(Key::Character("/".into())));
        assert!(matched.mods.is_empty());
    }

    #[test]
    fn slash_matches_directly_on_unshifted_layout() {
        // US layout: "/" is its own unshifted key.
        let press = char_key("/", "/", Modifiers::empty());

        let matched = press.keymap_press().unwrap();
        assert_eq!(matched.key, KeyMapKey::Logical(Key::Character("/".into())));
        assert!(matched.mods.is_empty());
    }

    #[test]
    fn legacy_representation_is_preserved_for_backward_compatibility() {
        // US layout event for Shift+4 ("$"), shaped like what an old
        // "shift+4" custom binding expects.
        let press = char_key("4", "$", Modifiers::SHIFT);

        let legacy = press.legacy_keymap_press().unwrap();
        assert_eq!(legacy.key, KeyMapKey::Logical(Key::Character("4".into())));
        assert_eq!(legacy.mods, Modifiers::SHIFT);

        // The primary representation is the new, layout-correct one.
        let primary = press.keymap_press().unwrap();
        assert_eq!(primary.key, KeyMapKey::Logical(Key::Character("$".into())));
        assert!(primary.mods.is_empty());
    }

    #[test]
    fn ctrl_modifier_is_preserved_for_symbol_keys() {
        // ctrl+/ on a US layout: Ctrl is a real modifier, not absorbed.
        let press = char_key("/", "/", Modifiers::CONTROL);

        let matched = press.keymap_press().unwrap();
        assert_eq!(matched.key, KeyMapKey::Logical(Key::Character("/".into())));
        assert_eq!(matched.mods, Modifiers::CONTROL);
    }

    #[test]
    fn dead_key_composed_symbol_matches_via_actual_character() {
        // Italian layout: "^" is a dead key. Pressing it then Space
        // produces the standalone "^" character; the event for that
        // second keypress has key_without_modifiers = Space (the physical
        // key that was pressed), which is unrelated to the composed
        // output - only `logical` carries the real character.
        let press = KeyPress {
            key: KeyInput::Keyboard {
                physical: PhysicalKey::Code(KeyCode::Space),
                logical: Key::Character("^".into()),
                location: KeyLocation::Standard,
                key_without_modifiers: Key::Named(NamedKey::Space),
                repeat: false,
            },
            mods: Modifiers::empty(),
        };

        let matched = press.keymap_press().unwrap();
        assert_eq!(matched.key, KeyMapKey::Logical(Key::Character("^".into())));
        assert!(matched.mods.is_empty());
    }

    #[test]
    fn unshifted_digit_is_unaffected() {
        // Plain "4" (e.g. vim count prefix): no Shift/AltGr involved, new
        // and legacy representations coincide.
        let press = char_key("4", "4", Modifiers::empty());

        assert_eq!(press.keymap_press(), press.legacy_keymap_press());
    }

    #[test]
    fn alphabetic_keys_use_legacy_representation() {
        // Shift+A: letters are out of scope for this fix, unchanged
        // behavior (case tracked via explicit Shift modifier).
        let press = char_key("a", "A", Modifiers::SHIFT);

        let matched = press.keymap_press().unwrap();
        assert_eq!(matched.key, KeyMapKey::Logical(Key::Character("a".into())));
        assert_eq!(matched.mods, Modifiers::SHIFT);
    }
}
