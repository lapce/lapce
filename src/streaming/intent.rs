//! Intent router for MCP ActiveInvoker — classify user queries into Review /
//! Refactor / Debug / Complete / Build / Test and route to MCP tool bundles.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent { Review, Refactor, Debug, Complete, Build, Test }

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}

impl Intent {
    pub fn detect(query: &str) -> Option<Intent> {
        let q = query.to_ascii_lowercase();
        let table: &[(&[&str], Intent)] = &[
            (&["review", "code review", "审阅", "审查"], Intent::Review),
            (&["refactor", "重构"], Intent::Refactor),
            (&["debug", "bug", "fix", "调试", "修复", "问题"], Intent::Debug),
            (&["complete", "autocomplete", "补全", "完成"], Intent::Complete),
            (&["build", "compile", "编译", "构建"], Intent::Build),
            (&["test", "pytest", "cargo test", "测试"], Intent::Test),
        ];
        for (pat, intent) in table.iter() { for p in pat.iter() { if q.contains(p) { return Some(*intent); } } }
        None
    }
    pub fn route_tools(&self) -> &'static [&'static str] {
        match self {
            Intent::Review => &["code_review", "security_scan", "find_smells"],
            Intent::Refactor => &["refactor_plan", "extract_function", "rename_symbol"],
            Intent::Debug => &["diagnose", "stack_trace_explain", "find_broken_refs"],
            Intent::Complete => &["completion_trigger", "signature_help", "hover_doc"],
            Intent::Build => &["build_check", "cargo_check", "compile_watch"],
            Intent::Test => &["test_run", "test_coverage", "test_failing"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_detect_review() { assert_eq!(Intent::detect("please review my code changes"), Some(Intent::Review)); }
    #[test] fn test_detect_refactor_zh() { assert_eq!(Intent::detect("帮我重构这个函数"), Some(Intent::Refactor)); }
    #[test] fn test_detect_none() { assert_eq!(Intent::detect("hello world"), None); }
    #[test] fn test_route_tools_nonempty() { assert!(Intent::Debug.route_tools().len() >= 1); }
}
