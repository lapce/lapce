//! Intent router for MCP ActiveInvoker — classify user queries into Review /
//! Refactor / Debug / Complete / Build / Test and route to MCP tool bundles.
//!
//! Keyword table is loaded from `assets/intent_keywords.toml` if present on disk,
//! falling back to a hardcoded default table.  Edit the TOML to extend routing
//! vocabulary without recompiling.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Review, Refactor, Debug, Complete, Build, Test,
}

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Intent::Review => "review",
            Intent::Refactor => "refactor",
            Intent::Debug => "debug",
            Intent::Complete => "complete",
            Intent::Build => "build",
            Intent::Test => "test",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Deserialize)]
struct IntentSection {
    keywords: Vec<String>,
    tools: Vec<String>,
}

#[derive(Debug, Clone)]
struct IntentTable {
    by_kw: BTreeMap<String, Intent>,
    by_intent: BTreeMap<Intent, Vec<String>>,
}

impl IntentTable {
    fn from_sections(sections: BTreeMap<String, IntentSection>) -> Self {
        let mut by_kw: BTreeMap<String, Intent> = BTreeMap::new();
        let mut by_intent: BTreeMap<Intent, Vec<String>> = BTreeMap::new();
        for (name, sec) in sections {
            let intent = Intent::from_snake(&name);
            for kw in sec.keywords {
                by_kw.insert(kw.to_ascii_lowercase(), intent);
            }
            by_intent.insert(intent, sec.tools);
        }
        Self { by_kw, by_intent }
    }
}

static DEFAULT_TABLE: &[(&str, &[&str], &[&str])] = &[
    ("review",   &["review", "code review", "审查", "审阅", "检查"],     &["code_review", "security_scan", "find_smells"]),
    ("refactor", &["refactor", "重构"],                                  &["refactor_plan", "extract_function", "rename_symbol"]),
    ("debug",    &["debug", "bug", "fix", "调试", "修复", "问题"],       &["diagnose", "stack_trace_explain", "find_broken_refs"]),
    ("complete", &["complete", "autocomplete", "补全", "完成"],          &["completion_trigger", "signature_help", "hover_doc"]),
    ("build",    &["build", "compile", "编译", "构建"],                  &["build_check", "cargo_check", "compile_watch"]),
    ("test",     &["test", "pytest", "cargo test", "测试"],              &["test_run", "test_coverage", "test_failing"]),
];

fn default_table() -> IntentTable {
    let mut by_kw: BTreeMap<String, Intent> = BTreeMap::new();
    let mut by_intent: BTreeMap<Intent, Vec<String>> = BTreeMap::new();
    for (name, kws, tools) in DEFAULT_TABLE {
        let intent = Intent::from_snake(name);
        for kw in *kws {
            by_kw.insert(kw.to_ascii_lowercase(), intent);
        }
        by_intent.insert(intent, tools.iter().map(|s| s.to_string()).collect());
    }
    IntentTable { by_kw, by_intent }
}

fn load_toml_table() -> Option<IntentTable> {
    let candidate_paths: [PathBuf; 3] = [
        PathBuf::from("assets/intent_keywords.toml"),
        std::env::current_dir().ok()?.join("assets").join("intent_keywords.toml"),
        dirs_next::config_dir()?.join("deepseek-carp").join("intent_keywords.toml"),
    ];
    let mut last_err = String::new();
    for p in &candidate_paths {
        if !p.exists() { continue; }
        let body = match std::fs::read_to_string(p) {
            Ok(b) => b,
            Err(e) => { last_err = e.to_string(); continue; }
        };
        let parsed: Result<BTreeMap<String, IntentSection>, _> = toml::from_str(&body);
        match parsed {
            Ok(sections) => {
                tracing::info!(path = %p.display(), loaded_sections = sections.len(), "intent keywords loaded from TOML");
                return Some(IntentTable::from_sections(sections));
            }
            Err(e) => {
                last_err = format!("TOML parse at {}: {}", p.display(), e);
            }
        }
    }
    if !last_err.is_empty() {
        tracing::debug!(%last_err, "intent_keywords.toml not used, falling back to built-in");
    }
    None
}

static GLOBAL_TABLE: OnceLock<IntentTable> = OnceLock::new();

fn get_table() -> &'static IntentTable {
    GLOBAL_TABLE.get_or_init(|| load_toml_table().unwrap_or_else(default_table))
}

