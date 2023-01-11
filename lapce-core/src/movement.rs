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
    ParagraphForward,
    ParagraphBackward,
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
                | Movement::ParagraphForward
                | Movement::ParagraphBackward
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
                | Movement::ParagraphForward
                | Movement::ParagraphBackward
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
            Movement::Up => index.saturating_sub(count),

            Movement::Line(position) => match position {
                // Selects the nth line
                LinePosition::Line(n) => (*n).min(last),
                LinePosition::First => 0,
                LinePosition::Last => last,
            },

            Movement::ParagraphForward => 0,
            Movement::ParagraphBackward => 0,
            _ => index,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::movement::Movement;

    #[test]
    fn test_wrapping() {
        // Move by 1 position
        // List length of 1
        assert_eq!(0, Movement::Up.update_index(0, 1, 1, true));
        assert_eq!(0, Movement::Down.update_index(0, 1, 1, true));

        // List length of 5
        assert_eq!(4, Movement::Up.update_index(0, 5, 1, true));
        assert_eq!(1, Movement::Down.update_index(0, 5, 1, true));

        // Move by 2 positions
        // List length of 1
        assert_eq!(0, Movement::Up.update_index(0, 1, 2, true));
        assert_eq!(0, Movement::Down.update_index(0, 1, 2, true));

        // List length of 5
        assert_eq!(3, Movement::Up.update_index(0, 5, 2, true));
        assert_eq!(2, Movement::Down.update_index(0, 5, 2, true));
    }

    #[test]
    fn test_non_wrapping() {
        // Move by 1 position
        // List length of 1
        assert_eq!(0, Movement::Up.update_index(0, 1, 1, false));
        assert_eq!(0, Movement::Down.update_index(0, 1, 1, false));

        // List length of 5
        assert_eq!(0, Movement::Up.update_index(0, 5, 1, false));
        assert_eq!(1, Movement::Down.update_index(0, 5, 1, false));

        // Move by 2 positions
        // List length of 1
        assert_eq!(0, Movement::Up.update_index(0, 1, 2, false));
        assert_eq!(0, Movement::Down.update_index(0, 1, 2, false));

        // List length of 5
        assert_eq!(0, Movement::Up.update_index(0, 5, 2, false));
        assert_eq!(2, Movement::Down.update_index(0, 5, 2, false));
    }
}

/// UTF8 line and column-offset
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineCol {
    pub line: usize,
    pub column: usize,
}
