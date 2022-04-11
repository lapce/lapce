use std::{collections::HashSet, path::Path};

use tree_sitter::{Parser, TreeCursor};

use crate::style::HighlightConfiguration;

const DEFAULT_CODE_LENS_LIST: &[&str] = &["source_file"];
const DEFAULT_CODE_LENS_IGNORE_LIST: &[&str] = &["source_file"];
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
const PYTHON_CODE_LENS_LIST: &[&str] = &[
    "source_file",
    "module",
    "class_definition",
    "class",
    "identifier",
    "decorated_definition",
    "block",
];
const PYTHON_CODE_LENS_IGNORE_LIST: &[&str] =
    &["source_file", "import_statement", "import_from_statement"];
const JAVASCRIPT_CODE_LENS_LIST: &[&str] = &["source_file", "program"];
const JAVASCRIPT_CODE_LENS_IGNORE_LIST: &[&str] = &["source_file"];

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug)]
pub enum LapceLanguage {
    Rust,
    Go,
    Javascript,
    Jsx,
    Typescript,
    Tsx,
    Python,
    Toml,
    Php,
    Elixir,
    C,
    Cpp,
    Json,
}

impl LapceLanguage {
    pub fn from_path(path: &Path) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?.to_lowercase();
        Some(match extension.as_str() {
            "rs" => LapceLanguage::Rust,
            "js" => LapceLanguage::Javascript,
            "jsx" => LapceLanguage::Jsx,
            "ts" => LapceLanguage::Typescript,
            "tsx" => LapceLanguage::Tsx,
            "go" => LapceLanguage::Go,
            "py" => LapceLanguage::Python,
            "toml" => LapceLanguage::Toml,
            "php" => LapceLanguage::Php,
            "ex" | "exs" => LapceLanguage::Elixir,
            "c" | "h" => LapceLanguage::C,
            "cpp" | "cxx" | "cc" | "c++" | "hpp" | "hxx" | "hh" | "h++" => {
                LapceLanguage::Cpp
            }
            "json" => LapceLanguage::Json,
            _ => return None,
        })
    }

    pub fn comment_token(&self) -> &str {
        match self {
            LapceLanguage::Rust => "//",
            LapceLanguage::Go => "//",
            LapceLanguage::Javascript => "//",
            LapceLanguage::Jsx => "//",
            LapceLanguage::Typescript => "//",
            LapceLanguage::Tsx => "//",
            LapceLanguage::Python => "#",
            LapceLanguage::Toml => "#",
            LapceLanguage::Php => "//",
            LapceLanguage::Elixir => "#",
            LapceLanguage::C => "//",
            LapceLanguage::Cpp => "//",
            LapceLanguage::Json => "",
        }
    }

    pub fn indent_unit(&self) -> &str {
        match self {
            LapceLanguage::Rust => "    ",
            LapceLanguage::Go => "\t",
            LapceLanguage::Javascript => "  ",
            LapceLanguage::Jsx => "  ",
            LapceLanguage::Typescript => "  ",
            LapceLanguage::Tsx => "  ",
            LapceLanguage::Python => "    ",
            LapceLanguage::Toml => "  ",
            LapceLanguage::Php => "  ",
            LapceLanguage::Elixir => "  ",
            LapceLanguage::C => "  ",
            LapceLanguage::Cpp => "    ",
            LapceLanguage::Json => "    ",
        }
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        match self {
            LapceLanguage::Rust => tree_sitter_rust::language(),
            LapceLanguage::Go => tree_sitter_go::language(),
            LapceLanguage::Javascript => tree_sitter_javascript::language(),
            LapceLanguage::Jsx => tree_sitter_javascript::language(),
            LapceLanguage::Typescript => {
                tree_sitter_typescript::language_typescript()
            }
            LapceLanguage::Tsx => tree_sitter_typescript::language_tsx(),
            LapceLanguage::Python => tree_sitter_python::language(),
            LapceLanguage::Toml => tree_sitter_toml::language(),
            LapceLanguage::Php => tree_sitter_php::language(),
            LapceLanguage::Elixir => tree_sitter_elixir::language(),
            LapceLanguage::C => tree_sitter_c::language(),
            LapceLanguage::Cpp => tree_sitter_cpp::language(),
            LapceLanguage::Json => tree_sitter_json::language(),
        }
    }

    pub(crate) fn new_parser(&self) -> Parser {
        let language = self.tree_sitter_language();
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        parser
    }

    pub(crate) fn new_highlight_config(&self) -> HighlightConfiguration {
        let language = self.tree_sitter_language();
        let query = match self {
            LapceLanguage::Rust => tree_sitter_rust::HIGHLIGHT_QUERY,
            LapceLanguage::Go => tree_sitter_go::HIGHLIGHT_QUERY,
            LapceLanguage::Javascript => tree_sitter_javascript::HIGHLIGHT_QUERY,
            LapceLanguage::Jsx => tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            LapceLanguage::Typescript => tree_sitter_typescript::HIGHLIGHT_QUERY,
            LapceLanguage::Tsx => tree_sitter_typescript::HIGHLIGHT_QUERY,
            LapceLanguage::Python => tree_sitter_python::HIGHLIGHT_QUERY,
            LapceLanguage::Toml => tree_sitter_toml::HIGHLIGHT_QUERY,
            LapceLanguage::Php => tree_sitter_php::HIGHLIGHT_QUERY,
            LapceLanguage::Elixir => tree_sitter_elixir::HIGHLIGHTS_QUERY,
            LapceLanguage::C => tree_sitter_c::HIGHLIGHT_QUERY,
            LapceLanguage::Cpp => tree_sitter_cpp::HIGHLIGHT_QUERY,
            LapceLanguage::Json => tree_sitter_json::HIGHLIGHT_QUERY,
        };

        HighlightConfiguration::new(language, query, "", "").unwrap()
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        let (list, ignore_list) = match self {
            LapceLanguage::Rust => (RUST_CODE_LENS_LIST, RUST_CODE_LENS_IGNORE_LIST),
            LapceLanguage::Go => (GO_CODE_LENS_LIST, GO_CODE_LENS_IGNORE_LIST),
            LapceLanguage::Python => {
                (PYTHON_CODE_LENS_LIST, PYTHON_CODE_LENS_IGNORE_LIST)
            }
            LapceLanguage::Javascript
            | LapceLanguage::Jsx
            | LapceLanguage::Typescript
            | LapceLanguage::Tsx => {
                (JAVASCRIPT_CODE_LENS_LIST, JAVASCRIPT_CODE_LENS_IGNORE_LIST)
            }
            _ => (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        };
        walk_tree(cursor, normal_lines, list, ignore_list);
    }
}

fn walk_tree(
    cursor: &mut TreeCursor,
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
            walk_tree(cursor, normal_lines, list, ignore_list);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}
