use std::sync::Arc;

use druid::{Command, Data, EventCtx, Target, WidgetId};
use lapce_core::{command::FocusCommand, movement::Movement};

use crate::{
    command::{
        CommandExecuted, CommandKind, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND,
    },
    config::{GetConfig, LapceConfig},
};

// Note: when adding fields to this, make sure to think whether they need to be added to the `same`
// implementation
/// Note: all `T` are going to be required by the UI code to implement `ListPaint<D>`  
#[derive(Clone)]
pub struct ListData<T: Clone, D: Data> {
    /// The id of the widget that contains the [`ListData`] which wishes to receive
    /// events (such as when an entry is selected)
    pub parent: WidgetId,

    /// The items that can be selected from the list  
    /// Note that since this is an `im::Vector`, cloning is cheap.  
    pub items: im::Vector<T>,
    /// Extra data attached to the list for the item rendering to use.
    pub data: D,

    /// The index of the item which is selected
    pub selected_index: usize,

    /// The maximum number of items to render in the list.
    pub max_displayed_items: usize,

    /// The line height of each list element  
    /// Defaults to the editor line height if not set
    pub line_height: Option<usize>,

    // These should be filled whenever you call into the `List` widget
    pub config: Arc<LapceConfig>,
}
impl<T: Clone, D: Data> ListData<T, D> {
    pub fn new(
        config: Arc<LapceConfig>,
        parent: WidgetId,
        held_data: D,
    ) -> ListData<T, D> {
        ListData {
            parent,
            items: im::Vector::new(),
            data: held_data,
            selected_index: 0,
            max_displayed_items: 15,
            line_height: None,
            config,
        }
    }

    /// Clone the list data, giving it data needed to update it  
    /// This is typically what you need to use to ensure that it has the
    /// appropriately updated data when passing the data to the list's widget functions    
    /// Note that due to the usage of `Arc` and `im::Vector`, cloning is relatively cheap.
    pub fn clone_with(&self, config: Arc<LapceConfig>) -> ListData<T, D> {
        let mut data = self.clone();
        data.update_data(config);

        data
    }

    pub fn update_data(&mut self, config: Arc<LapceConfig>) {
        self.config = config;
    }

    pub fn line_height(&self) -> usize {
        self.line_height
            .unwrap_or_else(|| self.config.ui.list_line_height())
    }

    /// The maximum number of items in the list that can be displayed  
    /// This is limited by `max_displayed_items` *or* by the number of items
    pub fn max_display_count(&self) -> usize {
        let mut count = 0;
        for _ in self.items.iter() {
            count += 1;
            if count >= self.max_displayed_items {
                return self.max_displayed_items;
            }
        }

        count
    }

    pub fn clear_items(&mut self) {
        self.items.clear();
        self.selected_index = 0;
    }

    /// Run a command, like those received from KeyPressFocus  
    pub fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &LapceCommand,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Focus(cmd) => self.run_focus_command(ctx, cmd),
            _ => CommandExecuted::No,
        }
    }

    pub fn run_focus_command(
        &mut self,
        ctx: &mut EventCtx,
        command: &FocusCommand,
    ) -> CommandExecuted {
        match command {
            // ModalClose should be handled (if desired) by the containing widget
            FocusCommand::ListNext => {
                self.next();
            }
            FocusCommand::ListNextPage => {
                self.next_page();
            }
            FocusCommand::ListPrevious => {
                self.previous();
            }
            FocusCommand::ListPreviousPage => {
                self.previous_page();
            }
            FocusCommand::ListSelect => {
                self.select(ctx);
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    // TODO: Option for whether moving should be wrapping

    pub fn next(&mut self) {
        self.selected_index = Movement::Down.update_index(
            self.selected_index,
            self.items.len(),
            1,
            true,
        );
    }

    pub fn next_page(&mut self) {
        self.selected_index = Movement::Down.update_index(
            self.selected_index,
            self.items.len(),
            self.max_displayed_items - 1,
            false,
        );
    }

    pub fn previous(&mut self) {
        self.selected_index = Movement::Up.update_index(
            self.selected_index,
            self.items.len(),
            1,
            true,
        );
    }

    pub fn previous_page(&mut self) {
        self.selected_index = Movement::Up.update_index(
            self.selected_index,
            self.items.len(),
            self.max_displayed_items - 1,
            false,
        );
    }

    pub fn select(&self, ctx: &mut EventCtx) {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ListItemSelected,
            Target::Widget(self.parent),
        ));
    }

    pub fn current_selected_item(&self) -> Option<&T> {
        self.items.get(self.selected_index)
    }
}
impl<T: Clone + PartialEq + 'static, D: Data> Data for ListData<T, D> {
    fn same(&self, other: &Self) -> bool {
        // We don't compare the held Config, because that should be updated whenever
        // the widget is used

        self.parent == other.parent
            && self.items == other.items
            && self.data.same(&other.data)
            && self.selected_index.same(&other.selected_index)
            && self.max_displayed_items.same(&other.max_displayed_items)
            && self.line_height.same(&other.line_height)
    }
}
impl<T: Clone + PartialEq + 'static, D: Data> GetConfig for ListData<T, D> {
    fn get_config(&self) -> &LapceConfig {
        &self.config
    }
}
