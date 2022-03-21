pub enum PanelResizePosition {
    Left,
    LeftSplit,
    Bottom,
}

#[derive(Eq, PartialEq, Hash, Clone)]
pub enum PanelPosition {
    LeftTop,
    LeftBottom,
    BottomLeft,
    BottomRight,
    RightTop,
    RightBottom,
}
