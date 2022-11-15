use lapce_xi_rope::{Cursor, Rope, RopeInfo};

use crate::{
    mode::Mode,
    syntax::util::{matching_char, matching_pair_direction},
};

/// Describe char classifications used to compose word boundaries
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum CharClassification {
    /// Carriage Return (`r`)
    Cr,
    /// Line feed (`\n`)
    Lf,
    /// Whitespace character
    Space,
    /// Any punctuation character
    Punctuation,
    /// Includes letters and all of non-ascii unicode
    Other,
}

/// A word boundary can be the start of a word, its end or both for punctuation
#[derive(PartialEq, Eq)]
enum WordBoundary {
    /// Denote that this is not a boundary
    Interior,
    /// A boundary indicating the end of a word
    Start,
    /// A boundary indicating the start of a word
    End,
    /// Both start and end boundaries (ex: punctuation characters)
    Both,
}

impl WordBoundary {
    fn is_start(&self) -> bool {
        *self == WordBoundary::Start || *self == WordBoundary::Both
    }

    fn is_end(&self) -> bool {
        *self == WordBoundary::End || *self == WordBoundary::Both
    }

    #[allow(unused)]
    fn is_boundary(&self) -> bool {
        *self != WordBoundary::Interior
    }
}

/// A cursor providing utility function to navigate the rope
/// by word boundaries.
/// Boundaries can be the start of a word, its end, punctuation etc.
pub struct WordCursor<'a> {
    pub(crate) inner: Cursor<'a, RopeInfo>,
}

