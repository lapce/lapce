use druid::shell::keyboard::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use strum;
use strum_macros::{Display, EnumProperty, EnumString};

#[derive(EnumString, Display, Clone, PartialEq)]
pub enum InputState {
    #[strum(serialize = "normal", serialize = "n")]
    Normal,
    #[strum(serialize = "insert", serialize = "i")]
    Insert,
    #[strum(serialize = "visual", serialize = "v")]
    Visual,
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
            "up" => key_code = KeyCode::ArrowUp,
            "down" => key_code = KeyCode::ArrowDown,
            "left" => key_code = KeyCode::ArrowLeft,
            "right" => key_code = KeyCode::ArrowRight,
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
        r.push_str(match self.key_code {
            KeyCode::Escape => "esc",
            KeyCode::Tab => "tab",
            KeyCode::Backspace => "bs",
            KeyCode::Return => "cr",
            KeyCode::ArrowUp => "up",
            KeyCode::ArrowDown => "down",
            KeyCode::ArrowLeft => "left",
            KeyCode::ArrowRight => "right",
            _ => &self.text,
        });
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

        keymap.add("n", "u", Command::Undo);
        keymap.add("n", "<C-r>", Command::Redo);
        keymap.add("n", "i", Command::Insert);
        keymap.add("n", "I", Command::InsertStartOfLine);
        keymap.add("n", "A", Command::AppendEndOfLine);
        keymap.add("n", "o", Command::NewLineBelow);
        keymap.add("n", "O", Command::NewLineAbove);
        keymap.add("n", "<M-;>", Command::SplitVertical);
        keymap.add("n", "<C-w>v", Command::SplitVertical);
        keymap.add("n", "<m-h>", Command::MoveCursorToWindowLeft);
        keymap.add("n", "<m-l>", Command::MoveCursorToWindowRight);

        keymap.add("nv", "v", Command::Visual);
        keymap.add("nv", "V", Command::VisualLine);

        keymap.add("nv", "x", Command::DeleteForward);
        keymap.add("nv", "s", Command::DeleteForwardInsert);

        keymap.add("inv", "<down>", Command::MoveDown);
        keymap.add("inv", "<up>", Command::MoveUp);
        keymap.add("inv", "<left>", Command::MoveLeft);
        keymap.add("inv", "<right>", Command::MoveRight);

        keymap.add("nv", "k", Command::MoveUp);
        keymap.add("nv", "j", Command::MoveDown);
        keymap.add("nv", "h", Command::MoveLeft);
        keymap.add("nv", "l", Command::MoveRight);
        keymap.add("nv", "b", Command::MoveWordLeft);
        keymap.add("nv", "e", Command::MoveWordRight);
        keymap.add("nv", "0", Command::MoveStartOfLine);
        keymap.add("nv", "$", Command::MoveEndOfLine);
        keymap.add("nv", "<C-u>", Command::ScrollPageUp);
        keymap.add("nv", "<C-d>", Command::ScrollPageDown);

        keymap.add("v", "<Esc>", Command::Escape);

        keymap.add("i", "<Esc>", Command::Escape);
        keymap.add("i", "<bs>", Command::DeleteBackward);
        keymap.add("i", "<C-h>", Command::DeleteBackward);
        keymap.add("i", "<cr>", Command::InsertNewLine);
        keymap.add("i", "<C-m>", Command::InsertNewLine);
        keymap.add("i", "<Tab>", Command::InsertTab);

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

    fn add(&mut self, state_strings: &str, input_strings: &str, command: Command) {
        let inputs = KeyInput::from_strings(input_strings.to_string());
        let len = inputs.len();
        for i in 0..len {
            for state_char in state_strings.chars() {
                if let Ok(state) = InputState::from_str(&state_char.to_string()) {
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
    #[strum(serialize = "visual", props(description = ""))]
    Visual,
    #[strum(serialize = "visual_line", props(description = ""))]
    VisualLine,
    #[strum(serialize = "escape", props(description = ""))]
    Escape,
    #[strum(serialize = "undo", props(description = ""))]
    Undo,
    #[strum(serialize = "redo", props(description = ""))]
    Redo,
    #[strum(serialize = "delete_forward_insert", props(description = ""))]
    DeleteForwardInsert,
    #[strum(serialize = "delete_forward", props(description = ""))]
    DeleteForward,
    #[strum(serialize = "delete_backward", props(description = ""))]
    DeleteBackward,
    #[strum(serialize = "move_cursor_to_window_below", props(description = ""))]
    MoveCursorToWindowBelow,
    #[strum(serialize = "move_cursor_to_window_above", props(description = ""))]
    MoveCursorToWindowAbove,
    #[strum(serialize = "move_cursor_to_window_left", props(description = ""))]
    MoveCursorToWindowLeft,
    #[strum(serialize = "move_cursor_to_window_right", props(description = ""))]
    MoveCursorToWindowRight,
    #[strum(serialize = "split_vertical", props(description = ""))]
    SplitVertical,
    #[strum(serialize = "split_horizontal", props(description = ""))]
    SplitHorizontal,
    #[strum(serialize = "scroll_page_up", props(description = ""))]
    ScrollPageUp,
    #[strum(serialize = "scroll_page_down", props(description = ""))]
    ScrollPageDown,
    #[strum(serialize = "move_down", props(description = ""))]
    MoveUp,
    #[strum(serialize = "move_down", props(description = ""))]
    MoveDown,
    #[strum(serialize = "move_left", props(description = ""))]
    MoveLeft,
    #[strum(serialize = "move_right", props(description = ""))]
    MoveRight,
    #[strum(serialize = "move_word_left", props(description = ""))]
    MoveWordLeft,
    #[strum(serialize = "move_word_right", props(description = ""))]
    MoveWordRight,
    #[strum(serialize = "move_start_of_line", props(description = ""))]
    MoveStartOfLine,
    #[strum(serialize = "move_end_of_line", props(description = ""))]
    MoveEndOfLine,
    #[strum(serialize = "insert_start_of_line", props(description = ""))]
    InsertStartOfLine,
    #[strum(serialize = "append_end_of_line", props(description = ""))]
    AppendEndOfLine,
    #[strum(serialize = "new_line_below", props(description = ""))]
    NewLineBelow,
    #[strum(serialize = "new_line_above", props(description = ""))]
    NewLineAbove,
    #[strum(serialize = "insert_new_line", props(description = ""))]
    InsertNewLine,
    #[strum(serialize = "insert_tab", props(description = ""))]
    InsertTab,
    #[strum(serialize = "unknown", props(description = ""))]
    Unknown,
}

pub struct Input {
    pub state: InputState,
    pub visual_line: bool,
    pub count: u64,
    pub pending_keys: Vec<KeyInput>,
}

impl Input {
    pub fn new() -> Input {
        Input {
            state: InputState::Normal,
            visual_line: false,
            count: 0,
            pending_keys: Vec::new(),
        }
    }
}
