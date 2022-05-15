use super::WordCursor;
use crate::buffer::Buffer;

#[test]
fn test_next_boundary_by_newline() {
    let buffer = Buffer::new("a\nb");
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.next_boundary(), Some(2));
    assert_eq!(cursor.next_boundary(), Some(buffer.len()));
    assert_eq!(cursor.next_boundary(), None);
}

#[test]
fn test_next_boundary_by_space() {
    let buffer = Buffer::new("a b   ");
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.next_boundary(), Some(2));
    assert_eq!(cursor.next_boundary(), Some(buffer.len()));
    assert_eq!(cursor.next_boundary(), None);
}

#[test]
fn test_end_boundary_by_spaces() {
    // NOTE: The word end boundary is 1 after the last character in the word.
    // For example, in "abc-def", the end boundary of the word "abc" is at the
    // position of '-'.
    let buffer = Buffer::new("");
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.end_boundary(), None);
    let buffer = Buffer::new("a");
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.end_boundary(), None);

    let buffer = Buffer::new("bc");
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.end_boundary(), Some(2));
    assert_eq!(cursor.end_boundary(), None);

    let buffer = Buffer::new("a bc  def   ");
    //                      ->012345678901<-
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.end_boundary(), Some(4));
    assert_eq!(cursor.end_boundary(), Some(9));
    // This end of word bounary is expected to go 1 beyond the buffer end.
    assert_eq!(cursor.end_boundary(), Some(buffer.len()));
    assert_eq!(cursor.end_boundary(), None);
}

#[test]
fn test_end_boundary_by_newline() {
    let buffer = Buffer::new("a\nbc\n\ndef\n\n\nghij\n\n\n");
    //                      ->01 234 5 6789 0 1 23456 7 8 <-
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.end_boundary(), Some(4));
    assert_eq!(cursor.end_boundary(), Some(9));
    assert_eq!(cursor.end_boundary(), Some(16));
    assert_eq!(cursor.end_boundary(), Some(buffer.len()));
    assert_eq!(cursor.end_boundary(), None);
}

// This test fails. See #501.
#[should_panic]
#[test]
fn next_boundary_failure_to_be_fixed_1() {
    let buffer = Buffer::new("a\nb\n\n\n");
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.next_boundary(), Some(2));
    // This assertion fails.
    assert_eq!(cursor.next_boundary(), Some(buffer.len()));
    assert_eq!(cursor.next_boundary(), None);
}

#[should_panic]
#[test]
fn next_boundary_failure_to_be_fixed_2() {
    let buffer = Buffer::new("a\nb\n\n\n");
    let mut cursor = WordCursor::new(buffer.text(), 0);
    assert_eq!(cursor.next_boundary(), Some(2));
    // This assertion also fails:
    assert_eq!(cursor.next_boundary(), Some(3));
    // assert_eq!(cursor.next_boundary(), Some(4));
    // assert_eq!(cursor.next_boundary(), Some(5));
    // assert_eq!(cursor.next_boundary(), Some(6));
    // assert_eq!(cursor.next_boundary(), None);
}
