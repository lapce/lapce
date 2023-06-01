use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct PanelStyle {
    pub active: usize,
    pub shown: bool,
    pub maximized: bool,
}
