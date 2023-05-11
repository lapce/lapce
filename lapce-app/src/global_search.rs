use std::path::PathBuf;

use floem::reactive::{create_rw_signal, RwSignal, Scope};
use indexmap::IndexMap;
use lapce_rpc::proxy::SearchMatch;

use crate::{editor::EditorData, id::EditorId, window_tab::CommonData};

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

impl GlobalSearchData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        let editor = EditorData::new_local(cx, EditorId::next(), common.clone());
        let search_result = create_rw_signal(cx, IndexMap::new());
        Self {
            editor,
            search_result,
            common,
        }
    }
}