impl<'a> WordCursor<'a> {
    pub fn new(text: &'a Rope, pos: usize) -> WordCursor<'a> {
        let inner = Cursor::new(text, pos);
        WordCursor { inner }
    }

    /// Get the previous start boundary of a word, and set the cursor position to the boundary found.
    /// The behaviour diffs a bit on new line character with modal and non modal,
    /// while on modal, it will ignore the new line character and on non-modal,
    /// it will stop at the new line character
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_core::mode::Mode;
    /// # use lapce_xi_rope::Rope;
    /// let rope = Rope::from("Hello world");
    /// let mut cursor = WordCursor::new(&rope, 4);
    /// let boundary = cursor.prev_boundary(Mode::Insert);
    /// assert_eq!(boundary, Some(0));
    ///```
    pub fn prev_boundary(&mut self, mode: Mode) -> Option<usize> {
        if let Some(ch) = self.inner.prev_codepoint() {
            let mut prop = get_char_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(prev) = self.inner.prev_codepoint() {
                let prop_prev = get_char_property(prev);
                if classify_boundary(prop_prev, prop).is_start() {
                    break;
                }

                // Stop if line beginning reached, without any non-whitespace characters
                if mode == Mode::Insert
                    && prop_prev == CharClassification::Lf
                    && prop == CharClassification::Space
                {
                    break;
                }

                prop = prop_prev;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    /// Computes where the cursor position should be after backward deletion.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let text = "violet are blue";
    /// let rope = Rope::from(text);
    /// let mut cursor = WordCursor::new(&rope, 9);
    /// let position = cursor.prev_deletion_boundary();
    /// let position = position;
    ///
    /// assert_eq!(position, Some(7));
    /// assert_eq!(&text[..position.unwrap()], "violet ");
    ///```
    pub fn prev_deletion_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.prev_codepoint() {
            let mut prop = get_char_property(ch);
            let mut candidate = self.inner.pos();

            // Flag, determines if the word should be deleted or not
            // If not, erase only whitespace characters.
            let mut keep_word = false;
            while let Some(prev) = self.inner.prev_codepoint() {
                let prop_prev = get_char_property(prev);

                // Stop if line beginning reached, without any non-whitespace characters
                if prop_prev == CharClassification::Lf
                    && prop == CharClassification::Space
                {
                    break;
                }

                // More than a single whitespace: keep word, remove only whitespaces
                if prop == CharClassification::Space
                    && prop_prev == CharClassification::Space
                {
                    keep_word = true;
                }

                // Line break found: keep words, delete line break & trailing whitespaces
                if prop == CharClassification::Lf || prop == CharClassification::Cr {
                    keep_word = true;
                }

                // Skip word deletion if above conditions were met
                if keep_word
                    && (prop_prev == CharClassification::Punctuation
                        || prop_prev == CharClassification::Other)
                {
                    break;
                }

                // Default deletion
                if classify_boundary(prop_prev, prop).is_start() {
                    break;
                }
                prop = prop_prev;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    /// Get the position of the next non blank character in the rope
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let rope = Rope::from("    world");
    /// let mut cursor = WordCursor::new(&rope, 0);
    /// let char_position = cursor.next_non_blank_char();
    /// assert_eq!(char_position, 4);
    ///```
    pub fn next_non_blank_char(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(next) = self.inner.next_codepoint() {
            let prop = get_char_property(next);
            if prop != CharClassification::Space {
                break;
            }
            candidate = self.inner.pos();
        }
        self.inner.set(candidate);
        candidate
    }

    /// Get the next start boundary of a word, and set the cursor position to the boundary found.
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let rope = Rope::from("Hello world");
    /// let mut cursor = WordCursor::new(&rope, 0);
    /// let boundary = cursor.next_boundary();
    /// assert_eq!(boundary, Some(6));
    ///```
    pub fn next_boundary(&mut self) -> Option<usize> {
        if let Some(ch) = self.inner.next_codepoint() {
            let mut prop = get_char_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_char_property(next);
                if classify_boundary(prop, prop_next).is_start() {
                    break;
                }
                prop = prop_next;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    /// Get the next end boundary, and set the cursor position to the boundary found.
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let rope = Rope::from("Hello world");
    /// let mut cursor = WordCursor::new(&rope, 3);
    /// let end_boundary = cursor.end_boundary();
    /// assert_eq!(end_boundary, Some(5));
    ///```
    pub fn end_boundary(&mut self) -> Option<usize> {
        self.inner.next_codepoint();
        if let Some(ch) = self.inner.next_codepoint() {
            let mut prop = get_char_property(ch);
            let mut candidate = self.inner.pos();
            while let Some(next) = self.inner.next_codepoint() {
                let prop_next = get_char_property(next);
                if classify_boundary(prop, prop_next).is_end() {
                    break;
                }
                prop = prop_next;
                candidate = self.inner.pos();
            }
            self.inner.set(candidate);
            return Some(candidate);
        }
        None
    }

    /// Get the first matching [`CharClassification::Other`] backward and set the cursor position to this location .
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let text = "violet, are\n blue";
    /// let rope = Rope::from(text);
    /// let mut cursor = WordCursor::new(&rope, 11);
    /// let position = cursor.prev_code_boundary();
    /// assert_eq!(&text[position..], "are\n blue");
    ///```
    pub fn prev_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.prev_codepoint() {
            let prop_prev = get_char_property(prev);
            if prop_prev != CharClassification::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        candidate
    }

    /// Get the first matching [`CharClassification::Other`] forward and set the cursor position to this location .
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let text = "violet, are\n blue";
    /// let rope = Rope::from(text);
    /// let mut cursor = WordCursor::new(&rope, 11);
    /// let position = cursor.next_code_boundary();
    /// assert_eq!(&text[position..], "\n blue");
    ///```
    pub fn next_code_boundary(&mut self) -> usize {
        let mut candidate = self.inner.pos();
        while let Some(prev) = self.inner.next_codepoint() {
            let prop_prev = get_char_property(prev);
            if prop_prev != CharClassification::Other {
                break;
            }
            candidate = self.inner.pos();
        }
        candidate
    }

    /// Looks for a matching pair character, either forward for opening chars (ex: `(`) or
    /// backward for closing char (ex: `}`), and return the matched character position if found.
    /// Will return `None` if the character under cursor is not matchable (see [`crate::syntax::util::matching_char`]).
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let text = "{ }";
    /// let rope = Rope::from(text);
    /// let mut cursor = WordCursor::new(&rope, 2);
    /// let position = cursor.match_pairs();
    /// assert_eq!(position, Some(0));
    ///```
    pub fn match_pairs(&mut self) -> Option<usize> {
        let c = self.inner.peek_next_codepoint()?;
        let other = matching_char(c)?;
        let left = matching_pair_direction(other)?;
        if left {
            self.previous_unmatched(other)
        } else {
            self.inner.next_codepoint();
            let offset = self.next_unmatched(other)?;
            Some(offset - 1)
        }
    }

    /// Take a matchable character and look cforward for the first unmatched one
    /// ignoring the encountered matched pairs.
    ///
    /// **Example**:
    /// ```rust
    /// # use lapce_xi_rope::Rope;
    /// # use lapce_core::word::WordCursor;
    /// let rope = Rope::from("outer {inner}} world");
    /// let mut cursor = WordCursor::new(&rope, 0);
    /// let position = cursor.next_unmatched('}');
    /// assert_eq!(position, Some(14));
    ///  ```
    pub fn next_unmatched(&mut self, c: char) -> Option<usize> {
        let other = matching_char(c)?;
        let mut n = 0;
        while let Some(current) = self.inner.next_codepoint() {
            if current == c && n == 0 {
                return Some(self.inner.pos());
            }
            if current == other {
                n += 1;
            } else if current == c {
                n -= 1;
            }
        }
        None
    }

    /// Take a matchable character and look backward for the first unmatched one
    /// ignoring the encountered matched pairs.
    ///
    /// **Example**:
    ///
    /// ```rust
    /// # use lapce_xi_rope::Rope;
    /// # use lapce_core::word::WordCursor;
    /// let rope = Rope::from("outer {{inner} world");
    /// let mut cursor = WordCursor::new(&rope, 15);
    /// let position = cursor.previous_unmatched('{');
    /// assert_eq!(position, Some(6));
    ///  ```
    pub fn previous_unmatched(&mut self, c: char) -> Option<usize> {
        let other = matching_char(c)?;
        let mut n = 0;
        while let Some(current) = self.inner.prev_codepoint() {
            if current == c && n == 0 {
                return Some(self.inner.pos());
            }
            if current == other {
                n += 1;
            } else if current == c {
                n -= 1;
            }
        }
        None
    }

    /// Return the previous and end boundaries of the word under cursor.
    ///
    /// **Example**:
    ///
    ///```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let text = "violet are blue";
    /// let rope = Rope::from(text);
    /// let mut cursor = WordCursor::new(&rope, 9);
    /// let (start, end) = cursor.select_word();
    /// assert_eq!(&text[start..end], "are");
    ///```
    pub fn select_word(&mut self) -> (usize, usize) {
        let initial = self.inner.pos();
        let end = self.next_code_boundary();
        self.inner.set(initial);
        let start = self.prev_code_boundary();
        (start, end)
    }

    /// Return the enclosing brackets of the current position
    ///
    /// **Example**:
    ///
    ///```rust
    /// # use lapce_core::word::WordCursor;
    /// # use lapce_xi_rope::Rope;
    /// let text = "outer {{inner} world";
    /// let rope = Rope::from(text);
    /// let mut cursor = WordCursor::new(&rope, 10);
    /// let (start, end) = cursor.find_enclosing_pair().unwrap();
    /// assert_eq!(start, 7);
    /// assert_eq!(end, 13)
    ///```
    pub fn find_enclosing_pair(&mut self) -> Option<(usize, usize)> {
        let old_offset = self.inner.pos();
        while let Some(c) = self.inner.prev_codepoint() {
            if matching_pair_direction(c) == Some(true) {
                let opening_bracket_offset = self.inner.pos();
                if let Some(closing_bracket_offset) = self.match_pairs() {
                    if (opening_bracket_offset..closing_bracket_offset)
                        .contains(&old_offset)
                    {
                        return Some((
                            opening_bracket_offset,
                            closing_bracket_offset,
                        ));
                    } else {
                        self.inner.set(opening_bracket_offset);
                    }
                }
            }
        }
        None
    }
}

/// Return the [`CharClassification`] of the input character
pub fn get_char_property(codepoint: char) -> CharClassification {
    if codepoint <= ' ' {
        if codepoint == '\r' {
            return CharClassification::Cr;
        }
        if codepoint == '\n' {
            return CharClassification::Lf;
        }
        return CharClassification::Space;
    } else if codepoint <= '\u{3f}' {
        if (0xfc00fffe00000000u64 >> (codepoint as u32)) & 1 != 0 {
            return CharClassification::Punctuation;
        }
    } else if codepoint <= '\u{7f}' {
        // Hardcoded: @[\]^`{|}~
        if (0x7800000178000001u64 >> ((codepoint as u32) & 0x3f)) & 1 != 0 {
            return CharClassification::Punctuation;
        }
    }
    CharClassification::Other
}

fn classify_boundary(
    prev: CharClassification,
    next: CharClassification,
) -> WordBoundary {
    use self::{CharClassification::*, WordBoundary::*};
    match (prev, next) {
        (Lf, Lf) => Start,
        (Lf, Space) => Interior,
        (Cr, Lf) => Interior,
        (Space, Lf) => Interior,
        (Space, Cr) => Interior,
        (Space, Space) => Interior,
        (_, Space) => End,
        (Space, _) => Start,
        (Lf, _) => Start,
        (_, Cr) => End,
        (_, Lf) => End,
        (Punctuation, Other) => Both,
        (Other, Punctuation) => Both,
        _ => Interior,
    }
}

#[cfg(test)]
mod test {
    use lapce_xi_rope::Rope;

    use super::WordCursor;
    use crate::mode::Mode;

    #[test]
    fn prev_boundary_should_be_none_at_position_zero() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 0);
        let boudary = cursor.prev_boundary(Mode::Insert);
        assert!(boudary.is_none())
    }

    #[test]
    fn prev_boundary_should_be_zero_when_cursor_on_first_word() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 4);
        let boundary = cursor.prev_boundary(Mode::Insert);
        assert_eq!(boundary, Some(0));
    }

    #[test]
    fn prev_boundary_should_be_at_word_start() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 9);
        let boundary = cursor.prev_boundary(Mode::Insert);
        assert_eq!(boundary, Some(6));
    }

    #[test]
    fn on_whitespace_prev_boundary_should_be_at_line_start_for_non_modal() {
        let rope = Rope::from("Hello\n    world");
        let mut cursor = WordCursor::new(&rope, 10);
        let boundary = cursor.prev_boundary(Mode::Insert);
        assert_eq!(boundary, Some(6));
    }

    #[test]
    fn on_whitespace_prev_boundary_should_cross_line_for_modal() {
        let rope = Rope::from("Hello\n    world");
        let mut cursor = WordCursor::new(&rope, 10);
        let boundary = cursor.prev_boundary(Mode::Normal);
        assert_eq!(boundary, Some(0));
    }

    #[test]
    fn should_get_next_word_boundary() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 0);
        let boundary = cursor.next_boundary();
        assert_eq!(boundary, Some(6));
    }

