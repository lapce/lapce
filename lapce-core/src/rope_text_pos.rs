use floem_editor_core::buffer::rope_text::RopeText;
use lsp_types::Position;

use crate::encoding::{offset_utf8_to_utf16, offset_utf16_to_utf8};

pub trait RopeTextPosition: RopeText {
    /// Converts a UTF8 offset to a UTF16 LSP position
    /// Returns None if it is not a valid UTF16 offset
    fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset);
        let line_offset = self.offset_of_line(line);

        let utf16_col =
            offset_utf8_to_utf16(self.char_indices_iter(line_offset..), col);

        Position {
            line: line as u32,
            character: utf16_col as u32,
        }
    }

    fn offset_of_position(&self, pos: &Position) -> usize {
        let (line, column) = self.position_to_line_col(pos);

        self.offset_of_line_col(line, column)
    }

    fn position_to_line_col(&self, pos: &Position) -> (usize, usize) {
        let line = pos.line as usize;
        let line_offset = self.offset_of_line(line);

        let column = offset_utf16_to_utf8(
            self.char_indices_iter(line_offset..),
            pos.character as usize,
        );

        (line, column)
    }
}
impl<T: RopeText> RopeTextPosition for T {}
