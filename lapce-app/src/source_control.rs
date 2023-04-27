use std::path::PathBuf;

use floem::reactive::Scope;
use indexmap::IndexMap;
use lapce_rpc::source_control::FileDiff;

use crate::window_tab::CommonData;

#[derive(Clone)]
pub struct SourceControlData {
    // VCS modified files & whether they should be included in the next commit
    pub file_diffs: IndexMap<PathBuf, (FileDiff, bool)>,
    pub branch: String,
    pub branches: im::Vector<String>,
    pub common: CommonData,
}

impl SourceControlData {
    pub fn new(_cx: Scope, common: CommonData) -> Self {
        Self {
            file_diffs: IndexMap::new(),
            branch: "".to_string(),
            branches: im::Vector::new(),
            common,
        }
    }
}
