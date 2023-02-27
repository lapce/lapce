use druid::{piet::TextLayout, Point};


pub fn truncate_if_necessary<T: TextLayout>(text_layout: &T, max_width: f64, text: String) -> Option<String> {
    if text_layout.size().width > max_width {
        let hit_point = text_layout.hit_test_point(
            Point::new(max_width, 0.0));

        let end = text
            .char_indices()
            .filter(|(i,_)| hit_point.idx.overflowing_sub(*i).0 < 3)
            .collect::<Vec<(usize, char)>>();

        let end = if end.is_empty() {
            text.len()
        } else {
            end[0].0
        };

        if end == 0 {
            Some("".to_string())
        } else {
            Some(format!("{}...", &text[0..end].trim_end()))
        }
    } else {
        None
    }
}
