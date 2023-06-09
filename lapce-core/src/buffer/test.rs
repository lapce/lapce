use super::{Buffer, RopeText};

mod editing {
    use lapce_xi_rope::Rope;

    use super::*;
    use crate::{editor::EditType, selection::Selection};

    #[test]
    fn is_pristine() {
        let mut buffer = Buffer::new("");
        buffer.init_content(Rope::from("abc"));
        buffer.edit(&[(Selection::caret(0), "d")], EditType::InsertChars);
        buffer.edit(&[(Selection::caret(0), "e")], EditType::InsertChars);
        buffer.do_undo();
        buffer.do_undo();
        assert!(buffer.is_pristine());
    }
}

mod motion {
    use super::*;
    use crate::mode::Mode;

    #[test]
    fn cannot_move_in_empty_buffer() {
        let buffer = Buffer::new("");
        assert_eq!(buffer.move_word_forward(0), 0);
        assert_eq!(buffer.move_n_words_forward(0, 2), 0);

        assert_eq!(buffer.move_word_backward(0, Mode::Insert), 0);
        assert_eq!(buffer.move_n_words_backward(0, 2, Mode::Insert), 0);

        assert_eq!(buffer.move_n_wordends_forward(0, 2, false), 0);
        assert_eq!(buffer.move_n_wordends_forward(0, 2, true), 0);
    }

    #[test]
    fn on_word_boundary_in_either_direction() {
        let buffer = Buffer::new("one two three  four  ");
        //                      ->012345678901234567890<-

        // 0 count does not move.
        assert_eq!(buffer.move_n_words_forward(0, 0), 0);
        assert_eq!(buffer.move_n_words_backward(0, 0, Mode::Insert), 0);

        for offset in 0..4 {
            assert_eq!(buffer.move_word_forward(offset), 4);
            assert_eq!(buffer.move_word_backward(offset, Mode::Insert), 0);
        }

        assert_eq!(buffer.move_word_forward(4), 8);
        assert_eq!(buffer.move_word_backward(4, Mode::Insert), 0);

        let end = buffer.len() - 1;
        for offset in 0..4 {
            assert_eq!(buffer.move_n_words_forward(offset, 2), 8);
        }
        assert_eq!(buffer.move_n_words_forward(4, 2), 15);
        for offset in 0..5 {
            assert_eq!(
                buffer.move_n_words_backward(end - offset, 2, Mode::Insert),
                8
            )
        }
        assert_eq!(buffer.move_n_words_backward(end - 6, 2, Mode::Insert), 4);

        assert_eq!(buffer.move_n_words_forward(0, 2), 8);
        assert_eq!(buffer.move_n_words_forward(0, 3), 15);
        assert_eq!(buffer.move_n_words_backward(end, 2, Mode::Insert), 8);
        assert_eq!(buffer.move_n_words_backward(end, 3, Mode::Insert), 4);

        // FIXME: see #501 for possible issues in WordCursor::next_boundary()
        //
        // Trying to move beyond the buffer end.  The cursor will stay there.
        for offset in 0..end {
            assert_eq!(buffer.move_n_words_forward(offset, 100), end + 1);
        }

        // In the other direction.
        for offset in 0..end {
            assert_eq!(
                buffer.move_n_words_backward(end - offset, 100, Mode::Insert),
                0
            );
        }
    }

    mod on_word_end_forward {
        use super::*;

        #[test]
        fn non_insertion_mode() {
            // To save some keystrokes.
            fn v(buf: &Buffer, off: usize, n: usize, end: usize) {
                assert_eq!(buf.move_n_wordends_forward(off, n, false), end);
            }

            let buffer = Buffer::new("one two three  four  ");
            //                      ->012345678901234567890<-

            // 0 count does not move.
            v(&buffer, 0, 0, 0);

            for offset in 0..2 {
                v(&buffer, offset, 1, 2);
                v(&buffer, offset, 2, 6);
                v(&buffer, offset, 3, 12);
            }

            let end = buffer.len() - 1;
            // Trying to move beyond the buffer end.
            for offset in 0..end {
                v(&buffer, offset, 100, 21);
            }

            v(&buffer, 2, 1, 6);
            v(&buffer, 2, 2, 12);
            v(&buffer, 2, 3, 18);
            v(&buffer, 2, 10, 21);

            let buffer = Buffer::new("one\n\ntwo\n\n\nthree\n\n\n");
            //                      ->0123 4 5678 9 0 123456 7 8 <-

            v(&buffer, 0, 2, 7);
            v(&buffer, 0, 3, 15);
            v(&buffer, 0, 4, 19);
        }

        #[test]
        fn insertion_mode() {
            fn v(buf: &Buffer, off: usize, n: usize, end: usize) {
                assert_eq!(buf.move_n_wordends_forward(off, n, true), end);
            }

            let buffer = Buffer::new("one two three  four  ");
            //                      ->012345678901234567890<-

            // 0 count does not move.
            v(&buffer, 0, 0, 0);

            for offset in 0..2 {
                // In Mode::Insert, returns 1 pass the word end.
                v(&buffer, offset, 1, 3);
                v(&buffer, offset, 2, 7);
                v(&buffer, offset, 3, 13);
            }

            let end = buffer.len() - 1;
            // Trying to move beyond the buffer end.
            for offset in 0..end {
                v(&buffer, offset, 100, 21);
            }

            v(&buffer, 2, 1, 7);
            v(&buffer, 2, 2, 13);
            v(&buffer, 2, 3, 19);
            v(&buffer, 2, 10, 21);

            let buffer = Buffer::new("one\n\ntwo\n\n\nthree\n\n\n");
            //                      ->0123 4 5678 9 0 123456 7 8 <-

            v(&buffer, 0, 2, 8);
            v(&buffer, 0, 3, 16);
            v(&buffer, 0, 4, 19);
        }
    }
}
