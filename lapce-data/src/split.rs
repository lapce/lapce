use druid::{Rect, Size};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

impl SplitDirection {
    pub fn main_size(self, size: Size) -> f64 {
        match self {
            Self::Vertical => size.width,
            Self::Horizontal => size.height,
        }
    }

    pub fn cross_size(self, size: Size) -> f64 {
        match self {
            Self::Vertical => size.height,
            Self::Horizontal => size.width,
        }
    }

    pub fn start(self, rect: Rect) -> f64 {
        match self {
            Self::Vertical => rect.x0,
            Self::Horizontal => rect.y0,
        }
    }
}
