use druid::{Color, FontDescriptor, Key};

pub struct LapceTheme {}

impl LapceTheme {
    pub const CHANGED: Key<bool> = Key::new("lapce.theme_changed");
    pub const EDITOR_LINE_HEIGHT: Key<f64> = Key::new("lapce.editor_line_height");
    pub const PALETTE_BACKGROUND: Key<Color> = Key::new("lapce.palette_background");
    pub const PALETTE_INPUT_BACKGROUND: Key<Color> =
        Key::new("lapce.palette_input_background");
    pub const PALETTE_INPUT_FOREROUND: Key<Color> =
        Key::new("lapce.palette_input_foreground");
    pub const PALETTE_INPUT_BORDER: Key<Color> =
        Key::new("lapce.palette_input_border");
    pub const EDITOR_FONT: Key<FontDescriptor> = Key::new("lapce.eidtor_font");
    pub const EDITOR_COMMENT: Key<Color> = Key::new("lapce.eidtor_comment");
    pub const EDITOR_FOREGROUND: Key<Color> = Key::new("lapce.eidtor_foreground");
    pub const EDITOR_BACKGROUND: Key<Color> = Key::new("lapce.eidtor_background");
    pub const EDITOR_ERROR: Key<Color> = Key::new("lapce.eidtor_error");
    pub const EDITOR_WARN: Key<Color> = Key::new("lapce.eidtor_warn");
    pub const EDITOR_CURSOR_COLOR: Key<Color> =
        Key::new("lapce.eidtor_cursor_color");
    pub const EDITOR_CURRENT_LINE_BACKGROUND: Key<Color> =
        Key::new("lapce.eidtor_current_line_background");
    pub const EDITOR_SELECTION_COLOR: Key<Color> =
        Key::new("lapce.editor_selection_color");
    pub const LIST_BACKGROUND: Key<Color> = Key::new("lapce.list_background");
    pub const LIST_CURRENT: Key<Color> = Key::new("lapce.list_current");
}
