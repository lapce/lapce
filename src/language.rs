use std::collections::HashMap;

use include_dir::{include_dir, Dir};
use tree_sitter::{Language, Parser};
use tree_sitter_highlight::HighlightConfiguration;

const LIB_DIR: Dir = include_dir!("lib");

extern "C" {
    fn tree_sitter_rust() -> Language;
}

#[derive(Eq, PartialEq, Hash)]
pub enum LapceLanguage {
    Rust,
}

pub struct TreeSitter {
    parsers: HashMap<LapceLanguage, Parser>,
}

pub fn new_highlight_config(
    language: LapceLanguage,
) -> (HighlightConfiguration, Vec<String>) {
    let language = unsafe { tree_sitter_rust() };
    let mut configuration = HighlightConfiguration::new(
        language,
        LIB_DIR
            .get_file("tree-sitter-rust/queries/highlights.scm")
            .unwrap()
            .contents_utf8()
            .unwrap(),
        LIB_DIR
            .get_file("tree-sitter-rust/queries/injections.scm")
            .unwrap()
            .contents_utf8()
            .unwrap(),
        "",
    )
    .unwrap();

    let recognized_names = vec![
        "constant",
        "constant.builtin",
        "type",
        "type.builtin",
        "property",
        "comment",
        "constructor",
        "function",
        "function.method",
        "function.macro",
        "punctuation.bracket",
        "punctuation.delimiter",
        "label",
        "keyword",
        "string",
        "variable.parameter",
        "operator",
        "attribute",
        "escape",
        "variable.builtin",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<String>>();
    configuration.configure(&recognized_names);

    (configuration, recognized_names)
}

pub fn new_parser(language: LapceLanguage) -> Parser {
    let language = unsafe { tree_sitter_rust() };
    let mut parser = Parser::new();
    parser.set_language(language).unwrap();
    parser
}

impl TreeSitter {
    pub fn new() -> TreeSitter {
        let mut parsers = HashMap::new();

        let mut parser = Parser::new();
        let language = unsafe { tree_sitter_rust() };
        parser.set_language(language);
        parsers.insert(LapceLanguage::Rust, parser);

        TreeSitter { parsers }
    }
}
