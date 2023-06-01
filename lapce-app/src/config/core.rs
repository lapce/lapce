use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct CoreConfig {
    #[field_names(desc = "Enable modal editing (Vim like)")]
    pub modal: bool,
    #[field_names(desc = "Set the color theme of Lapce")]
    pub color_theme: String,
    #[field_names(desc = "Set the icon theme of Lapce")]
    pub icon_theme: String,
    #[field_names(
        desc = "Enable customised titlebar and disable OS native one (Linux, BSD, Windows)"
    )]
    pub custom_titlebar: bool,
}
