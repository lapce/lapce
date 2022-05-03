#[derive(Clone, Debug)]
pub enum LinePosition {
    First,
    Last,
    Line(usize),
}

#[derive(Clone, Debug)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    DocumentStart,
    DocumentEnd,
    FirstNonBlank,
    StartOfLine,
    EndOfLine,
    Line(LinePosition),
    Offset(usize),
    WordEndForward,
    WordForward,
    WordBackward,
    NextUnmatched(char),
    PreviousUnmatched(char),
    MatchPairs,
}

impl PartialEq for Movement {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Movement {
    pub fn is_vertical(&self) -> bool {
        matches!(
            self,
            Movement::Up
                | Movement::Down
                | Movement::Line(_)
                | Movement::DocumentStart
                | Movement::DocumentEnd
        )
    }

    pub fn is_inclusive(&self) -> bool {
        matches!(self, Movement::WordEndForward)
    }

    pub fn is_jump(&self) -> bool {
        matches!(
            self,
            Movement::Line(_)
                | Movement::Offset(_)
                | Movement::DocumentStart
                | Movement::DocumentEnd
        )
    }

    pub fn update_index(
        &self,
        index: usize,
        len: usize,
        count: usize,
        wrapping: bool,
    ) -> usize {
        if len == 0 {
            return 0;
        }
        let last = len - 1;
        match self {
            // Select the next entry/line
            Movement::Down if wrapping => (index + count) % len,
            Movement::Down => (index + count).min(last),

            // Selects the previous entry/line
            Movement::Up if wrapping => (index + (len.saturating_sub(count))) % len,
            Movement::Up => (index.saturating_sub(count)),

            Movement::Line(position) => match position {
                // Selects the nth line
                LinePosition::Line(n) => (*n).min(last),
                LinePosition::First => 0,
                LinePosition::Last => last,
            },
            _ => index,
        }
    }
}
