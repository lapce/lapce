use std::rc::Rc;

use floem::{
    keyboard::Modifiers,
    peniko::kurbo::Rect,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
};
use lapce_core::{command::FocusCommand, mode::Mode, movement::Movement};
use lapce_rpc::plugin::PluginId;
use lsp_types::CodeActionOrCommand;

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand},
    keypress::{KeyPressFocus, condition::Condition},
    window_tab::{CommonData, Focus},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CodeActionStatus {
    Inactive,
    Active,
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug)]
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
    pub common: Rc<CommonData>,
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
        command: &crate::command::LapceCommand,
        _count: Option<usize>,
        _mods: Modifiers,
    ) -> crate::command::CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Edit(_) => {}
            CommandKind::Move(_) => {}
            CommandKind::Scroll(_) => {}
            CommandKind::Focus(cmd) => {
                self.run_focus_command(cmd);
            }
            CommandKind::MotionMode(_) => {}
            CommandKind::MultiSelection(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, _c: &str) {}
}

impl CodeActionData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        let status = cx.create_rw_signal(CodeActionStatus::Inactive);
        let active = cx.create_rw_signal(0);

        let code_action = Self {
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
        };

        {
            let code_action = code_action.clone();
            cx.create_effect(move |_| {
                let focus = code_action.common.focus.get();
                if focus != Focus::CodeAction
                    && code_action.status.get_untracked()
                        != CodeActionStatus::Inactive
                {
                    code_action.cancel();
                }
            })
        }

        code_action
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
        plugin_id: PluginId,
        code_actions: im::Vector<CodeActionOrCommand>,
        offset: usize,
        mouse_click: bool,
    ) {
        self.active.set(0);
        self.status.set(CodeActionStatus::Active);
        self.offset = offset;
        self.mouse_click = mouse_click;
        self.request_id += 1;
        self.items = code_actions
            .into_iter()
            .map(|code_action| ScoredCodeActionItem {
                item: code_action,
                plugin_id,
                score: 0,
                indices: Vec::new(),
            })
            .collect();
        self.filtered_items = self.items.clone();
        self.common.focus.set(Focus::CodeAction);
    }

    fn cancel(&self) {
        self.status.set(CodeActionStatus::Inactive);
        self.common.focus.set(Focus::Workbench);
    }

    pub fn select(&self) {
        if let Some(item) = self.filtered_items.get(self.active.get_untracked()) {
            self.common
                .internal_command
                .send(InternalCommand::RunCodeAction {
                    plugin_id: item.plugin_id,
                    action: item.item.clone(),
                });
        }
        self.cancel();
    }

    fn run_focus_command(&self, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            FocusCommand::ModalClose => {
                self.cancel();
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
                self.select();
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }
}
