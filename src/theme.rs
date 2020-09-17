use druid::{Color, FontDescriptor, Key};

pub struct CraneTheme {}

impl CraneTheme {
    pub const EDITOR_LINE_HEIGHT: Key<f64> =
        Key::new("crane.editor_line_height");
    pub const PALETTE_BACKGROUND: Key<Color> =
        Key::new("crane.palette_background");
    pub const PALETTE_INPUT_BACKGROUND: Key<Color> =
        Key::new("crane.palette_input_background");
    pub const PALETTE_INPUT_FOREROUND: Key<Color> =
        Key::new("crane.palette_input_foreground");
    pub const PALETTE_INPUT_BORDER: Key<Color> =
        Key::new("crane.palette_input_border");
    pub const EDITOR_FONT: Key<FontDescriptor> = Key::new("crane.eidtor_font");
}
