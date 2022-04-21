use strum_macros::{Display, EnumIter, EnumMessage, EnumString};

use crate::movement::{LinePosition, Movement};

#[derive(Display, EnumString, EnumIter, Clone, PartialEq, Debug, EnumMessage)]
pub enum EditCommand {
    #[strum(serialize = "move_line_up")]
    MoveLineUp,

    #[strum(serialize = "down")]
    Down,
    #[strum(serialize = "up")]
    Up,
    #[strum(serialize = "left")]
    Left,
    #[strum(serialize = "right")]
    Right,
    #[strum(serialize = "page_up")]
    PageUp,
    #[strum(serialize = "page_down")]
    PageDown,
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

impl EditCommand {
    pub fn move_command(&self, count: Option<usize>) -> Option<Movement> {
        use EditCommand::*;
        match self {
            Left => Some(Movement::Left),
            Right => Some(Movement::Right),
            Up => Some(Movement::Up),
            Down => Some(Movement::Down),
            DocumentStart => Some(Movement::DocumentStart),
            DocumentEnd => Some(Movement::DocumentEnd),
            LineStart => Some(Movement::StartOfLine),
            LineStartNonBlank => Some(Movement::FirstNonBlank),
            LineEnd => Some(Movement::EndOfLine),
            GotoLineDefaultFirst => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::First),
            }),
            GotoLineDefaultLast => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::Last),
            }),
            WordBackward => Some(Movement::WordBackward),
            WordForward => Some(Movement::WordForward),
            WordEndForward => Some(Movement::WordEndForward),
            MatchPairs => Some(Movement::MatchPairs),
            NextUnmatchedRightBracket => Some(Movement::NextUnmatched(')')),
            PreviousUnmatchedLeftBracket => Some(Movement::PreviousUnmatched('(')),
            NextUnmatchedRightCurlyBracket => Some(Movement::NextUnmatched('}')),
            PreviousUnmatchedLeftCurlyBracket => {
                Some(Movement::PreviousUnmatched('{'))
            }
            _ => None,
        }
    }
}
