use strum_macros::{Display, EnumIter, EnumString, IntoStaticStr};

#[derive(
    Display, EnumString, EnumIter, IntoStaticStr, Debug, Clone, Copy, PartialEq, Eq,
)]
pub enum EditorColor {
    #[strum(serialize = "editor.background")]
    Background,
    #[strum(serialize = "editor.scroll_bar")]
    Scrollbar,
    #[strum(serialize = "editor.dropdown_shadow")]
    DropdownShadow,
    #[strum(serialize = "editor.foreground")]
    Foreground,
    #[strum(serialize = "editor.dim")]
    Dim,
    #[strum(serialize = "editor.focus")]
    Focus,
    #[strum(serialize = "editor.caret")]
    Caret,
    #[strum(serialize = "editor.selection")]
    Selection,
    #[strum(serialize = "editor.current_line")]
    CurrentLine,
    #[strum(serialize = "editor.link")]
    Link,
    #[strum(serialize = "editor.visible_whitespace")]
    VisibleWhitespace,
    #[strum(serialize = "editor.indent_guide")]
    IndentGuide,
    #[strum(serialize = "editor.sticky_header_background")]
    StickyHeaderBackground,
    #[strum(serialize = "editor.preedit.underline")]
    PreeditUnderline,
}
