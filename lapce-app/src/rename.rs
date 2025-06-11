use std::{path::PathBuf, rc::Rc};

use floem::{
    ext_event::create_ext_action,
    keyboard::Modifiers,
    peniko::kurbo::Rect,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use lapce_core::{command::FocusCommand, mode::Mode, selection::Selection};
use lapce_rpc::proxy::ProxyResponse;
use lapce_xi_rope::Rope;
use lsp_types::Position;

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand, LapceCommand},
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::Editors,
    window_tab::{CommonData, Focus},
};

#[derive(Clone, Debug)]
pub struct RenameData {
    pub active: RwSignal<bool>,
    pub editor: EditorData,
    pub start: RwSignal<usize>,
    pub position: RwSignal<Position>,
    pub path: RwSignal<PathBuf>,
    pub layout_rect: RwSignal<Rect>,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for RenameData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::RenameFocus | Condition::ModalFocus)
    }

    fn run_command(
        &self,
        command: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Scroll(_) => {}
            CommandKind::Focus(cmd) => {
                self.run_focus_command(cmd);
            }
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                self.editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}

impl RenameData {
    pub fn new(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Self {
        let active = cx.create_rw_signal(false);
        let start = cx.create_rw_signal(0);
        let position = cx.create_rw_signal(Position::default());
        let layout_rect = cx.create_rw_signal(Rect::ZERO);
        let path = cx.create_rw_signal(PathBuf::new());
        let editor = editors.make_local(cx, common.clone());
        Self {
            active,
            editor,
            start,
            position,
            layout_rect,
            path,
            common,
        }
    }

    pub fn start(
        &self,
        path: PathBuf,
        placeholder: String,
        start: usize,
        position: Position,
    ) {
        self.editor.doc().reload(Rope::from(&placeholder), true);
        self.editor.cursor().update(|cursor| {
            cursor.set_insert(Selection::region(0, placeholder.len()))
        });
        self.path.set(path);
        self.start.set(start);
        self.position.set(position);
        self.active.set(true);
        self.common.focus.set(Focus::Rename);
    }

    fn run_focus_command(&self, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            FocusCommand::ModalClose => {
                self.cancel();
            }
            FocusCommand::ConfirmRename => {
                self.confirm();
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn cancel(&self) {
        self.active.set(false);
        if let Focus::Rename = self.common.focus.get_untracked() {
            self.common.focus.set(Focus::Workbench);
        }
    }

    fn confirm(&self) {
        let new_name = self
            .editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        let new_name = new_name.trim();
        if !new_name.is_empty() {
            let path = self.path.get_untracked();
            let position = self.position.get_untracked();
            let internal_command = self.common.internal_command;
            let send = create_ext_action(self.common.scope, move |result| {
                if let Ok(ProxyResponse::Rename { edit }) = result {
                    internal_command
                        .send(InternalCommand::ApplyWorkspaceEdit { edit });
                }
            });
            self.common.proxy.rename(
                path,
                position,
                new_name.to_string(),
                move |result| {
                    send(result);
                },
            );
        }
        self.cancel();
    }
}
