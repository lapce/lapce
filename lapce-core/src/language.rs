use std::{collections::HashSet, path::Path};

use tree_sitter::{Parser, TreeCursor};

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug)]
pub enum LapceLanguage {
    Rust,
}

impl LapceLanguage {
    pub fn from_path(path: &Path) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?;
        Some(match extension {
            "rs" => LapceLanguage::Rust,
            // "js" => LapceLanguage::Javascript,
            // "jsx" => LapceLanguage::Javascript,
            // "go" => LapceLanguage::Go,
            // "toml" => LapceLanguage::Toml,
            // "yaml" => LapceLanguage::Yaml,
            // "yml" => LapceLanguage::Yaml,
            _ => return None,
        })
    }

    pub(crate) fn new_parser(&self) -> Parser {
        let language = match self {
            LapceLanguage::Rust => tree_sitter_rust::language(),
        };
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        parser
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        match self {
            LapceLanguage::Rust => rust_walk_tree(cursor, 0, normal_lines),
        };
    }
}

fn rust_walk_tree(
    cursor: &mut TreeCursor,
    level: usize,
    normal_lines: &mut HashSet<usize>,
) {
    let node = cursor.node();
    let start_pos = node.start_position();
    let end_pos = node.end_position();
    let kind = node.kind();
    if !["source_file", "use_declaration", "line_comment"].contains(&kind) {
        normal_lines.insert(start_pos.row);
        normal_lines.insert(end_pos.row);
    }

    if ["source_file", "impl_item", "trait_item", "declaration_list"].contains(&kind)
        && cursor.goto_first_child()
    {
        loop {
            rust_walk_tree(cursor, level + 1, normal_lines);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}
