use druid::Data;
use serde::{Deserialize, Serialize};

use crate::config::LapceIcons;

pub type PanelOrder = im::HashMap<PanelPosition, im::Vector<PanelKind>>;

#[derive(Clone, Copy, PartialEq, Data, Serialize, Deserialize, Hash, Eq, Debug)]
pub enum PanelKind {
    FileExplorer,
    SourceControl,
    Plugin,
    Terminal,
    Search,
    Problem,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PanelSize {
    pub left: f64,
    pub left_split: f64,
    pub bottom: f64,
    pub bottom_split: f64,
    pub right: f64,
    pub right_split: f64,
}

impl PanelKind {
    pub fn svg_name(&self) -> &'static str {
        match &self {
            PanelKind::FileExplorer => LapceIcons::FILE_EXPLORER,
            PanelKind::SourceControl => LapceIcons::SCM,
            PanelKind::Plugin => LapceIcons::EXTENSIONS,
            PanelKind::Terminal => LapceIcons::TERMINAL,
            PanelKind::Search => LapceIcons::SEARCH,
            PanelKind::Problem => LapceIcons::PROBLEM,
        }
    }

    pub fn panel_name(&self) -> &'static str {
        match &self {
            PanelKind::FileExplorer => "File Explorer",
            PanelKind::SourceControl => "Source Control",
            PanelKind::Plugin => "Plugin",
            PanelKind::Terminal => "Terminal",
            PanelKind::Search => "Search",
            PanelKind::Problem => "Problem",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PanelStyle {
    pub active: usize,
    pub shown: bool,
    pub maximized: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PanelData {
    pub active: PanelPosition,
    pub order: PanelOrder,
    pub style: im::HashMap<PanelPosition, PanelStyle>,
    pub size: PanelSize,
}

impl PanelData {
    pub fn new(order: PanelOrder) -> Self {
        let size = PanelSize {
            left: 250.0,
            left_split: 0.5,
            bottom: 300.0,
            bottom_split: 0.5,
            right: 250.0,
            right_split: 0.5,
        };
        let mut style = im::HashMap::new();
        style.insert(
            PanelPosition::LeftTop,
            PanelStyle {
                active: 0,
                shown: true,
                maximized: false,
            },
        );
        style.insert(
            PanelPosition::LeftBottom,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        style.insert(
            PanelPosition::BottomLeft,
            PanelStyle {
                active: 0,
                shown: true,
                maximized: false,
            },
        );
        style.insert(
            PanelPosition::BottomRight,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        style.insert(
            PanelPosition::RightTop,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        style.insert(
            PanelPosition::RightBottom,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );

        Self {
            active: PanelPosition::LeftTop,
            order,
            size,
            style,
        }
    }

    pub fn is_container_shown(&self, position: &PanelContainerPosition) -> bool {
        self.is_position_shown(&position.first())
            || self.is_position_shown(&position.second())
    }

    pub fn is_position_shown(&self, position: &PanelPosition) -> bool {
        self.style.get(position).map(|s| s.shown).unwrap_or(false)
    }

    pub fn is_contianer_maximized(&self, position: &PanelContainerPosition) -> bool {
        self.is_position_maximized(&position.first())
            || self.is_position_maximized(&position.second())
    }

    pub fn is_position_maximized(&self, position: &PanelPosition) -> bool {
        self.style
            .get(position)
            .map(|s| s.maximized)
            .unwrap_or(false)
    }

    pub fn panel_position(
        &self,
        kind: &PanelKind,
    ) -> Option<(usize, PanelPosition)> {
        for (pos, panels) in self.order.iter() {
            let index = panels.iter().position(|k| k == kind);
            if let Some(index) = index {
                return Some((index, *pos));
            }
        }
        None
    }

    pub fn toggle_maximize(&mut self, position: &PanelPosition) {
        match position {
            PanelPosition::LeftTop | PanelPosition::LeftBottom => {
                self.toggle_left_maximize();
            }
            PanelPosition::BottomLeft | PanelPosition::BottomRight => {
                self.toggle_bottom_maximize();
            }
            PanelPosition::RightTop | PanelPosition::RightBottom => {
                self.toggle_right_maximize();
            }
        }
    }

    pub fn toggle_active_maximize(&mut self) {
        self.toggle_maximize(&self.active.clone());
    }

    pub fn set_container_maximized(
        &mut self,
        position: &PanelContainerPosition,
        maximized: bool,
    ) {
        vec![position.first(), position.second()]
            .iter()
            .for_each(|p| {
                if let Some(style) = self.style.get_mut(p) {
                    style.maximized = maximized;
                }
            });
    }

    pub fn set_container_shown(
        &mut self,
        position: &PanelContainerPosition,
        shown: bool,
    ) {
        vec![position.first(), position.second()]
            .iter()
            .for_each(|p| {
                if let Some(style) = self.style.get_mut(p) {
                    style.shown = shown;
                }
            });
    }

    pub fn toggle_left_maximize(&mut self) {
        let maximized = !self.is_contianer_maximized(&PanelContainerPosition::Left);
        self.set_container_maximized(&PanelContainerPosition::Left, maximized);

        if self.is_contianer_maximized(&PanelContainerPosition::Right) {
            self.set_container_maximized(&PanelContainerPosition::Right, false);
        }
        if maximized && self.is_container_shown(&PanelContainerPosition::Bottom) {
            self.set_container_shown(&PanelContainerPosition::Bottom, false);
        }
    }

    pub fn toggle_right_maximize(&mut self) {
        let maximized = !self.is_contianer_maximized(&PanelContainerPosition::Right);
        self.set_container_maximized(&PanelContainerPosition::Right, maximized);

        if self.is_contianer_maximized(&PanelContainerPosition::Left) {
            self.set_container_maximized(&PanelContainerPosition::Left, false);
        }
        if maximized && self.is_container_shown(&PanelContainerPosition::Bottom) {
            self.set_container_shown(&PanelContainerPosition::Bottom, false);
        }
    }

    pub fn toggle_bottom_maximize(&mut self) {
        let maximized =
            !self.is_contianer_maximized(&PanelContainerPosition::Bottom);
        self.set_container_maximized(&PanelContainerPosition::Bottom, maximized);

        if self.is_contianer_maximized(&PanelContainerPosition::Left) {
            self.set_container_maximized(&PanelContainerPosition::Left, false);
        }

        if self.is_contianer_maximized(&PanelContainerPosition::Right) {
            self.set_container_maximized(&PanelContainerPosition::Right, false);
        }
    }

    pub fn set_shown(&mut self, position: &PanelPosition, shown: bool) {
        if let Some(style) = self.style.get_mut(position) {
            style.shown = shown;
        }
    }

    pub fn toggle_container_visual(&mut self, position: &PanelContainerPosition) {
        let shown = !self.is_container_shown(position);
        if shown {
            if let Some((kind, _)) =
                self.active_panel_at_position(&position.second())
            {
                self.show_panel(&kind);
            }
            if let Some((kind, _)) = self.active_panel_at_position(&position.first())
            {
                self.show_panel(&kind);
            }
        } else {
            if let Some((kind, _)) =
                self.active_panel_at_position(&position.second())
            {
                self.hide_panel(&kind);
            }
            if let Some((kind, _)) = self.active_panel_at_position(&position.first())
            {
                self.hide_panel(&kind);
            }
        }
    }

    pub fn position_has_panels(&self, position: &PanelPosition) -> bool {
        self.order
            .get(position)
            .map(|o| !o.is_empty())
            .unwrap_or(false)
    }

    pub fn active_panel_at_position(
        &self,
        position: &PanelPosition,
    ) -> Option<(PanelKind, bool)> {
        let style = self.style.get(position)?;
        let order = self.order.get(position)?;
        order
            .get(style.active)
            .cloned()
            .or_else(|| order.last().cloned())
            .map(|p| (p, style.shown))
    }

    pub fn hide_panel(&mut self, kind: &PanelKind) {
        if let Some((_, position)) = self.panel_position(kind) {
            if let Some((active_panel, _)) = self.active_panel_at_position(&position)
            {
                if &active_panel == kind {
                    self.set_shown(&position, false);
                    let peer_position = position.peer();
                    if let Some(order) = self.order.get(&peer_position) {
                        if order.is_empty() {
                            self.set_shown(&peer_position, false);
                        }
                    }
                }
            }
        }
    }

    pub fn show_panel(&mut self, kind: &PanelKind) {
        if let Some((index, position)) = self.panel_position(kind) {
            if let Some(style) = self.style.get_mut(&position) {
                style.shown = true;
                style.active = index;
            }
        }
    }

    pub fn is_panel_visible(&self, kind: &PanelKind) -> bool {
        if let Some((index, position)) = self.panel_position(kind) {
            if let Some(style) = self.style.get(&position) {
                return style.active == index && style.shown;
            }
        }
        false
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum PanelResizePosition {
    Left,
    LeftSplit,
    Right,
    Bottom,
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PanelPosition {
    LeftTop,
    LeftBottom,
    BottomLeft,
    BottomRight,
    RightTop,
    RightBottom,
}

impl PanelPosition {
    pub fn is_bottom(&self) -> bool {
        matches!(self, PanelPosition::BottomLeft | PanelPosition::BottomRight)
    }

    pub fn peer(&self) -> PanelPosition {
        match &self {
            PanelPosition::LeftTop => PanelPosition::LeftBottom,
            PanelPosition::LeftBottom => PanelPosition::LeftTop,
            PanelPosition::BottomLeft => PanelPosition::BottomRight,
            PanelPosition::BottomRight => PanelPosition::BottomLeft,
            PanelPosition::RightTop => PanelPosition::RightBottom,
            PanelPosition::RightBottom => PanelPosition::RightTop,
        }
    }
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug)]
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
