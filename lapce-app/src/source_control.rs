use std::{path::PathBuf, rc::Rc};

use floem::{
    keyboard::ModifiersState,
    reactive::{RwSignal, Scope},
};
use indexmap::IndexMap;
use lapce_core::mode::Mode;
use lapce_rpc::source_control::FileDiff;

use crate::{
    command::{CommandExecuted, CommandKind},
    editor::EditorData,
    id::EditorId,
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct SourceControlData {
    // VCS modified files & whether they should be included in the next commit
    pub file_diffs: RwSignal<IndexMap<PathBuf, (FileDiff, bool)>>,
    pub branch: RwSignal<String>,
    pub branches: RwSignal<im::Vector<String>>,
    pub tags: RwSignal<im::Vector<String>>,
    pub editor: Rc<EditorData>,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for SourceControlData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(
            condition,
            Condition::PanelFocus | Condition::SourceControlFocus
        )
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                self.editor.run_command(command, count, mods)
            }
            _ => CommandExecuted::No,
        }
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}

impl SourceControlData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        Self {
            file_diffs: cx.create_rw_signal(IndexMap::new()),
            branch: cx.create_rw_signal("".to_string()),
            branches: cx.create_rw_signal(im::Vector::new()),
            tags: cx.create_rw_signal(im::Vector::new()),
            editor: Rc::new(EditorData::new_local(
                cx,
                EditorId::next(),
                common.clone(),
            )),
            common,
        }
    }

    pub fn commit(&self) {
        let diffs: Vec<FileDiff> = self.file_diffs.with_untracked(|file_diffs| {
            file_diffs
                .iter()
                .filter_map(
                    |(_, (diff, checked))| {
                        if *checked {
                            Some(diff)
                        } else {
                            None
                        }
                    },
                )
                .cloned()
                .collect()
        });
        if diffs.is_empty() {
            return;
        }

        let message = self
            .editor
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        let message = message.trim();
        if message.is_empty() {
            return;
        }

        self.editor.reset();
        self.common.proxy.git_commit(message.to_string(), diffs);
    }
}
