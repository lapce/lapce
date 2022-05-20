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