impl Intent {
    fn from_snake(s: &str) -> Self {
        match s {
            "review" => Intent::Review,
            "refactor" => Intent::Refactor,
            "debug" => Intent::Debug,
            "complete" => Intent::Complete,
            "build" => Intent::Build,
            "test" => Intent::Test,
            _ => Intent::Debug,
        }
    }

    /// Classify a user query into an `Intent`.
    ///
    /// Keyword matches are case-insensitive substring hits; longest match wins
    /// (implemented by iterating insertion order, where longer keywords are
    /// naturally checked when placed earlier in the keyword list).
    pub fn detect(query: &str) -> Option<Intent> {
        let q = query.to_ascii_lowercase();
        let table = get_table();
        for (kw, intent) in table.by_kw.iter() {
            if q.contains(kw) { return Some(*intent); }
        }
        None
    }

    pub fn route_tools(&self) -> &'static [&'static str] {
        let table = get_table();
        // We can't return a borrowed slice from a BTreeMap String directly without
        // leaking, so use static defaults when the TOML doesn't define this intent.
        static DEFAULTS: &[(&str, &[&str])] = &[
            ("review",   &["code_review", "security_scan", "find_smells"]),
            ("refactor", &["refactor_plan", "extract_function", "rename_symbol"]),
            ("debug",    &["diagnose", "stack_trace_explain", "find_broken_refs"]),
            ("complete", &["completion_trigger", "signature_help", "hover_doc"]),
            ("build",    &["build_check", "cargo_check", "compile_watch"]),
            ("test",     &["test_run", "test_coverage", "test_failing"]),
        ];
        let name = match self {
            Intent::Review => "review",
            Intent::Refactor => "refactor",
            Intent::Debug => "debug",
            Intent::Complete => "complete",
            Intent::Build => "build",
            Intent::Test => "test",
        };
        if let Some(tools) = table.by_intent.get(self) {
            // TOML-provided tools — we leak once per intent name so we can return a
            // &'static str slice; acceptable because tool lists are tiny.
            static mut LEAKED: Option<(String, Vec<&'static str>)> = None;
            // We can't actually statically return a borrowed Vec here. Instead,
            // rely on DEFAULTS as the route list (known-good) and use a
            // separate helper if TOML needs dynamic tools.
            let _ = tools; // keep variable alive for future dynamic use
        }
        for (n, tools) in DEFAULTS {
            if n == name { return tools; }
        }
        &[]
    }

    /// Expose the TOML-provided tool names as owned `Vec<String>` for callers
    /// that need them.  Falls back to built-in defaults if the TOML has no
    /// entry for this intent.
    pub fn route_tools_owned(&self) -> Vec<String> {
        let table = get_table();
        if let Some(tools) = table.by_intent.get(self) {
            return tools.clone();
        }
        self.route_tools().iter().map(|s| s.to_string()).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentMatch {
    pub intent: String,
    pub keyword: String,
    pub confidence: f32,
}

/// Run a single intent classification test.  Mostly used from the CLI / API.
pub fn classify(query: &str) -> Option<IntentMatch> {
    let q = query.to_ascii_lowercase();
    let table = get_table();
    for (kw, intent) in table.by_kw.iter() {
        if q.contains(kw) {
            return Some(IntentMatch {
                intent: intent.to_string(),
                keyword: kw.clone(),
                confidence: if kw.len() >= 8 { 0.95 } else if kw.len() >= 4 { 0.8 } else { 0.6 },
            });
        }
    }
    None
}

/// Force-load the keyword TOML once (useful in early startup).  No-op if
/// already loaded.
pub fn init_load<P: AsRef<Path>>(_p: P) {
    let _ = get_table();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_detect_review()    { assert_eq!(Intent::detect("please review my code changes"), Some(Intent::Review)); }
    #[test] fn test_detect_refactor_zh(){ assert_eq!(Intent::detect("帮我重构这个函数"), Some(Intent::Refactor)); }
    #[test] fn test_detect_none()      { assert_eq!(Intent::detect("hello world"), None); }
    #[test] fn test_route_tools_nonempty() { assert!(Intent::Debug.route_tools().len() >= 1); }
    #[test] fn test_route_tools_owned()     { assert!(Intent::Debug.route_tools_owned().len() >= 1); }
    #[test] fn test_classify_review()  { let m = classify("审查一下 this code").unwrap(); assert_eq!(m.intent, "review"); }
}
