use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

pub type LineStyles = HashMap<usize, Arc<Vec<LineStyle>>>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LineStyle {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub fg_color: Option<String>,
}
