use druid::{FontDescriptor, FontFamily, TextLayout};

pub struct CraneFont {
    font: FontDescriptor,
}

impl CraneFont {
    pub fn new(font_name: &str, size: f64) {
        let font =
            FontDescriptor::new(FontFamily::new_unchecked("Cascadia Code"))
                .with_size(size);
        let mut text_layout = TextLayout::new("W");
        text_layout.set_font(font.clone());
    }
}
