use lapce_core::buffer::Buffer;

/// The direction to snap. Left is used when moving left, Right when moving right.
/// Nearest is used for mouse selection.
pub enum SnapDirection {
    Left,
    Right,
    Nearest,
}

/// If the cursor is inside a soft tab at the start of the line, snap it to the
/// nearest, left or right edge. This version takes an offset and returns an offset.
pub fn snap_to_soft_tab(buffer: &Buffer, offset: usize, direction: SnapDirection, tab_width: usize) -> usize {
    // Fine which line we're on.
    let line = buffer.line_of_offset(offset);
    // Get the offset to the start of the line.
    let start_line_offset = buffer.offset_of_line(line);
    // And the offset within the lint.
    let offset_within_line = offset - start_line_offset;

    start_line_offset + snap_to_soft_tab_logic(buffer, offset_within_line, start_line_offset, direction, tab_width)
}

/// If the cursor is inside a soft tab at the start of the line, snap it to the
/// nearest, left or right edge. This version takes a line/column and returns a column.
pub fn snap_to_soft_tab_line_col(buffer: &Buffer, line: usize, col: usize, direction: SnapDirection, tab_width: usize) -> usize {
    // Get the offset to the start of the line.
    let start_line_offset = buffer.offset_of_line(line);

    snap_to_soft_tab_logic(buffer, col, start_line_offset, direction, tab_width)
}

/// Internal shared logic that performs the actual snapping. It can be passed
/// either an column or offset within the line since it is only modified when it makes no
/// difference which is used (since they're equal for spaces).
/// It returns the column or offset within the line (depending on what you passed in).
fn snap_to_soft_tab_logic(buffer: &Buffer, offset_or_col: usize, start_line_offset: usize, direction: SnapDirection, tab_width: usize) -> usize {
    assert!(tab_width >= 1);

    // Number of spaces, ignoring incomplete soft tabs.
    let space_count = (count_spaces_from(buffer, start_line_offset) / tab_width) * tab_width;

    // If we're past the soft tabs, we don't need to snap.
    if offset_or_col >= space_count {
        return offset_or_col;
    }

    let bias = match direction {
        SnapDirection::Left => 0,
        SnapDirection::Right => tab_width - 1,
        SnapDirection::Nearest => tab_width / 2,
    };

    ((offset_or_col + bias) / tab_width) * tab_width
}

/// Count the number of spaces found after a certain offset.
fn count_spaces_from(buffer: &Buffer, from_offset: usize) -> usize {
    let mut cursor = xi_rope::Cursor::new(buffer.text(), from_offset);
    let mut space_count = 0usize;
    while let Some(next) = cursor.next_codepoint() {
        if next != ' ' {
            break;
        }
        space_count += 1;
    }
    return space_count;
}
