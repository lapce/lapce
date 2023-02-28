use druid::{piet::TextLayout, Point};

/// This method truncates the text string if its width, as determined
/// by the `text_layout` object, exceeds the specified `max_width`. If
/// the text does not need to be truncated, the method returns `None`.
///
/// ## Parameters
/// * `text_layout`: A reference to an object implementing the TextLayout trait.
/// * `max_width`: A f64 value representing the maximum width allowed for the `text_layout`.
/// * `text`: A String value representing the text to be truncated if necessary.
///
/// ## Return Value
/// * If the text needs to be truncated, the method returns `Some(truncated_text)`,
///   where `truncated_text` is a `String` value representing the truncated text.
/// * If the text does not need to be truncated, the method returns `None`.
pub fn truncate_if_necessary<T: TextLayout>(
    text_layout: &T,
    max_width: f64,
    text: String,
) -> Option<String> {
    if text_layout.size().width > max_width {
        let hit_point = text_layout.hit_test_point(Point::new(max_width, 0.0));

        let end = text
            .char_indices()
            .filter(|(i, _)| hit_point.idx.overflowing_sub(*i).0 < 3)
            .collect::<Vec<(usize, char)>>();

        let end = if end.is_empty() {
            // the text does exceed the `max_width`
            // this should never be reached since it was
            // already checked if the width is okay
            text.len()
        } else {
            end[0].0
        };

        if end == 0 {
            // the length of the truncated string must be
            // zero for it not to exceed the `max_width`
            Some("".to_string())
        } else {
            // add ellipsis to the truncated text
            Some(format!("{}...", &text[0..end].trim_end()))
        }
    } else {
        None
    }
}
