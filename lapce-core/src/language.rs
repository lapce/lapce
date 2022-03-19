use std::{collections::HashSet, path::Path};

use tree_sitter::{Parser, Query, TreeCursor};
use tree_sitter_highlight::HighlightConfiguration;

const RUST_CODE_LENS_LIST: &[&str] =
    &["source_file", "impl_item", "trait_item", "declaration_list"];
const RUST_CODE_LENS_IGNORE_LIST: &[&str] =
    &["source_file", "use_declaration", "line_comment"];
const GO_CODE_LENS_LIST: &[&str] = &[
    "source_file",
    "type_declaration",
    "type_spec",
    "interface_type",
    "method_spec_list",
];
const GO_CODE_LENS_IGNORE_LIST: &[&str] =
    &["source_file", "comment", "line_comment"];

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug)]
pub enum LapceLanguage {
    Rust,
    Go,
}

impl LapceLanguage {
    pub fn from_path(path: &Path) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?;
        Some(match extension {
            "rs" => LapceLanguage::Rust,
            // "js" => LapceLanguage::Javascript,
            // "jsx" => LapceLanguage::Javascript,
            "go" => LapceLanguage::Go,
            // "toml" => LapceLanguage::Toml,
            // "yaml" => LapceLanguage::Yaml,
            // "yml" => LapceLanguage::Yaml,
            _ => return None,
        })
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        match self {
            LapceLanguage::Rust => tree_sitter_rust::language(),
            LapceLanguage::Go => tree_sitter_go::language(),
        }
    }

    pub(crate) fn new_parser(&self) -> Parser {
        let language = self.tree_sitter_language();
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        parser
    }

    // TODO: Use or remove
    pub(crate) fn _new_highlight_query(&self) -> Query {
        let language = self.tree_sitter_language();
        let query = match self {
            LapceLanguage::Rust => tree_sitter_rust::HIGHLIGHT_QUERY,
            LapceLanguage::Go => tree_sitter_go::HIGHLIGHT_QUERY,
        };
        HighlightConfiguration::new(language, query, "", "")
            .unwrap()
            .query
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        let (list, ignore_list) = match self {
            LapceLanguage::Rust => (RUST_CODE_LENS_LIST, RUST_CODE_LENS_IGNORE_LIST),
            LapceLanguage::Go => (GO_CODE_LENS_LIST, GO_CODE_LENS_IGNORE_LIST),
        };
        walk_tree(cursor, 0, normal_lines, list, ignore_list);
    }
}

fn walk_tree(
    cursor: &mut TreeCursor,
    level: usize,
    normal_lines: &mut HashSet<usize>,
    list: &[&str],
    ignore_list: &[&str],
) {
    let node = cursor.node();
    let start_pos = node.start_position();
    let end_pos = node.end_position();
    let kind = node.kind().trim();
    if !ignore_list.contains(&kind) && !kind.is_empty() {
        normal_lines.insert(start_pos.row);
        normal_lines.insert(end_pos.row);
    }

    if list.contains(&kind) && cursor.goto_first_child() {
        loop {
            walk_tree(cursor, level + 1, normal_lines, list, ignore_list);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}
