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

    pub fn first(&self) -> PanelPosition {
        match self {
            PanelContainerPosition::Left => PanelPosition::LeftTop,
            PanelContainerPosition::Bottom => PanelPosition::BottomLeft,
            PanelContainerPosition::Right => PanelPosition::RightTop,
        }
    }

    pub fn second(&self) -> PanelPosition {
        match self {
            PanelContainerPosition::Left => PanelPosition::LeftBottom,
            PanelContainerPosition::Bottom => PanelPosition::BottomRight,
            PanelContainerPosition::Right => PanelPosition::RightBottom,
        }
    }
}
