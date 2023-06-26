use std::path::PathBuf;

use floem::reactive::{create_rw_signal, RwSignal, Scope, SignalWithUntracked};
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
    pub editor: EditorData,
    pub common: CommonData,
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
        mods: floem::glazier::Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Focus(_) => {}
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

impl SourceControlData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        Self {
            file_diffs: create_rw_signal(cx, IndexMap::new()),
            branch: create_rw_signal(cx, "".to_string()),
            branches: create_rw_signal(cx, im::Vector::new()),
            tags: create_rw_signal(cx, im::Vector::new()),
            editor: EditorData::new_local(cx, EditorId::next(), common.clone()),
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
            .doc
            .with_untracked(|doc| doc.buffer().to_string());
        let message = message.trim();
        if message.is_empty() {
            return;
        }

        self.editor.reset();
        self.common.proxy.git_commit(message.to_string(), diffs);
    }
}
