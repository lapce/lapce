use std::fmt::Write;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionMode {
    Delete,
    Yank,
    Indent,
    Outdent,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy, Deserialize, Serialize)]
pub enum VisualMode {
    Normal,
    Linewise,
    Blockwise,
}

impl Default for VisualMode {
    fn default() -> Self {
        VisualMode::Normal
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy, PartialOrd, Ord)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    Terminal,
}

bitflags! {
    pub struct Modes: u32 {
        const NORMAL = 0x1;
        const INSERT = 0x2;
        const VISUAL = 0x4;
        const TERMINAL = 0x8;
    }
}

impl From<Mode> for Modes {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Normal => Self::NORMAL,
            Mode::Insert => Self::INSERT,
            Mode::Visual => Self::VISUAL,
            Mode::Terminal => Self::TERMINAL,
        }
    }
}

impl Modes {
    pub fn parse(modes_str: &str) -> Self {
        let mut this = Self::empty();

        for c in modes_str.chars() {
            match c {
                'i' | 'I' => this.set(Self::INSERT, true),
                'n' | 'N' => this.set(Self::NORMAL, true),
                'v' | 'V' => this.set(Self::VISUAL, true),
                't' | 'T' => this.set(Self::TERMINAL, true),
                _ => log::warn!("Not an editor mode: {c}"),
            }
        }

        this
    }
}

impl std::fmt::Display for Modes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bits = [
            (Self::INSERT, 'i'),
            (Self::NORMAL, 'n'),
            (Self::VISUAL, 'v'),
            (Self::TERMINAL, 't'),
        ];
        for (bit, chr) in bits {
            if self.contains(bit) {
                f.write_char(chr)?;
            }
        }

        Ok(())
    }
}
