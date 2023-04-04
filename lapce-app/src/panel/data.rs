use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal},
};
use serde::{Deserialize, Serialize};

use super::{kind::PanelKind, position::PanelPosition, style::PanelStyle};

pub type PanelOrder = im::HashMap<PanelPosition, im::Vector<PanelKind>>;

pub fn default_panel_order() -> PanelOrder {
    let mut order = PanelOrder::new();
    order.insert(
        PanelPosition::LeftTop,
        im::vector![
            PanelKind::FileExplorer,
            PanelKind::Debug,
            PanelKind::SourceControl,
            PanelKind::Plugin,
        ],
    );
    order.insert(
        PanelPosition::BottomLeft,
        im::vector![PanelKind::Terminal, PanelKind::Search, PanelKind::Problem,],
    );

    order
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

#[derive(Clone)]
pub struct PanelData {
    pub panels: RwSignal<PanelOrder>,
    pub styles: RwSignal<im::HashMap<PanelPosition, PanelStyle>>,
    pub size: RwSignal<PanelSize>,
}

impl PanelData {
    pub fn new(
        cx: AppContext,
        panels: im::HashMap<PanelPosition, im::Vector<PanelKind>>,
    ) -> Self {
        let panels = create_rw_signal(cx.scope, panels);

        let mut styles = im::HashMap::new();
        styles.insert(
            PanelPosition::LeftTop,
            PanelStyle {
                active: 0,
                shown: true,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::LeftBottom,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::BottomLeft,
            PanelStyle {
                active: 0,
                shown: true,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::BottomRight,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::RightTop,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::RightBottom,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        let styles = create_rw_signal(cx.scope, styles);
        let size = create_rw_signal(
            cx.scope,
            PanelSize {
                left: 250.0,
                left_split: 0.5,
                bottom: 300.0,
                bottom_split: 0.5,
                right: 250.0,
                right_split: 0.5,
            },
        );

        Self {
            panels,
            styles,
            size,
        }
    }
}
