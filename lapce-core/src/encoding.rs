/// Convert a utf8 offset into a utf16 offset, if possible  
/// `text` is what the offsets are into
pub fn offset_utf8_to_utf16(
    char_indices: impl Iterator<Item = (usize, char)>,
    offset: usize,
) -> usize {
    if offset == 0 {
        return 0;
    }

    let mut utf16_offset = 0;
    let mut last_ich = None;
    for (utf8_offset, ch) in char_indices {
        last_ich = Some((utf8_offset, ch));

        match utf8_offset.cmp(&offset) {
            std::cmp::Ordering::Less => {}
            // We found the right offset
            std::cmp::Ordering::Equal => {
                return utf16_offset;
            }
            // Implies that the offset was inside of a character
            std::cmp::Ordering::Greater => return utf16_offset,
        }

        utf16_offset += ch.len_utf16();
    }

    // TODO: We could use TrustedLen when that is stabilized and it is impl'd on
    // the iterators we use

    // We did not find the offset. This means that it is either at the end
    // or past the end.
    let text_len = last_ich.map(|(i, c)| i + c.len_utf8());
    if text_len == Some(offset) {
        // Since the utf16 offset was being incremented each time, by now it is equivalent to the length
        // but in utf16 characters
        return utf16_offset;
    }

    utf16_offset
}

pub fn offset_utf8_to_utf16_str(text: &str, offset: usize) -> usize {
    offset_utf8_to_utf16(text.char_indices(), offset)
}

/// Convert a utf16 offset into a utf8 offset, if possible  
/// `char_indices` is an iterator over utf8 offsets and the characters
/// It is cloneable so that it can be iterated multiple times. Though it should be cheaply cloneable.
pub fn offset_utf16_to_utf8(
    char_indices: impl Iterator<Item = (usize, char)>,
    offset: usize,
) -> usize {
    if offset == 0 {
        return 0;
    }

    // We accumulate the utf16 char lens until we find the utf8 offset that matches it
    // or, we find out that it went into the middle of sometext
    // We also keep track of the last offset and char in order to calculate the length of the text
    // if we the index was at the end of the string
    let mut utf16_offset = 0;
    let mut last_ich = None;
    for (utf8_offset, ch) in char_indices {
        last_ich = Some((utf8_offset, ch));

        let ch_utf16_len = ch.len_utf16();

        match utf16_offset.cmp(&offset) {
            std::cmp::Ordering::Less => {}
            // We found the right offset
            std::cmp::Ordering::Equal => {
                return utf8_offset;
            }
            // This implies that the offset was in the middle of a character as we skipped over it
            std::cmp::Ordering::Greater => return utf8_offset,
        }

        utf16_offset += ch_utf16_len;
    }

    // We did not find the offset, this means that it was either at the end
    // or past the end
    // Since we've iterated over all the char indices, the utf16_offset is now the
    // utf16 length
    if let Some((last_utf8_offset, last_ch)) = last_ich {
        last_utf8_offset + last_ch.len_utf8()
    } else {
        0
    }
}

pub fn offset_utf16_to_utf8_str(text: &str, offset: usize) -> usize {
    offset_utf16_to_utf8(text.char_indices(), offset)
}

#[cfg(test)]
mod tests {
    // TODO: more tests with unicode characters

    use crate::encoding::{offset_utf8_to_utf16_str, offset_utf16_to_utf8_str};

    #[test]
    fn utf8_to_utf16() {
        let text = "hello world";

        assert_eq!(offset_utf8_to_utf16_str(text, 0), 0);
        assert_eq!(offset_utf8_to_utf16_str("", 0), 0);

        assert_eq!(offset_utf8_to_utf16_str("", 1), 0);

        assert_eq!(offset_utf8_to_utf16_str("h", 0), 0);
        assert_eq!(offset_utf8_to_utf16_str("h", 1), 1);

        assert_eq!(offset_utf8_to_utf16_str(text, text.len()), text.len());

        assert_eq!(
            offset_utf8_to_utf16_str(text, text.len() - 1),
            text.len() - 1
        );

        assert_eq!(offset_utf8_to_utf16_str(text, text.len() + 1), text.len());

        assert_eq!(offset_utf8_to_utf16_str("×", 0), 0);
        assert_eq!(offset_utf8_to_utf16_str("×", 1), 1);
        assert_eq!(offset_utf8_to_utf16_str("×", 2), 1);
        assert_eq!(offset_utf8_to_utf16_str("a×", 0), 0);
        assert_eq!(offset_utf8_to_utf16_str("a×", 1), 1);
        assert_eq!(offset_utf8_to_utf16_str("a×", 2), 2);
        assert_eq!(offset_utf8_to_utf16_str("a×", 3), 2);
    }

    #[test]
    fn utf16_to_utf8() {
        let text = "hello world";

        assert_eq!(offset_utf16_to_utf8_str(text, 0), 0);
        assert_eq!(offset_utf16_to_utf8_str("", 0), 0);

        assert_eq!(offset_utf16_to_utf8_str("", 1), 0);

        assert_eq!(offset_utf16_to_utf8_str("h", 0), 0);
        assert_eq!(offset_utf16_to_utf8_str("h", 1), 1);

        assert_eq!(offset_utf16_to_utf8_str(text, text.len()), text.len());

        assert_eq!(
            offset_utf16_to_utf8_str(text, text.len() - 1),
            text.len() - 1
        );

        assert_eq!(offset_utf16_to_utf8_str(text, text.len() + 1), text.len());

        assert_eq!(offset_utf16_to_utf8_str("×", 0), 0);
        assert_eq!(offset_utf16_to_utf8_str("×", 1), 2);
        assert_eq!(offset_utf16_to_utf8_str("a×", 0), 0);
        assert_eq!(offset_utf16_to_utf8_str("a×", 1), 1);
        assert_eq!(offset_utf16_to_utf8_str("a×", 2), 3);
        assert_eq!(offset_utf16_to_utf8_str("×a", 1), 2);
        assert_eq!(offset_utf16_to_utf8_str("×a", 2), 3);
    }
}
