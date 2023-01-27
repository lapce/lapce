use std::sync::Arc;

use druid::{Data, WidgetId};

use crate::{
    config::{DropdownInfo, LapceConfig},
    list::ListData,
};

/// Data for a dropdown menu.  
/// **Note**: You should call `data.clone_with(config)` before passing it to the widget functions
/// because we need access to a recent version of the config.
#[derive(Clone)]
pub struct DropdownData<T: Clone, D: Data> {
    /// The index of the active item in the list, not necessarily the
    /// same as the index of the selected item, because you have to
    /// 'apply' the select item to change this
    pub active_item_index: usize,
    /// Whether the list is currently active for selection.
    pub list_active: bool,
    pub list: ListData<T, D>,
}

impl<T: Clone, D: Data> DropdownData<T, D> {
    pub fn new(config: Arc<LapceConfig>, parent: WidgetId, data: D) -> Self {
        let list = ListData::new(config, parent, data);
        DropdownData {
            active_item_index: 0,
            list_active: false,
            list,
        }
    }

    pub fn clone_with(&self, config: Arc<LapceConfig>) -> Self {
        let mut data = self.clone();
        data.update_data(config);

        data
    }

    pub fn update_data(&mut self, config: Arc<LapceConfig>) {
        self.list.update_data(config);
    }

    pub fn update_active_item(&mut self) {
        if self.list.current_selected_item().is_some() {
            self.active_item_index = self.list.selected_index;
        }
    }

    /// Show the dropdown-list. You'll need to request a layout.
    pub fn show(&mut self) {
        self.list_active = true;
    }

    /// Hide the dropdown-list from view. You'll need to request a layout.
    pub fn hide(&mut self) {
        self.list_active = false;
    }

    pub fn get_active_item(&self) -> Option<&T> {
        self.list.items.get(self.active_item_index)
    }
}

impl<D: Data> DropdownData<String, D> {
    pub fn update_from_info(&mut self, info: DropdownInfo) {
        self.active_item_index = info.active_index;
        self.list.items = info.items;
        self.list.selected_index = info.active_index;
    }
}

impl<T: Clone + PartialEq + 'static, D: Data> Data for DropdownData<T, D> {
    fn same(&self, other: &Self) -> bool {
        self.active_item_index == other.active_item_index
            && self.list_active == other.list_active
            && self.list.same(&other.list)
    }
}
