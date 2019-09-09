use druid::shell::keyboard::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use strum;
use strum_macros::{Display, EnumProperty, EnumString};

#[derive(EnumString, Display, Clone, PartialEq)]
pub enum InputState {
    #[strum(serialize = "normal")]
    Normal,
    #[strum(serialize = "insert")]
    Insert,
}

#[derive(Clone)]
pub struct KeyInput {
    pub key_code: KeyCode,
    pub mods: KeyModifiers,
    pub text: String,
}

impl FromStr for KeyInput {
    type Err = fmt::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let v: Vec<&str> = s.split('-').collect();
        let mut mods = KeyModifiers::default();
        let mut key_code = KeyCode::Key0;
        let mut text = "".to_string();
        for e in &v[..v.len() - 1] {
            match e.to_lowercase().as_ref() {
                "a" => mods.alt = true,
                "c" => mods.ctrl = true,
                "m" => mods.meta = true,
                _ => (),
            };
        }
        match v[v.len() - 1].to_lowercase().as_ref() {
            "tab" => key_code = KeyCode::Tab,
            "esc" => key_code = KeyCode::Escape,
            "bs" => key_code = KeyCode::Backspace,
            "cr" => key_code = KeyCode::Return,
            _ => text = v[v.len() - 1].to_string(),
        }
        Ok(KeyInput {
            key_code,
            mods,
            text,
        })
    }
}

impl Display for KeyInput {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let mut special = false;
        let mut r = "".to_string();
        if self.mods.alt {
            r.push_str("a-");
        }
        if self.mods.ctrl {
            r.push_str("c-");
        }
        if self.mods.meta {
            r.push_str("m-");
        }
        match self.key_code {
            KeyCode::Escape => {
                r.push_str("esc");
            }
            KeyCode::Tab => {
                r.push_str("tab");
            }
            KeyCode::Backspace => {
                r.push_str("bs");
            }
            _ => r.push_str(&self.text),
        }
        if r.len() > 1 {
            r = format!("<{}>", r)
        }
        write!(f, "{}", r)
    }
}

impl KeyInput {
    pub fn from_keyevent(event: &KeyEvent) -> KeyInput {
        KeyInput {
            key_code: event.key_code.clone(),
            mods: event.mods.clone(),
            text: event.unmod_text().unwrap_or("").to_string(),
        }
    }

    pub fn from_strings(s: String) -> Vec<KeyInput> {
        let mut keys = Vec::new();
        let mut special = false;
        let mut special_key = "".to_string();
        for c in s.chars() {
            if c == '<' {
                special = true;
            } else if c == '>' {
                if special {
                    keys.push(special_key.to_string());
                    special = false;
                } else {
                    keys.push(c.to_string());
                }
            } else {
                if special {
                    special_key.push(c);
                } else {
                    keys.push(c.to_string());
                }
            }
        }
        keys.iter()
            .map(|s| KeyInput::from_str(s).unwrap())
            .collect()
    }
}

pub struct KeyMap {
    map: HashMap<String, Cmd>,
}

impl KeyMap {
    pub fn new() -> KeyMap {
        let mut keymap = KeyMap {
            map: HashMap::new(),
        };

        keymap.add(InputState::Normal, "i", Command::Insert);
        keymap.add(InputState::Normal, "k", Command::MoveUp);
        keymap.add(InputState::Normal, "j", Command::MoveDown);
        keymap.add(InputState::Normal, "h", Command::MoveLeft);
        keymap.add(InputState::Normal, "l", Command::MoveRight);
        keymap.add(InputState::Normal, "<M-;>", Command::SplitVertical);
        keymap.add(InputState::Normal, "<C-w>v", Command::SplitVertical);

        keymap.add(InputState::Insert, "<Esc>", Command::Escape);
        keymap.add(InputState::Insert, "<bs>", Command::DeleteBackward);

        println!("keys is {:?}", &keymap.map.keys());
        keymap
    }

    pub fn get(&self, state: InputState, inputs: Vec<KeyInput>) -> Cmd {
        let key = inputs
            .iter()
            .map(|i| format!("{} {}", state, i))
            .collect::<Vec<String>>()
            .join(" ");
        self.map
            .get(&key)
            .unwrap_or(&Cmd {
                cmd: Some(Command::Unknown),
                more_input: false,
            })
            .clone()
    }

    fn add(&mut self, state: InputState, input_strings: &str, command: Command) {
        let inputs = KeyInput::from_strings(input_strings.to_string());
        let len = inputs.len();
        for i in 0..len {
            let input = inputs[..i + 1]
                .iter()
                .map(|i| format!("{} {}", state, i))
                .collect::<Vec<String>>()
                .join(" ");
            println!("input is {}", input);
            if i == len - 1 {
                if let Some(cmd) = self.map.get_mut(&input) {
                    cmd.cmd = Some(command.clone());
                } else {
                    let cmd = Cmd {
                        cmd: Some(command.clone()),
                        more_input: false,
                    };
                    self.map.insert(input, cmd);
                }
            } else {
                if let Some(cmd) = self.map.get_mut(&input) {
                    cmd.more_input = true;
                } else {
                    let cmd = Cmd {
                        cmd: None,
                        more_input: true,
                    };
                    self.map.insert(input, cmd);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Cmd {
    pub cmd: Option<Command>,
    pub more_input: bool,
}

#[derive(EnumProperty, EnumString, Debug, Clone, PartialEq)]
pub enum Command {
    #[strum(serialize = "insert", props(description = ""))]
    Insert,
    #[strum(serialize = "escape", props(description = ""))]
    Escape,
    #[strum(serialize = "delete_backward", props(description = ""))]
    DeleteBackward,
    #[strum(serialize = "split_vertical", props(description = ""))]
    SplitVertical,
    #[strum(serialize = "split_horizontal", props(description = ""))]
    SplitHorizontal,
    #[strum(serialize = "move_up", props(description = ""))]
    MoveUp,
    #[strum(serialize = "move_down", props(description = ""))]
    MoveDown,
    #[strum(serialize = "move_left", props(description = ""))]
    MoveLeft,
    #[strum(serialize = "move_right", props(description = ""))]
    MoveRight,
    #[strum(serialize = "unknown", props(description = ""))]
    Unknown,
}

pub struct Input {
    pub state: InputState,
    pub count: u64,
    pub pending_keys: Vec<KeyInput>,
}

impl Input {
    pub fn new() -> Input {
        Input {
            state: InputState::Normal,
            count: 0,
            pending_keys: Vec::new(),
        }
    }
}
