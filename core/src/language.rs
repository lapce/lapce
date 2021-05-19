use std::str::FromStr;
use std::{collections::HashMap, path::PathBuf};
use tree_sitter::Parser;
use tree_sitter_highlight::HighlightConfiguration;
use tree_sitter_rust;

#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub enum LapceLanguage {
    Rust,
    // Go,
}

impl LapceLanguage {
    pub fn from_path(path: &PathBuf) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?;
        Some(match extension {
            "rs" => LapceLanguage::Rust,
            _ => return None,
        })
    }
}

pub struct TreeSitter {
    parsers: HashMap<LapceLanguage, Parser>,
}

pub fn new_highlight_config(
    language: LapceLanguage,
) -> (HighlightConfiguration, Vec<String>) {
    match language {
        LapceLanguage::Rust => {
            let mut configuration = HighlightConfiguration::new(
                tree_sitter_rust::language(),
                tree_sitter_rust::HIGHLIGHT_QUERY,
                "",
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
                "variable.builtin",
                "operator",
                "attribute",
                "escape",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
            configuration.configure(&recognized_names);

            (configuration, recognized_names)
        } // LapceLanguage::Go => {
          //     let mut configuration = HighlightConfiguration::new(
          //         tree_sitter_go::language(),
          //         tree_sitter_go::HIGHLIGHT_QUERY,
          //         "",
          //         "",
          //     )
          //     .unwrap();
          //     let recognized_names = vec![
          //         "constant",
          //         "constant.builtin",
          //         "type",
          //         "type.builtin",
          //         "property",
          //         "comment",
          //         "constructor",
          //         "function",
          //         "function.method",
          //         "function.macro",
          //         "punctuation.bracket",
          //         "punctuation.delimiter",
          //         "label",
          //         "keyword",
          //         "string",
          //         "variable.parameter",
          //         "variable.builtin",
          //         "operator",
          //         "attribute",
          //         "escape",
          //     ]
          //     .iter()
          //     .map(|s| s.to_string())
          //     .collect::<Vec<String>>();
          //     configuration.configure(&recognized_names);

          //     (configuration, recognized_names)
          // }
    }
}

pub fn new_parser(language: LapceLanguage) -> Parser {
    let language = match language {
        LapceLanguage::Rust => tree_sitter_rust::language(),
        // LapceLanguage::Go => tree_sitter_go::language(),
    };
    let mut parser = Parser::new();
    parser.set_language(language).unwrap();
    parser
}

impl TreeSitter {
    pub fn new() -> TreeSitter {
        let mut parsers = HashMap::new();

        let mut parser = Parser::new();
        let language = tree_sitter_rust::language();
        parser.set_language(language);
        parsers.insert(LapceLanguage::Rust, parser);

        TreeSitter { parsers }
    }
}
