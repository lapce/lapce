use std::fmt::Display;

use lapce_core::mode::Modes;

use super::KeyPress;

#[derive(PartialEq, Debug)]
pub(super) enum KeymapMatch {
    Full(String),
    Multiple(Vec<String>),
    Prefix,
    None,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct KeyMap {
    pub key: Vec<KeyPress>,
    pub modes: Modes,
    pub when: Option<String>,
    pub command: String,
}

impl Display for KeyMap {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok({
            println!("thissss \n");
            for i in self.key.iter() {
                println!("{}", i);
            }
            println!("\n");
            
            println!("{}", self.modes);
            println!( "{:?}", self.when);
            println!( "\n");
        })
    }
}
