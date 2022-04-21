use strum_macros::{Display, EnumIter, EnumMessage, EnumString};

#[derive(Display, EnumString, EnumIter, Clone, PartialEq, Debug, EnumMessage)]
pub enum MoveCommand {
    #[strum(serialize = "down")]
    Down,
    #[strum(serialize = "up")]
    Up,
    #[strum(serialize = "left")]
    Left,
    #[strum(serialize = "right")]
    Right,
    #[strum(serialize = "word_backward")]
    WordBackward,
    #[strum(serialize = "word_forward")]
    WordForward,
    #[strum(serialize = "word_end_forward")]
    WordEndForward,
    #[strum(message = "Document Start")]
    #[strum(serialize = "document_start")]
    DocumentStart,
    #[strum(message = "Document End")]
    #[strum(serialize = "document_end")]
    DocumentEnd,
    #[strum(serialize = "line_end")]
    LineEnd,
    #[strum(serialize = "line_start")]
    LineStart,
    #[strum(serialize = "line_start_non_blank")]
    LineStartNonBlank,
    #[strum(serialize = "go_to_line_default_last")]
    GotoLineDefaultLast,
    #[strum(serialize = "go_to_line_default_first")]
    GotoLineDefaultFirst,
    #[strum(serialize = "match_pairs")]
    MatchPairs,
    #[strum(serialize = "next_unmatched_right_bracket")]
    NextUnmatchedRightBracket,
    #[strum(serialize = "previous_unmatched_left_bracket")]
    PreviousUnmatchedLeftBracket,
    #[strum(serialize = "next_unmatched_right_curly_bracket")]
    NextUnmatchedRightCurlyBracket,
    #[strum(serialize = "previous_unmatched_left_curly_bracket")]
    PreviousUnmatchedLeftCurlyBracket,
}

impl MoveCommand {
    pub fn to_movement(&self, count: Option<usize>) -> Movement {
        use MoveCommand::*;
        match self {
            Left => Movement::Left,
            Right => Movement::Right,
            Up => Movement::Up,
            Down => Movement::Down,
            DocumentStart => Movement::DocumentStart,
            DocumentEnd => Movement::DocumentEnd,
            LineStart => Movement::StartOfLine,
            LineStartNonBlank => Movement::FirstNonBlank,
            LineEnd => Movement::EndOfLine,
            GotoLineDefaultFirst => match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::First),
            },
            GotoLineDefaultLast => match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::Last),
            },
            WordBackward => Movement::WordBackward,
            WordForward => Movement::WordForward,
            WordEndForward => Movement::WordEndForward,
            MatchPairs => Movement::MatchPairs,
            NextUnmatchedRightBracket => Movement::NextUnmatched(')'),
            PreviousUnmatchedLeftBracket => Movement::PreviousUnmatched('('),
            NextUnmatchedRightCurlyBracket => Movement::NextUnmatched('}'),
            PreviousUnmatchedLeftCurlyBracket => Movement::PreviousUnmatched('{'),
        }
    }
}

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
