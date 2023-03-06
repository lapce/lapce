use lapce_xi_rope::{Cursor, Rope, RopeInfo};

/// Describe char classifications used to compose word boundaries
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum CharClassification {
    /// Carriage Return (`r`)
    Cr,
    /// Line feed (`\n`)
    Lf,
    /// Includes letters and all of non-ascii unicode
    Other,
}

/// A word boundary can be the start of a word, its end or both for punctuation
#[derive(PartialEq, Eq)]
enum ParagraphBoundary {
    /// Denote that this is not a boundary
    Interior,
    /// A boundary indicating the end of a new-line sequence
    Start,
    /// A boundary indicating the start of a new-line sequence
    End,
    /// Both start and end boundaries (when we have only one empty
    /// line)
    Both,
}

impl ParagraphBoundary {
    fn is_start(&self) -> bool {
        *self == ParagraphBoundary::Start || *self == ParagraphBoundary::Both
    }

    fn is_end(&self) -> bool {
        *self == ParagraphBoundary::End || *self == ParagraphBoundary::Both
    }

    #[allow(unused)]
    fn is_boundary(&self) -> bool {
        *self != ParagraphBoundary::Interior
    }
}

/// A cursor providing utility function to navigate the rope
/// by parahraphs boundaries.
/// Boundaries can be the start of a word, its end, punctuation etc.
pub struct ParagraphCursor<'a> {
    pub(crate) inner: Cursor<'a, RopeInfo>,
}

impl<'a> ParagraphCursor<'a> {
    pub fn new(text: &'a Rope, pos: usize) -> ParagraphCursor<'a> {
        let inner = Cursor::new(text, pos);
        ParagraphCursor { inner }
    }

    pub fn prev_boundary(&mut self) -> Option<usize> {
        if let (Some(ch1), Some(ch2), Some(ch3)) = (
            self.inner.prev_codepoint(),
            self.inner.prev_codepoint(),
            self.inner.prev_codepoint(),
        ) {
            let mut prop1 = get_char_property(ch1);
            let mut prop2 = get_char_property(ch2);
            let mut prop3 = get_char_property(ch3);
            let mut candidate = self.inner.pos();

            while let Some(prev) = self.inner.prev_codepoint() {
                let prop_prev = get_char_property(prev);
                if classify_boundary(prop_prev, prop3, prop2, prop1).is_start() {
                    break;
                }
                (prop3, prop2, prop1) = (prop_prev, prop3, prop2);
                candidate = self.inner.pos();
            }

            self.inner.set(candidate + 1);
            return Some(candidate + 1);
        }

        None
    }

    pub fn next_boundary(&mut self) -> Option<usize> {
        if let (Some(ch1), Some(ch2), Some(ch3)) = (
            self.inner.next_codepoint(),
            self.inner.next_codepoint(),
            self.inner.next_codepoint(),
        ) {
            let mut prop1 = get_char_property(ch1);
            let mut prop2 = get_char_property(ch2);
            let mut prop3 = get_char_property(ch3);
            let mut candidate = self.inner.pos();

            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_char_property(next);
                if classify_boundary(prop1, prop2, prop3, prop_next).is_end() {
                    break;
                }

                (prop1, prop2, prop3) = (prop2, prop3, prop_next);
                candidate = self.inner.pos();
            }
            self.inner.set(candidate - 1);
            return Some(candidate - 1);
        }
        None
    }
}

/// Return the [`CharClassification`] of the input character
pub fn get_char_property(codepoint: char) -> CharClassification {
    match codepoint {
        '\r' => CharClassification::Cr,
        '\n' => CharClassification::Lf,
        _ => CharClassification::Other,
    }
}

fn classify_boundary(
    before_prev: CharClassification,
    prev: CharClassification,
    next: CharClassification,
    after_next: CharClassification,
) -> ParagraphBoundary {
    use self::{CharClassification::*, ParagraphBoundary::*};

    match (before_prev, prev, next, after_next) {
        (Other, Lf, Lf, Other) => Both,
        (_, Lf, Lf, Other) => Start,
        (Lf, Cr, Lf, Other) => Start,
        (Other, Lf, Lf, _) => End,
        (Other, Cr, Lf, Cr) => End,
        _ => Interior,
    }
}
