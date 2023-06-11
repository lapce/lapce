use std::path::PathBuf;

use floem::reactive::{create_rw_signal, RwSignal, Scope};
use indexmap::IndexMap;
use lapce_rpc::source_control::FileDiff;

use crate::window_tab::CommonData;

#[derive(Clone)]
pub struct SourceControlData {
    // VCS modified files & whether they should be included in the next commit
    pub file_diffs: RwSignal<IndexMap<PathBuf, (FileDiff, bool)>>,
    pub branch: RwSignal<String>,
    pub branches: RwSignal<im::Vector<String>>,
    pub common: CommonData,
}

impl SourceControlData {
    pub fn new(cx: Scope, common: CommonData) -> Self {
        Self {
            file_diffs: create_rw_signal(cx, IndexMap::new()),
            branch: create_rw_signal(cx, "".to_string()),
            branches: create_rw_signal(cx, im::Vector::new()),
            common,
        }
    }
}
