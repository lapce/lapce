use anyhow::{anyhow, Result};
use indexmap::IndexMap;
use lapce_core::mode::Modes;
use tracing::{debug, error};

use super::{keymap::KeyMap, press::KeyPress};

pub struct KeyMapLoader {
    keymaps: IndexMap<Vec<KeyPress>, Vec<KeyMap>>,
    command_keymaps: IndexMap<String, Vec<KeyMap>>,
}

impl KeyMapLoader {
    pub fn new() -> Self {
        Self {
            keymaps: Default::default(),
            command_keymaps: Default::default(),
        }
    }

    pub fn load_from_str<'a>(
        &'a mut self,
        s: &str,
        modal: bool,
    ) -> Result<&'a mut Self> {
        let toml_keymaps: toml_edit::Document = s.parse()?;
        let toml_keymaps = toml_keymaps
            .get("keymaps")
            .and_then(|v| v.as_array_of_tables())
            .ok_or_else(|| anyhow!("no keymaps"))?;

        for toml_keymap in toml_keymaps {
            let keymap = match Self::get_keymap(toml_keymap, modal) {
                Ok(Some(keymap)) => keymap,
                Ok(None) => {
                    // Keymap ignored
                    continue;
                }
                Err(err) => {
                    error!("Could not parse keymap: {err}");
                    continue;
                }
            };

            let (command, bind) = match keymap.command.strip_prefix('-') {
                Some(cmd) => (cmd.to_string(), false),
                None => (keymap.command.clone(), true),
            };

            let current_keymaps = self.command_keymaps.entry(command).or_default();
            if bind {
                current_keymaps.push(keymap.clone());
                for i in 1..keymap.key.len() + 1 {
                    let key = keymap.key[..i].to_vec();
                    self.keymaps.entry(key).or_default().push(keymap.clone());
                }
            } else {
                let is_keymap = |k: &KeyMap| -> bool {
                    k.when == keymap.when
                        && k.modes == keymap.modes
                        && k.key == keymap.key
                };
                if let Some(index) = current_keymaps.iter().position(is_keymap) {
                    current_keymaps.remove(index);
                }
                for i in 1..keymap.key.len() + 1 {
                    if let Some(keymaps) = self.keymaps.get_mut(&keymap.key[..i]) {
                        if let Some(index) = keymaps.iter().position(is_keymap) {
                            keymaps.remove(index);
                        }
                    }
                }
            }
        }

        Ok(self)
    }

    #[allow(clippy::type_complexity)]
    pub fn finalize(
        self,
    ) -> (
        IndexMap<Vec<KeyPress>, Vec<KeyMap>>,
        IndexMap<String, Vec<KeyMap>>,
    ) {
        let Self {
            keymaps: map,
            command_keymaps: command_map,
        } = self;

        (map, command_map)
    }

    fn get_keymap(
        toml_keymap: &toml_edit::Table,
        modal: bool,
    ) -> Result<Option<KeyMap>> {
        let key = toml_keymap
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("no key in keymap"))?;

        let modes = get_modes(toml_keymap);
        // If not using modal editing, remove keymaps that only make sense in modal.
        if !modal
            && !modes.is_empty()
            && !modes.contains(Modes::INSERT)
            && !modes.contains(Modes::TERMINAL)
        {
            debug!("Keymap ignored: {}", key);
            return Ok(None);
        }

        Ok(Some(KeyMap {
            key: KeyPress::parse(key),
            modes,
            when: toml_keymap
                .get("when")
                .and_then(|w| w.as_str())
                .map(|w| w.to_string()),
            command: toml_keymap
                .get("command")
                .and_then(|c| c.as_str())
                .map(|w| w.trim().to_string())
                .unwrap_or_default(),
        }))
    }
}

fn get_modes(toml_keymap: &toml_edit::Table) -> Modes {
    toml_keymap
        .get("mode")
        .and_then(|v| v.as_str())
        .map(Modes::parse)
        .unwrap_or_else(Modes::empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keymap() {
        let keymaps = r#"
[[keymaps]]
key = "ctrl+w l l"
command = "right"
when = "n"

[[keymaps]]
key = "ctrl+w l"
command = "right"
when = "n"

[[keymaps]]
key = "ctrl+w h"
command = "left"
when = "n"

[[keymaps]]
key = "ctrl+w"
command = "left"
when = "n"

[[keymaps]]
key = "End"
command = "line_end"
when = "n"

[[keymaps]]
key = "shift+i"
command = "insert_first_non_blank"
when = "n"
        
[[keymaps]]
key = "MouseForward"
command = "jump_location_forward"

[[keymaps]]
key = "MouseBackward"
command = "jump_location_backward"
        
[[keymaps]]
key = "Ctrl+MouseMiddle"
command = "goto_definition"
        "#;
        let mut loader = KeyMapLoader::new();
        loader.load_from_str(keymaps, true).unwrap();

        let (keymaps, _) = loader.finalize();

        // Lower case modifiers
        let keypress = KeyPress::parse("ctrl+w");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 4);

        let keypress = KeyPress::parse("ctrl+w l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 2);

        let keypress = KeyPress::parse("ctrl+w h");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("ctrl+w l l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("end");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        // Upper case modifiers
        let keypress = KeyPress::parse("Ctrl+w");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 4);

        let keypress = KeyPress::parse("Ctrl+w l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 2);

        let keypress = KeyPress::parse("Ctrl+w h");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("Ctrl+w l l");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("End");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        // No modifier
        let keypress = KeyPress::parse("shift+i");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        // Mouse keys
        let keypress = KeyPress::parse("MouseForward");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("mousebackward");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);

        let keypress = KeyPress::parse("Ctrl+MouseMiddle");
        assert_eq!(keymaps.get(&keypress).unwrap().len(), 1);
    }
}
