use anyhow::Context;
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use libloading::{Library, Symbol};
use std::str::FromStr;
use std::{collections::HashMap, path::PathBuf};
use tree_sitter::{Language, Parser};
use tree_sitter_highlight::HighlightConfiguration;

#[cfg(unix)]
const DYLIB_EXTENSION: &str = "so";

#[cfg(windows)]
const DYLIB_EXTENSION: &str = "dll";

pub const QUERIES_DIR: Dir = include_dir!("../runtime/queries");
lazy_static! {
    pub static ref SCOPES: Vec<String> = vec![
        "constant".to_string(),
        "constant.builtin".to_string(),
        "type".to_string(),
        "type.builtin".to_string(),
        "property".to_string(),
        "comment".to_string(),
        "constructor".to_string(),
        "function".to_string(),
        "function.method".to_string(),
        "function.macro".to_string(),
        "punctuation.bracket".to_string(),
        "punctuation.delimiter".to_string(),
        "label".to_string(),
        "keyword".to_string(),
        "string".to_string(),
        "variable.parameter".to_string(),
        "variable.builtin".to_string(),
        "variable.other.member".to_string(),
        "operator".to_string(),
        "attribute".to_string(),
        "escape".to_string(),
    ];
}

#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub enum LapceLanguage {
    Rust,
    Toml,
    Javascript,
    Go,
    Yaml,
}

impl LapceLanguage {
    pub fn from_path(path: &PathBuf) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?;
        Some(match extension {
            "rs" => LapceLanguage::Rust,
            "toml" => LapceLanguage::Toml,
            "js" => LapceLanguage::Javascript,
            "jsx" => LapceLanguage::Javascript,
            "go" => LapceLanguage::Go,
            "yaml" => LapceLanguage::Yaml,
            "yml" => LapceLanguage::Yaml,
            _ => return None,
        })
    }
}

pub struct TreeSitter {
    parsers: HashMap<LapceLanguage, Parser>,
}

pub fn new_highlight_config(language: LapceLanguage) -> HighlightConfiguration {
    match language {
        LapceLanguage::Rust => {
            let mut configuration = HighlightConfiguration::new(
                unsafe { tree_sitter_rust() },
                QUERIES_DIR
                    .get_file("rust/highlights.scm")
                    .unwrap()
                    .contents_utf8()
                    .unwrap(),
                "",
                "",
            )
            .unwrap();
            configuration.configure(&SCOPES);
            configuration
        }
        LapceLanguage::Toml => {
            let mut configuration = HighlightConfiguration::new(
                unsafe { tree_sitter_toml() },
                QUERIES_DIR
                    .get_file("toml/highlights.scm")
                    .unwrap()
                    .contents_utf8()
                    .unwrap(),
                "",
                "",
            )
            .unwrap();
            configuration.configure(&SCOPES);
            configuration
        }
        LapceLanguage::Javascript => {
            let mut configuration = HighlightConfiguration::new(
                unsafe { tree_sitter_javascript() },
                QUERIES_DIR
                    .get_file("javascript/highlights.scm")
                    .unwrap()
                    .contents_utf8()
                    .unwrap(),
                "",
                "",
            )
            .unwrap();
            configuration.configure(&SCOPES);
            configuration
        }
        LapceLanguage::Yaml => {
            let mut configuration = HighlightConfiguration::new(
                unsafe { tree_sitter_yaml() },
                QUERIES_DIR
                    .get_file("yaml/highlights.scm")
                    .unwrap()
                    .contents_utf8()
                    .unwrap(),
                "",
                "",
            )
            .unwrap();
            configuration.configure(&SCOPES);
            configuration
        }
        LapceLanguage::Go => {
            let mut configuration = HighlightConfiguration::new(
                unsafe { tree_sitter_go() },
                QUERIES_DIR
                    .get_file("go/highlights.scm")
                    .unwrap()
                    .contents_utf8()
                    .unwrap(),
                "",
                "",
            )
            .unwrap();
            configuration.configure(&SCOPES);
            configuration
        }
    }
}

pub fn new_parser(language: LapceLanguage) -> Parser {
    let language = match language {
        LapceLanguage::Rust => unsafe { tree_sitter_rust() },
        LapceLanguage::Toml => unsafe { tree_sitter_toml() },
        LapceLanguage::Javascript => unsafe { tree_sitter_javascript() },
        LapceLanguage::Go => unsafe { tree_sitter_go() },
        LapceLanguage::Yaml => unsafe { tree_sitter_yaml() },
    };
    let mut parser = Parser::new();
    parser.set_language(language).unwrap();
    parser
}

extern "C" {
    fn tree_sitter_rust() -> Language;
    fn tree_sitter_toml() -> Language;
    fn tree_sitter_yaml() -> Language;
    fn tree_sitter_go() -> Language;
    fn tree_sitter_javascript() -> Language;
}

// impl TreeSitter {
//     pub fn new() -> TreeSitter {
//         let mut parsers = HashMap::new();
//
//         let mut parser = Parser::new();
//         let language = tree_sitter_rust::language();
//         parser.set_language(language);
//         parsers.insert(LapceLanguage::Rust, parser);
//
//         TreeSitter { parsers }
//     }
// }
