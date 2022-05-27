use druid::Size;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

impl SplitDirection {
    pub fn main_size(self, size: Size) -> f64 {
        match self {
            SplitDirection::Vertical => size.width,
            SplitDirection::Horizontal => size.height,
        }
    }

    pub fn cross_size(self, size: Size) -> f64 {
        match self {
            SplitDirection::Vertical => size.height,
            SplitDirection::Horizontal => size.width,
        }
    }
}
