use std::path::PathBuf;

use floem::{
    ext_event::create_ext_action,
    reactive::{
        create_effect, create_rw_signal, RwSignal, Scope, SignalGetUntracked,
        SignalSet, SignalUpdate, SignalWith,
    },
};
use indexmap::IndexMap;
use lapce_core::mode::Mode;
use lapce_rpc::proxy::{ProxyResponse, SearchMatch};

use crate::{
    command::{CommandExecuted, CommandKind},
    editor::EditorData,
    id::EditorId,
    keypress::{condition::Condition, KeyPressFocus},
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct SearchMatchData {
    pub expanded: RwSignal<bool>,
    pub matches: RwSignal<im::Vector<SearchMatch>>,
}

#[derive(Clone)]
pub struct GlobalSearchData {
    pub editor: EditorData,
    pub search_result: RwSignal<IndexMap<PathBuf, SearchMatchData>>,
    pub common: CommonData,
}

impl KeyPressFocus for GlobalSearchData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::PanelFocus)
    }

    fn run_command(
        &self,
        cx: Scope,
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
                self.editor.run_command(cx, command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, cx: Scope, c: &str) {
        self.editor.receive_char(cx, c);
    }
}

impl GlobalSearchData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        let editor = EditorData::new_local(cx, EditorId::next(), common.clone());
        let search_result = create_rw_signal(cx, IndexMap::new());

        let global_search = Self {
            editor,
            search_result,
            common,
        };

        {
            let global_search = global_search.clone();
            create_effect(cx, move |_| {
                let pattern = global_search
                    .editor
                    .doc
                    .with(|doc| doc.buffer().to_string());
                if pattern.is_empty() {
                    global_search.search_result.update(|r| r.clear());
                    return;
                }
                let case_sensitive = global_search.common.find.case_sensitive(true);
                let send = {
                    let global_search = global_search.clone();
                    create_ext_action(cx, move |result| {
                        if let Ok(ProxyResponse::GlobalSearchResponse { matches }) =
                            result
                        {
                            global_search.update_matches(matches);
                        }
                    })
                };
                global_search.common.proxy.global_search(
                    pattern,
                    case_sensitive,
                    move |result| {
                        send(result);
                    },
                );
            });
        }

        global_search
    }

    fn update_matches(&self, matches: IndexMap<PathBuf, Vec<SearchMatch>>) {
        let current = self.search_result.get_untracked();

        self.search_result.set(
            matches
                .into_iter()
                .map(|(path, matches)| {
                    let match_data =
                        current.get(&path).cloned().unwrap_or_else(|| {
                            SearchMatchData {
                                expanded: create_rw_signal(self.common.scope, true),
                                matches: create_rw_signal(
                                    self.common.scope,
                                    im::Vector::new(),
                                ),
                            }
                        });

                    match_data.matches.set(matches.into());

                    (path, match_data)
                })
                .collect(),
        );
    }
}
