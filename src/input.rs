use druid::shell::keyboard::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use strum;
use strum_macros::{Display, EnumProperty, EnumString};

#[derive(EnumString, Display, Clone, PartialEq)]
pub enum InputState {
    #[strum(serialize = "normal")]
    Nomral,
    #[strum(serialize = "insert")]
    Insert,
}

#[derive(Clone)]
pub struct KeyInput {
    key_code: KeyCode,
    mods: KeyModifiers,
    pub text: String,
    state: InputState,
}

impl FromStr for KeyInput {
    type Err = fmt::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let v: Vec<&str> = s.split('-').collect();
        let mut mods = KeyModifiers::default();
        let mut key_code = KeyCode::Key0;
        let mut text = "".to_string();
        let state = match v[0] {
            "n" => InputState::Nomral,
            _ => InputState::Insert,
        };
        for e in v[1..].iter() {
            match *e {
                "alt" => mods.alt = true,
                "ctrl" => mods.ctrl = true,
                "meta" => mods.meta = true,
                "escape" => key_code = KeyCode::Escape,
                "esc" => key_code = KeyCode::Escape,
                _ => text = e.to_string(),
            };
        }
        Ok(KeyInput {
            key_code,
            mods,
            text,
            state,
        })
    }
}

impl Display for KeyInput {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if self.mods.alt {
            write!(f, "alt-");
        }
        if self.mods.ctrl {
            write!(f, "ctrl-");
        }
        if self.mods.meta {
            write!(f, "meta-");
        }
        match self.key_code {
            KeyCode::Escape => write!(f, "escpe"),
            KeyCode::Tab => write!(f, "tab"),
            _ => write!(f, "{}", self.text),
        }
    }
}

impl KeyInput {
    pub fn new() -> HashMap<String, Command> {
        let mut map = HashMap::new();

        map.insert(
            KeyInput::from_str("n-i").unwrap().get_key(),
            Command::Insert,
        );
        map.insert(
            KeyInput::from_str("i-esc").unwrap().get_key(),
            Command::Escape,
        );

        map
    }

    pub fn get_key(&self) -> String {
        format!("{}-{}", self, self.state)
    }

    pub fn from_keyevent(event: &KeyEvent, state: InputState) -> KeyInput {
        KeyInput {
            key_code: event.key_code.clone(),
            mods: event.mods.clone(),
            text: event.unmod_text().unwrap_or("").to_string(),
            state,
        }
    }
}

#[derive(EnumProperty, EnumString, Debug)]
pub enum Command {
    #[strum(serialize = "insert", props(description = ""))]
    Insert,
    #[strum(serialize = "escape", props(description = ""))]
    Escape,
    #[strum(serialize = "unknown", props(description = ""))]
    Unknown,
}

pub struct Input {
    pub state: InputState,
}

impl Input {
    pub fn new() -> Input {
        Input {
            state: InputState::Nomral,
        }
    }
}
