use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PanelStyle {
    pub active: usize,
    pub shown: bool,
    pub maximized: bool,
}
