use druid::{Env, PaintCtx, WidgetId};

use crate::panel::{PanelPosition, PanelProperty};

pub struct OutlineState {
    widget_id: WidgetId,
}

impl OutlineState {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
        }
    }
}
