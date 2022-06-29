pub enum PanelResizePosition {
    Left,
    LeftSplit,
    Bottom,
}

#[derive(Eq, PartialEq, Hash, Clone, Copy)]
pub enum PanelPosition {
    LeftTop,
    LeftBottom,
    BottomLeft,
    BottomRight,
    RightTop,
    RightBottom,
}

#[derive(Eq, PartialEq, Hash, Clone, Copy)]
pub enum PanelContainerPosition {
    Left,
    Bottom,
    Right,
}

impl PanelContainerPosition {
    pub fn is_bottom(&self) -> bool {
        matches!(self, PanelContainerPosition::Bottom)
    }
}
