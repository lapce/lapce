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
pub fn snap_to_soft_tab(
    buffer: &Buffer,
    offset: usize,
    direction: SnapDirection,
    tab_width: usize,
) -> usize {
    // Fine which line we're on.
    let line = buffer.line_of_offset(offset);
    // Get the offset to the start of the line.
    let start_line_offset = buffer.offset_of_line(line);
    // And the offset within the lint.
    let offset_within_line = offset - start_line_offset;

    start_line_offset
        + snap_to_soft_tab_logic(
            buffer,
            offset_within_line,
            start_line_offset,
            direction,
            tab_width,
        )
}

/// If the cursor is inside a soft tab at the start of the line, snap it to the
/// nearest, left or right edge. This version takes a line/column and returns a column.
pub fn snap_to_soft_tab_line_col(
    buffer: &Buffer,
    line: usize,
    col: usize,
    direction: SnapDirection,
    tab_width: usize,
) -> usize {
    // Get the offset to the start of the line.
    let start_line_offset = buffer.offset_of_line(line);

    snap_to_soft_tab_logic(buffer, col, start_line_offset, direction, tab_width)
}

/// Internal shared logic that performs the actual snapping. It can be passed
/// either an column or offset within the line since it is only modified when it makes no
/// difference which is used (since they're equal for spaces).
/// It returns the column or offset within the line (depending on what you passed in).
fn snap_to_soft_tab_logic(
    buffer: &Buffer,
    offset_or_col: usize,
    start_line_offset: usize,
    direction: SnapDirection,
    tab_width: usize,
) -> usize {
    assert!(tab_width >= 1);

    // Number of spaces, ignoring incomplete soft tabs.
    let space_count =
        (count_spaces_from(buffer, start_line_offset) / tab_width) * tab_width;

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
    let mut cursor = lapce_xi_rope::Cursor::new(buffer.text(), from_offset);
    let mut space_count = 0usize;
    while let Some(next) = cursor.next_codepoint() {
        if next != ' ' {
            break;
        }
        space_count += 1;
    }
    space_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_spaces_from() {
        let buffer = Buffer::new("     abc\n   def\nghi\n");
        assert_eq!(count_spaces_from(&buffer, 0), 5);
        assert_eq!(count_spaces_from(&buffer, 1), 4);
        assert_eq!(count_spaces_from(&buffer, 5), 0);
        assert_eq!(count_spaces_from(&buffer, 6), 0);

        assert_eq!(count_spaces_from(&buffer, 8), 0);
        assert_eq!(count_spaces_from(&buffer, 9), 3);
        assert_eq!(count_spaces_from(&buffer, 10), 2);

        assert_eq!(count_spaces_from(&buffer, 16), 0);
        assert_eq!(count_spaces_from(&buffer, 17), 0);
    }

    #[test]
    fn test_snap_to_soft_tab() {
        let buffer =
            Buffer::new("          abc\n      def\n    ghi\nklm\n        opq");

        let tab_width = 4;

        // Input offset, and output offset for Left, Nearest and Right respectively.
        let test_cases = [
            (0, 0, 0, 0),
            (1, 0, 0, 4),
            (2, 0, 4, 4),
            (3, 0, 4, 4),
            (4, 4, 4, 4),
            (5, 4, 4, 8),
            (6, 4, 8, 8),
            (7, 4, 8, 8),
            (8, 8, 8, 8),
            (9, 9, 9, 9),
            (10, 10, 10, 10),
            (11, 11, 11, 11),
            (12, 12, 12, 12),
            (13, 13, 13, 13),
            (14, 14, 14, 14),
            (15, 14, 14, 18),
            (16, 14, 18, 18),
            (17, 14, 18, 18),
            (18, 18, 18, 18),
            (19, 19, 19, 19),
            (20, 20, 20, 20),
            (21, 21, 21, 21),
        ];

        for test_case in test_cases {
            assert_eq!(
                snap_to_soft_tab(
                    &buffer,
                    test_case.0,
                    SnapDirection::Left,
                    tab_width
                ),
                test_case.1
            );
            assert_eq!(
                snap_to_soft_tab(
                    &buffer,
                    test_case.0,
                    SnapDirection::Nearest,
                    tab_width
                ),
                test_case.2
            );
            assert_eq!(
                snap_to_soft_tab(
                    &buffer,
                    test_case.0,
                    SnapDirection::Right,
                    tab_width
                ),
                test_case.3
            );
        }
    }

    #[test]
    fn test_snap_to_soft_tab_line_col() {
        let buffer =
            Buffer::new("          abc\n      def\n    ghi\nklm\n        opq");

        let tab_width = 4;

        // Input line, column, and output column for Left, Nearest and Right respectively.
        let test_cases = [
            (0, 0, 0, 0, 0),
            (0, 1, 0, 0, 4),
            (0, 2, 0, 4, 4),
            (0, 3, 0, 4, 4),
            (0, 4, 4, 4, 4),
            (0, 5, 4, 4, 8),
            (0, 6, 4, 8, 8),
            (0, 7, 4, 8, 8),
            (0, 8, 8, 8, 8),
            (0, 9, 9, 9, 9),
            (0, 10, 10, 10, 10),
            (0, 11, 11, 11, 11),
            (0, 12, 12, 12, 12),
            (0, 13, 13, 13, 13),
            (1, 0, 0, 0, 0),
            (1, 1, 0, 0, 4),
            (1, 2, 0, 4, 4),
            (1, 3, 0, 4, 4),
            (1, 4, 4, 4, 4),
            (1, 5, 5, 5, 5),
            (1, 6, 6, 6, 6),
            (1, 7, 7, 7, 7),
            (4, 0, 0, 0, 0),
            (4, 1, 0, 0, 4),
            (4, 2, 0, 4, 4),
            (4, 3, 0, 4, 4),
            (4, 4, 4, 4, 4),
            (4, 5, 4, 4, 8),
            (4, 6, 4, 8, 8),
            (4, 7, 4, 8, 8),
            (4, 8, 8, 8, 8),
            (4, 9, 9, 9, 9),
        ];

        for test_case in test_cases {
            assert_eq!(
                snap_to_soft_tab_line_col(
                    &buffer,
                    test_case.0,
                    test_case.1,
                    SnapDirection::Left,
                    tab_width
                ),
                test_case.2
            );
            assert_eq!(
                snap_to_soft_tab_line_col(
                    &buffer,
                    test_case.0,
                    test_case.1,
                    SnapDirection::Nearest,
                    tab_width
                ),
                test_case.3
            );
            assert_eq!(
                snap_to_soft_tab_line_col(
                    &buffer,
                    test_case.0,
                    test_case.1,
                    SnapDirection::Right,
                    tab_width
                ),
                test_case.4
            );
        }
    }
}
