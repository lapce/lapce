use std::{ops::Range, path::PathBuf};

use floem::{
    ext_event::create_ext_action,
    reactive::{
        create_effect, create_rw_signal, Memo, RwSignal, Scope, SignalGet,
        SignalGetUntracked, SignalSet, SignalUpdate, SignalWith,
    },
    views::VirtualListVector,
};
use indexmap::IndexMap;
use lapce_core::{mode::Mode, selection::Selection};
use lapce_rpc::proxy::{ProxyResponse, SearchMatch};
use lapce_xi_rope::Rope;

use crate::{
    command::{CommandExecuted, CommandKind},
    editor::EditorData,
    id::EditorId,
    keypress::{condition::Condition, KeyPressFocus},
    main_split::MainSplitData,
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct SearchMatchData {
    pub expanded: RwSignal<bool>,
    pub matches: RwSignal<im::Vector<SearchMatch>>,
    pub line_height: Memo<f64>,
}

impl SearchMatchData {
    pub fn height(&self) -> f64 {
        let line_height = self.line_height.get();
        let count = if self.expanded.get() {
            self.matches.with(|m| m.len()) + 1
        } else {
            1
        };
        line_height * count as f64
    }
}

#[derive(Clone)]
pub struct GlobalSearchData {
    pub editor: EditorData,
    pub search_result: RwSignal<IndexMap<PathBuf, SearchMatchData>>,
    pub main_split: MainSplitData,
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

impl VirtualListVector<(PathBuf, SearchMatchData)> for GlobalSearchData {
    type ItemIterator = Box<dyn Iterator<Item = (PathBuf, SearchMatchData)>>;

    fn total_len(&self) -> usize {
        0
    }

    fn total_size(&self) -> Option<f64> {
        let line_height = self.common.ui_line_height.get();
        let count: usize = self.search_result.with(|result| {
            result
                .iter()
                .map(|(_, data)| {
                    if data.expanded.get() {
                        data.matches.with(|m| m.len()) + 1
                    } else {
                        1
                    }
                })
                .sum()
        });
        Some(line_height * count as f64)
    }

    fn slice(&mut self, _range: Range<usize>) -> Self::ItemIterator {
        Box::new(self.search_result.get().into_iter())
    }
}

impl GlobalSearchData {
    pub fn new(cx: Scope, main_split: MainSplitData, common: CommonData) -> Self {
        let editor = EditorData::new_local(cx, EditorId::next(), common.clone());
        let search_result = create_rw_signal(cx, IndexMap::new());

        let global_search = Self {
            editor,
            search_result,
            main_split,
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
                let whole_word = global_search.common.find.whole_words.get();
                let is_regex = global_search.common.find.is_regex.get();
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
                    whole_word,
                    is_regex,
                    move |result| {
                        send(result);
                    },
                );
            });
        }

        {
            let global_search_doc = global_search.editor.doc;
            let main_split = global_search.main_split.clone();
            create_effect(cx, move |_| {
                let content = global_search_doc.with(|doc| doc.buffer().to_string());
                main_split.set_find_pattern(Some(content));
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
                                line_height: self.common.ui_line_height,
                            }
                        });

                    match_data.matches.set(matches.into());

                    (path, match_data)
                })
                .collect(),
        );
    }

    pub fn set_pattern(&self, pattern: String) {
        let pattern_len = pattern.len();
        self.editor
            .doc
            .update(|doc| doc.reload(Rope::from(pattern), true));
        self.editor
            .cursor
            .update(|cursor| cursor.set_insert(Selection::region(0, pattern_len)));
    }
}
