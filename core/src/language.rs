use anyhow::Context;
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use libloading::{Library, Symbol};
use std::str::FromStr;
use std::{collections::HashMap, path::PathBuf};
use tree_sitter::{Language, Parser};
use tree_sitter_highlight::HighlightConfiguration;

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
    Javascript,
    Go,
}

impl LapceLanguage {
    pub fn from_path(path: &PathBuf) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?;
        Some(match extension {
            "rs" => LapceLanguage::Rust,
            "js" => LapceLanguage::Javascript,
            "jsx" => LapceLanguage::Javascript,
            "go" => LapceLanguage::Go,
            // "toml" => LapceLanguage::Toml,
            // "yaml" => LapceLanguage::Yaml,
            // "yml" => LapceLanguage::Yaml,
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
                tree_sitter_rust::language(),
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
        LapceLanguage::Javascript => {
            let mut configuration = HighlightConfiguration::new(
                tree_sitter_javascript::language(),
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
        LapceLanguage::Go => {
            let mut configuration = HighlightConfiguration::new(
                tree_sitter_go::language(),
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
        LapceLanguage::Rust => tree_sitter_rust::language(),
        LapceLanguage::Javascript => tree_sitter_javascript::language(),
        LapceLanguage::Go => tree_sitter_go::language(),
    };
    let mut parser = Parser::new();
    parser.set_language(language).unwrap();
    parser
}
