use std::sync::Arc;

use floem::{
    peniko::kurbo::Rect,
    reactive::{create_rw_signal, RwSignal, Scope, SignalGetUntracked, SignalSet},
};
use lapce_core::{command::FocusCommand, mode::Mode, movement::Movement};
use lapce_rpc::plugin::PluginId;
use lsp_types::CodeActionOrCommand;

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand},
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::{CommonData, Focus},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CodeActionStatus {
    Inactive,
    Active,
}

#[derive(Clone, PartialEq)]
pub struct ScoredCodeActionItem {
    pub item: CodeActionOrCommand,
    pub plugin_id: PluginId,
    pub score: i64,
    pub indices: Vec<usize>,
}

impl ScoredCodeActionItem {
    pub fn title(&self) -> &str {
        match &self.item {
            CodeActionOrCommand::Command(c) => &c.title,
            CodeActionOrCommand::CodeAction(c) => &c.title,
        }
    }
}

#[derive(Clone)]
pub struct CodeActionData {
    pub status: RwSignal<CodeActionStatus>,
    pub active: RwSignal<usize>,
    pub request_id: usize,
    pub input_id: usize,
    pub offset: usize,
    pub items: im::Vector<ScoredCodeActionItem>,
    pub filtered_items: im::Vector<ScoredCodeActionItem>,
    pub layout_rect: Rect,
    pub mouse_click: bool,
    pub common: CommonData,
}

impl KeyPressFocus for CodeActionData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::ListFocus | Condition::ModalFocus)
    }

    fn run_command(
        &self,
        cx: Scope,
        command: &crate::command::LapceCommand,
        _count: Option<usize>,
        _mods: floem::glazier::Modifiers,
    ) -> crate::command::CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Edit(_) => {}
            CommandKind::Move(_) => {}
            CommandKind::Focus(cmd) => {
                self.run_focus_command(cx, cmd);
            }
            CommandKind::MotionMode(_) => {}
            CommandKind::MultiSelection(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, _cx: Scope, _c: &str) {}
}

impl CodeActionData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        let status = create_rw_signal(cx, CodeActionStatus::Inactive);
        let active = create_rw_signal(cx, 0);
        Self {
            status,
            active,
            request_id: 0,
            input_id: 0,
            offset: 0,
            items: im::Vector::new(),
            filtered_items: im::Vector::new(),
            layout_rect: Rect::ZERO,
            mouse_click: false,
            common,
        }
    }

    pub fn next(&self) {
        let active = self.active.get_untracked();
        let new =
            Movement::Down.update_index(active, self.filtered_items.len(), 1, true);
        self.active.set(new);
    }

    pub fn previous(&self) {
        let active = self.active.get_untracked();
        let new =
            Movement::Up.update_index(active, self.filtered_items.len(), 1, true);
        self.active.set(new);
    }

    pub fn next_page(&self) {
        let config = self.common.config.get_untracked();
        let count = ((self.layout_rect.size().height
            / config.editor.line_height() as f64)
            .floor() as usize)
            .saturating_sub(1);
        let active = self.active.get_untracked();
        let new = Movement::Down.update_index(
            active,
            self.filtered_items.len(),
            count,
            false,
        );
        self.active.set(new);
    }

    pub fn previous_page(&self) {
        let config = self.common.config.get_untracked();
        let count = ((self.layout_rect.size().height
            / config.editor.line_height() as f64)
            .floor() as usize)
            .saturating_sub(1);
        let active = self.active.get_untracked();
        let new = Movement::Up.update_index(
            active,
            self.filtered_items.len(),
            count,
            false,
        );
        self.active.set(new);
    }

    pub fn show(
        &mut self,
        code_actions: Arc<(PluginId, Vec<CodeActionOrCommand>)>,
        offset: usize,
        mouse_click: bool,
    ) {
        self.active.set(0);
        self.status.set(CodeActionStatus::Active);
        self.offset = offset;
        self.mouse_click = mouse_click;
        self.request_id += 1;
        self.items = code_actions
            .1
            .iter()
            .map(|code_action| ScoredCodeActionItem {
                item: code_action.clone(),
                plugin_id: code_actions.0,
                score: 0,
                indices: Vec::new(),
            })
            .collect();
        self.filtered_items = self.items.clone();
        self.common.focus.set(Focus::CodeAction);
    }

    fn cancel(&self, _cx: Scope) {
        self.status.set(CodeActionStatus::Inactive);
        self.common.focus.set(Focus::Workbench);
    }

    fn select(&self, cx: Scope) {
        if let Some(item) = self.filtered_items.get(self.active.get_untracked()) {
            self.common
                .internal_command
                .set(Some(InternalCommand::RunCodeAction {
                    plugin_id: item.plugin_id,
                    action: item.item.clone(),
                }));
        }
        self.cancel(cx);
    }

    fn run_focus_command(&self, cx: Scope, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            FocusCommand::ModalClose => {
                self.cancel(cx);
            }
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
                self.select(cx);
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }
}