use std::path::Path;

use lazy_static::lazy_static;
use tree_sitter::Parser;
use tree_sitter_highlight::HighlightConfiguration;

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
    pub fn from_path(path: &Path) -> Option<LapceLanguage> {
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

pub fn new_highlight_config(language: LapceLanguage) -> HighlightConfiguration {
    match language {
        LapceLanguage::Rust => {
            let mut configuration = HighlightConfiguration::new(
                tree_sitter_rust::language(),
                tree_sitter_rust::HIGHLIGHT_QUERY,
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
                tree_sitter_javascript::HIGHLIGHT_QUERY,
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
                tree_sitter_go::HIGHLIGHT_QUERY,
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
