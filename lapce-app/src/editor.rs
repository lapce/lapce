use std::{path::PathBuf, sync::Arc};

use floem::{
    app::AppContext,
    glazier::Modifiers,
    reactive::{create_rw_signal, ReadSignal, RwSignal},
};
use lapce_core::{
    command::EditCommand,
    cursor::{Cursor, CursorMode},
    mode::Mode,
    register::Register,
    selection::Selection,
};

use crate::{
    command::CommandExecuted, config::LapceConfig, doc::Document,
    keypress::KeyPressFocus,
};

#[derive(Clone)]
pub struct EditorData {
    pub doc: RwSignal<Document>,
    pub cursor: RwSignal<Cursor>,
    register: RwSignal<Register>,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl EditorData {
    pub fn new(
        cx: AppContext,
        doc: RwSignal<Document>,
        register: RwSignal<Register>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let cursor =
            Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None);
        let cursor = create_rw_signal(cx.scope, cursor);
        Self {
            doc,
            cursor,
            register,
            config,
        }
    }

    pub fn new_local(
        cx: AppContext,
        register: RwSignal<Register>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let doc = Document::new_local(cx, config);
        let doc = create_rw_signal(cx.scope, doc);
        let cursor =
            Cursor::new(CursorMode::Insert(Selection::caret(0)), None, None);
        let cursor = create_rw_signal(cx.scope, cursor);
        Self {
            doc,
            cursor,
            register,
            config,
        }
    }

    fn run_edit_command(&self, cmd: &EditCommand) -> CommandExecuted {
        let modal = self.config.with(|config| config.core.modal)
            && !self.doc.with(|doc| doc.content.is_local());
        let mut cursor = self.cursor.get();
        self.doc.update(|doc| {
            self.register.update(|register| {
                let yank_data =
                    if let lapce_core::cursor::CursorMode::Visual { .. } =
                        &cursor.mode
                    {
                        Some(cursor.yank(doc.buffer()))
                    } else {
                        None
                    };
                let deltas = doc.do_edit(&mut cursor, cmd, modal, register);
                if !deltas.is_empty() {
                    if let Some(data) = yank_data {
                        register.add_delete(data);
                    }
                }
            });
        });
        self.cursor.set(cursor);
        CommandExecuted::Yes
    }

    fn run_move_command(
        &self,
        movement: &lapce_core::movement::Movement,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        let mut cursor = self.cursor.get();
        let config = self.config.get();
        self.doc.update(|doc| {
            self.register.update(|register| {
                doc.move_cursor(
                    &mut cursor,
                    movement,
                    count.unwrap_or(1),
                    mods.shift(),
                    register,
                    &config,
                );
            });
        });
        self.cursor.set(cursor);
        CommandExecuted::Yes
    }
}

impl KeyPressFocus for EditorData {
    fn get_mode(&self) -> lapce_core::mode::Mode {
        self.cursor.with(|c| c.get_mode())
    }

    fn check_condition(
        &self,
        condition: crate::keypress::condition::Condition,
    ) -> bool {
        false
    }

    fn run_command(
        &self,
        cx: AppContext,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: floem::glazier::Modifiers,
    ) -> crate::command::CommandExecuted {
        match &command.kind {
            crate::command::CommandKind::Workbench(_) => CommandExecuted::No,
            crate::command::CommandKind::Edit(cmd) => self.run_edit_command(cmd),
            crate::command::CommandKind::Move(cmd) => {
                let movement = cmd.to_movement(count);
                self.run_move_command(&movement, count, mods)
            }
            crate::command::CommandKind::Focus(_) => CommandExecuted::No,
            crate::command::CommandKind::MotionMode(_) => CommandExecuted::No,
            crate::command::CommandKind::MultiSelection(_) => CommandExecuted::No,
        }
    }

    fn receive_char(&self, cx: AppContext, c: &str) {
        if self.get_mode() == Mode::Insert {
            let mut cursor = self.cursor.get();
            let config = self.config.get();
            self.doc.update(|doc| {
                let deltas = doc.do_insert(&mut cursor, c, &config);
            });
            self.cursor.set(cursor);
        }
    }
}
