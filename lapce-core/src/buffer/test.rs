use xi_rope::Rope;

use crate::{editor::EditType, selection::Selection};

use super::Buffer;

#[test]
fn test_is_pristine() {
    let mut buffer = Buffer::new("");
    buffer.init_content(Rope::from("abc"));
    buffer.edit(&[(Selection::caret(0), "d")], EditType::InsertChars);
    buffer.edit(&[(Selection::caret(0), "e")], EditType::InsertChars);
    buffer.do_undo();
    buffer.do_undo();
    assert!(buffer.is_pristine());
}

#[test]
fn test_move_by_word() {
    let mut buffer = Buffer::new("");
    // Cannot move anywhere in an empty buffer.
    assert_eq!(buffer.move_word_forward(0), 0);
    assert_eq!(buffer.move_n_words_forward(0, 0), 0);
    assert_eq!(buffer.move_n_words_forward(0, 2), 0);
    assert_eq!(buffer.move_word_backward(0), 0);
    assert_eq!(buffer.move_n_words_backward(0, 2), 0);
    assert_eq!(buffer.move_n_words_backward(0, 0), 0);

    // Add some content to the buffer.
    buffer.init_content(Rope::from("one two three  four  "));
    //                            ->012345678901234567890<-

    // 0 count don't move at all.
    assert_eq!(buffer.move_n_words_forward(0, 0), 0);
    assert_eq!(buffer.move_n_words_backward(0, 0), 0);

    for i in 0..4 {
        assert_eq!(buffer.move_word_forward(i), 4);
        assert_eq!(buffer.move_word_backward(i), 0);
    }

    assert_eq!(buffer.move_word_forward(4), 8);
    assert_eq!(buffer.move_word_backward(4), 0);

    let end = buffer.len() - 1;
    for i in 0..4 {
        assert_eq!(buffer.move_n_words_forward(i, 2), 8);
    }
    assert_eq!(buffer.move_n_words_forward(4, 2), 15);
    for i in 0..5 {
        assert_eq!(buffer.move_n_words_backward(end - i, 2), 8)
    }
    assert_eq!(buffer.move_n_words_backward(end - 6, 2), 4);

    assert_eq!(buffer.move_n_words_forward(0, 2), 8);
    assert_eq!(buffer.move_n_words_forward(0, 3), 15);
    assert_eq!(buffer.move_n_words_backward(end, 2), 8);
    assert_eq!(buffer.move_n_words_backward(end, 3), 4);

    // FIXME: see #501 for possible issues in WordCursor::next_boundary()
    //
    // Trying to move beyond the buffer end.  The cursor will stay there.
    for i in 0..end {
        assert_eq!(buffer.move_n_words_forward(i, 100), end + 1);
    }

    // In the other direction.
    for i in 0..end {
        assert_eq!(buffer.move_n_words_backward(end - i, 100), 0);
    }
}

#[test]
fn test_move_n_wordends_forward() {
    fn v(buf: &Buffer, off: usize, n: usize, ins: bool, end: usize) {
        assert_eq!(buf.move_n_wordends_forward(off, n, ins), end);
    }

    let buffer = Buffer::new("");
    // Cannot move anywhere in an empty buffer.
    v(&buffer, 0, 1, false, 0);
    v(&buffer, 0, 1, true, 0);

    let buffer = Buffer::new("one two three  four  ");
    //                      ->012345678901234567890<-

    v(&buffer, 0, 0, false, 0);
    v(&buffer, 0, 0, true, 0);

    for i in 0..2 {
        v(&buffer, i, 1, false, 2);
        v(&buffer, i, 2, false, 6);
        v(&buffer, i, 3, false, 12);

        v(&buffer, i, 10, false, 21);

        // In Mode::Insert, returns 1 pass the word end.
        v(&buffer, i, 1, true, 3);
        v(&buffer, i, 2, true, 7);
        v(&buffer, i, 3, true, 13);

        v(&buffer, i, 10, true, 21);
    }

    v(&buffer, 2, 1, false, 6);
    v(&buffer, 2, 2, false, 12);
    v(&buffer, 2, 3, false, 18);
    v(&buffer, 2, 10, false, 21);

    v(&buffer, 2, 1, true, 7);
    v(&buffer, 2, 2, true, 13);
    v(&buffer, 2, 3, true, 19);
    v(&buffer, 2, 10, true, 21);

    let buffer = Buffer::new("one\n\ntwo\n\n\nthree\n\n\n");
    //                      ->0123 4 5678 9 0 123456 7 8 <-

    v(&buffer, 0, 2, false, 7);
    v(&buffer, 0, 3, false, 15);
    v(&buffer, 0, 4, false, 19);
}