    #[test]
    fn next_word_boundary_should_be_none_at_last_position() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 11);
        let boundary = cursor.next_boundary();
        assert_eq!(boundary, None);
    }

    #[test]
    fn should_get_previous_code_boundary() {
        let text = "violet, are\n blue";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 11);
        let position = cursor.prev_code_boundary();
        assert_eq!(&text[position..], "are\n blue");
    }

    #[test]
    fn should_get_next_code_boundary() {
        let text = "violet, are\n blue";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 11);
        let position = cursor.next_code_boundary();
        assert_eq!(&text[position..], "\n blue");
    }

    #[test]
    fn get_next_non_blank_char_should_skip_whitespace() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 5);
        let char_position = cursor.next_non_blank_char();
        assert_eq!(char_position, 6);
    }

    #[test]
    fn get_next_non_blank_char_should_return_current_position_on_non_blank_char() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 3);
        let char_position = cursor.next_non_blank_char();
        assert_eq!(char_position, 3);
    }

    #[test]
    fn should_get_end_boundary() {
        let rope = Rope::from("Hello world");
        let mut cursor = WordCursor::new(&rope, 3);
        let end_boundary = cursor.end_boundary();
        assert_eq!(end_boundary, Some(5));
    }

    #[test]
    fn should_get_next_unmatched_char() {
        let rope = Rope::from("hello { world");
        let mut cursor = WordCursor::new(&rope, 0);
        let position = cursor.next_unmatched('{');
        assert_eq!(position, Some(7));
    }

    #[test]
    fn should_get_next_unmatched_char_witch_matched_chars() {
        let rope = Rope::from("hello {} world }");
        let mut cursor = WordCursor::new(&rope, 0);
        let position = cursor.next_unmatched('}');
        assert_eq!(position, Some(16));
    }

    #[test]
    fn should_get_previous_unmatched_char() {
        let rope = Rope::from("hello { world");
        let mut cursor = WordCursor::new(&rope, 12);
        let position = cursor.previous_unmatched('{');
        assert_eq!(position, Some(6));
    }

    #[test]
    fn should_get_previous_unmatched_char_with_inner_matched_chars() {
        let rope = Rope::from("{hello {} world");
        let mut cursor = WordCursor::new(&rope, 10);
        let position = cursor.previous_unmatched('{');
        assert_eq!(position, Some(0));
    }

    #[test]
    fn should_match_pair_forward() {
        let text = "{ }";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 0);
        let position = cursor.match_pairs();
        assert_eq!(position, Some(2));
    }

    #[test]
    fn should_match_pair_backward() {
        let text = "{ }";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 2);
        let position = cursor.match_pairs();
        assert_eq!(position, Some(0));
    }

    #[test]
    fn match_pair_should_be_none() {
        let text = "{ }";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 1);
        let position = cursor.match_pairs();
        assert_eq!(position, None);
    }

    #[test]
    fn select_word_should_return_word_boundaries() {
        let text = "violet are blue";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 9);
        let (start, end) = cursor.select_word();
        assert_eq!(&text[start..end], "are");
    }

    #[test]
    fn should_get_deletion_boundary_backward() {
        let text = "violet are blue";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 9);
        let position = cursor.prev_deletion_boundary();
        let position = position;

        assert_eq!(position, Some(7));
        assert_eq!(&text[..position.unwrap()], "violet ");
    }

    #[test]
    fn find_pair_should_return_positions() {
        let text = "violet (are) blue";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 9);
        let positions = cursor.find_enclosing_pair();
        assert_eq!(positions, Some((7, 11)));
    }

    #[test]
    fn find_pair_should_return_next_pair() {
        let text = "violets {are (blue)    }";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 11);
        let positions = cursor.find_enclosing_pair();
        assert_eq!(positions, Some((8, 23)));

        let mut cursor = WordCursor::new(&rope, 20);
        let positions = cursor.find_enclosing_pair();
        assert_eq!(positions, Some((8, 23)))
    }

    #[test]
    fn find_pair_should_return_none() {
        let text = "violet (are) blue";
        let rope = Rope::from(text);
        let mut cursor = WordCursor::new(&rope, 1);
        let positions = cursor.find_enclosing_pair();
        assert_eq!(positions, None);
    }
}
