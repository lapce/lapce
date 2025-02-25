use std::rc::Rc;

use floem::{View, reactive::SignalGet, views::Decorators};

use super::position::PanelPosition;
use crate::{
    panel::implementation_view::common_reference_panel, window_tab::WindowTabData,
};

pub fn references_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    common_reference_panel(window_tab_data.clone(), _position, move || {
        window_tab_data.main_split.references.get()
    })
    .debug_name("references panel")
}
