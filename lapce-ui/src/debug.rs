use lapce_data::{debug::RunDebugData, panel::PanelKind};

use crate::panel::LapcePanel;

pub fn new_debug_panel(data: &RunDebugData) -> LapcePanel {
    LapcePanel::new(PanelKind::Debug, data.widget_id, data.split_id, vec![])
}
