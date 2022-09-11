use std::{borrow::Cow, ops::Range};

use lsp_types::Position;
use xi_rope::{interval::IntervalBounds, Cursor, Rope};

use crate::encoding::{offset_utf16_to_utf8, offset_utf8_to_utf16};

pub struct RopeText<'a> {
    text: &'a Rope,
}

impl<'a> RopeText<'a> {
    pub fn new(text: &'a Rope) -> Self {
        Self { text }
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.len())
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = line.min(last_line + 1);
        self.text.offset_of_line(line)
    }

    pub fn offset_line_end(&self, offset: usize, caret: bool) -> usize {
        let line = self.line_of_offset(offset);
        self.line_end_offset(line, caret)
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        let offset = offset.min(self.len());
        let offset = self
            .text
            .at_or_prev_codepoint_boundary(offset)
            .unwrap_or(offset);

        self.text.line_of_offset(offset)
    }

    /// Converts a UTF8 offset to a UTF16 LSP position  
    /// Returns None if it is not a valid UTF16 offset
    pub fn offset_to_position(&self, offset: usize) -> Option<Position> {
        let (line, col) = self.offset_to_line_col(offset);
        let line_offset = self.offset_of_line(line);

        let utf16_col =
            offset_utf8_to_utf16(self.char_indices_iter(line_offset..), col)?;

        Some(Position {
            line: line as u32,
            character: utf16_col as u32,
        })
    }

    /// Returns None if the UTF16 Position can't be converted to a UTF8 offset
    pub fn offset_of_position(&self, pos: &Position) -> Option<usize> {
        let (line, column) = self.position_to_line_col(pos);

        column.map(|column| self.offset_of_line_col(line, column))
    }

    /// Returns None if the UTF16 Position can't be converted to a UTF8 offset
    pub fn position_to_line_col(&self, pos: &Position) -> (usize, Option<usize>) {
        let line = pos.line as usize;
        let line_offset = self.offset_of_line(line);

        let column = offset_utf16_to_utf8(
            self.char_indices_iter(line_offset..),
            pos.character as usize,
        );

        (line, column)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.len());
        let line = self.line_of_offset(offset);
        let line_start = self.offset_of_line(line);
        if offset == line_start {
            return (line, 0);
        }

        let col = offset - line_start;
        (line, col)
    }

    pub fn offset_of_line_col(&self, line: usize, col: usize) -> usize {
        let mut pos = 0;
        let mut offset = self.offset_of_line(line);
        for c in self
            .slice_to_cow(offset..self.offset_of_line(line + 1))
            .chars()
        {
            if c == '\n' {
                return offset;
            }

            let char_len = c.len_utf8();
            if pos + char_len > col {
                return offset;
            }
            pos += char_len;
            offset += char_len;
        }
        offset
    }

    pub fn line_end_col(&self, line: usize, caret: bool) -> usize {
        let line_start = self.offset_of_line(line);
        let offset = self.line_end_offset(line, caret);
        offset - line_start
    }

    pub fn line_end_offset(&self, line: usize, caret: bool) -> usize {
        let mut offset = self.offset_of_line(line + 1);
        let mut line_content: &str = &self.line_content(line);
        if line_content.ends_with("\r\n") {
            offset -= 2;
            line_content = &line_content[..line_content.len() - 2];
        } else if line_content.ends_with('\n') {
            offset -= 1;
            line_content = &line_content[..line_content.len() - 1];
        }
        if !caret && !line_content.is_empty() {
            offset = self.prev_grapheme_offset(offset, 1, 0);
        }
        offset
    }

    pub fn line_content(&self, line: usize) -> Cow<'a, str> {
        self.text
            .slice_to_cow(self.offset_of_line(line)..self.offset_of_line(line + 1))
    }

    pub fn prev_grapheme_offset(
        &self,
        offset: usize,
        count: usize,
        limit: usize,
    ) -> usize {
        let mut cursor = Cursor::new(&self.text, offset);
        let mut new_offset = offset;
        for _i in 0..count {
            if let Some(prev_offset) = cursor.prev_grapheme() {
                if prev_offset < limit {
                    return new_offset;
                }
                new_offset = prev_offset;
                cursor.set(prev_offset);
            } else {
                return new_offset;
            }
        }
        new_offset
    }

    pub fn slice_to_cow(&self, range: Range<usize>) -> Cow<'a, str> {
        self.text
            .slice_to_cow(range.start.min(self.len())..range.end.min(self.len()))
    }

    /// Iterate over (utf8_offset, char) values in the given range  
    /// This uses `iter_chunks` and so does not allocate, compared to `slice_to_cow` which can
    pub fn char_indices_iter<T: IntervalBounds>(
        &self,
        range: T,
    ) -> impl Iterator<Item = (usize, char)> + 'a {
        CharIndicesJoin::new(self.text.iter_chunks(range).map(str::char_indices))
    }

    pub fn num_lines(&self) -> usize {
        self.last_line() + 1
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
    }
}

/// Joins an iterator of iterators over char indices `(usize, char)` into one
/// as if they were from a single long string
/// Assumes the iterators end after the first `None` value
#[derive(Clone)]
pub struct CharIndicesJoin<I: Iterator<Item = (usize, char)>, O: Iterator<Item = I>>
{
    /// Our iterator of iterators
    main_iter: O,
    /// Our current working iterator of indices
    current_indices: Option<I>,
    /// The amount we should shift future offsets
    current_base: usize,
    /// The latest base, since we don't know when the `current_indices` iterator will end
    latest_base: usize,
}

impl<I: Iterator<Item = (usize, char)>, O: Iterator<Item = I>>
    CharIndicesJoin<I, O>
{
    pub fn new(main_iter: O) -> CharIndicesJoin<I, O> {
        CharIndicesJoin {
            main_iter,
            current_indices: None,
            current_base: 0,
            latest_base: 0,
        }
    }
}

impl<I: Iterator<Item = (usize, char)>, O: Iterator<Item = I>> Iterator
    for CharIndicesJoin<I, O>
{
    type Item = (usize, char);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = &mut self.current_indices {
            if let Some((next_offset, next_ch)) = current.next() {
                // Shift by the current base offset, which is the accumulated offset from previous
                // iterators, which makes so the offset produced looks like it is from one long str
                let next_offset = self.current_base + next_offset;
                // Store the latest base offset, because we don't know when the current iterator
                // will end (though technically the str iterator impl does)
                self.latest_base = next_offset + next_ch.len_utf8();
                return Some((next_offset, next_ch));
            }
        }

        // Otherwise, if we didn't return something above, then we get a next iterator
        if let Some(next_current) = self.main_iter.next() {
            // Update our current working iterator
            self.current_indices = Some(next_current);
            // Update the current base offset with the previous iterators latest offset base
            // This is what we are shifting by
            self.current_base = self.latest_base;

            // Get the next item without new current iterator
            // As long as main_iter and the iterators it produces aren't infinite then this
            // recursion won't be infinite either
            // and even the non-recursion version would be infinite if those were infinite
            self.next()
        } else {
            // We didn't get anything from the main iter, so we're completely done.
            None
        }
    }
}
