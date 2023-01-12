use lapce_data::{debug::DebugData, panel::PanelKind};

use crate::panel::LapcePanel;

pub fn new_debug_panel(data: &DebugData) -> LapcePanel {
    LapcePanel::new(PanelKind::Debug, data.widget_id, data.split_id, vec![])
}
