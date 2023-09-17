use floem::cosmic_text::FamilyOwned;
use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct UIConfig {
    #[field_names(
        desc = "Set the UI font family. If empty, it uses system default."
    )]
    pub font_family: String,

    #[field_names(desc = "Set the UI base font size")]
    font_size: usize,

    #[field_names(desc = "Set the icon size in the UI")]
    icon_size: usize,

    #[field_names(
        desc = "Set the header height for panel header and editor tab header"
    )]
    header_height: usize,

    #[field_names(desc = "Set the height for status line")]
    status_height: usize,

    #[field_names(desc = "Set the minimum width for editor tab")]
    tab_min_width: usize,

    #[field_names(desc = "Set the width for scroll bar")]
    scroll_width: usize,

    #[field_names(desc = "Controls the width of drop shadow in the UI")]
    drop_shadow_width: usize,

    #[field_names(desc = "Controls the width of the preview editor")]
    preview_editor_width: usize,

    #[field_names(
        desc = "Set the hover font family. If empty, it uses the UI font family"
    )]
    hover_font_family: String,
    #[field_names(desc = "Set the hover font size. If 0, uses the UI font size")]
    hover_font_size: usize,

    #[field_names(desc = "Trim whitespace from search results")]
    pub trim_search_results_whitespace: bool,

    #[field_names(desc = "Set the line height for list items")]
    list_line_height: usize,

    #[field_names(desc = "Set position of the close button in editor tabs")]
    pub tab_close_button: TabCloseButton,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Deserialize,
    Serialize,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum_macros::EnumVariantNames,
)]
pub enum TabCloseButton {
    Left,
    #[default]
    Right,
    Off,
}

impl UIConfig {
    pub fn font_size(&self) -> usize {
        self.font_size.max(6).min(32)
    }

    pub fn font_family(&self) -> Vec<FamilyOwned> {
        FamilyOwned::parse_list(&self.font_family).collect()
    }

    pub fn header_height(&self) -> usize {
        let font_size = self.font_size();
        self.header_height.max(font_size)
    }

    pub fn icon_size(&self) -> usize {
        if self.icon_size == 0 {
            self.font_size()
        } else {
            self.icon_size.max(6).min(32)
        }
    }

    pub fn status_height(&self) -> usize {
        let font_size = self.font_size();
        self.status_height.max(font_size)
    }
}
