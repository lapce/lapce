use druid::{Env, PaintCtx};

use crate::{
    panel::{PanelPosition, PanelProperty},
    state::LapceUIState,
};

pub struct OutlineState {}

impl PanelProperty for OutlineState {
    fn position(&self) -> &PanelPosition {
        &PanelPosition::RightTop
    }

    fn active(&self) -> usize {
        0
    }

    fn size(&self) -> (f64, f64) {
        (300.0, 0.5)
    }

    fn paint(&self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {}
}

impl OutlineState {
    pub fn new() -> Self {
        Self {}
    }
}
