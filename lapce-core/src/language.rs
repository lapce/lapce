use std::{collections::HashSet, path::Path};

use tree_sitter::{Parser, TreeCursor};

use crate::style::HighlightConfiguration;

//
// To add support for an hypothetical language called Foo, for example, using
// the crate named as tree-sitter-foo:
//
// 1. Add an optional dependency on tree-sitter-foo in this crate.
//
//    [dependencies]
//    # ...
//    tree-sitter-foo = { version = "1", optional = true }
//
// 2. Add a new feature, say "lang-foo", to this crate to use this dependency.
//    Also add "lang-foo" to the "all-languages" feature (see
//    lapce-core/Cargo.toml).
//
//    [features]
//    # ...
//    lang-foo = "dep:tree-sitter-foo"
//
// 3. Add a new variant to `LapceLanguage`, say Foo, following the existing
//    variants, guard the new variant with the new feature.
//
//    pub enum LapceLanguage {
//         // ...
//         #[cfg(feature = "lang-foo")]
//         Foo,
//    }
//
// 4. Add a new element in the LANGUAGES array, guard the new element with the
//    new feature.
//
//    const LANGUAGES: &[Settings] = &[
//        // ...
//        #[cfg(feature = "lang-foo")]
//        Setting{
//            id: LapceLanguage::Foo,
//            language: tree_sitter_foo::language,
//            highlight: tree_sitter_foo::HIGHLIGHT_QUERY,
//            comment: "//",
//            indent: "    ",
//            code_lens: (&[/* ... */], &[/* ... */]),
//        },
//    ];
//
// 5. Add a new match arm in `LapceLanguage::from_path`, guard the new arm with
//    the new feature.
//
//    Some(match extension.as_str() {
//        // ...
//        #[cfg(feature = "lang-foo")]
//        "foo" => LapceLanguage::Foo,
//        _ => return None,
//    })
//
// 6. Add a new feature, say "lang-foo", to the lapce-ui crate (see
//    lapce-ui/Cargo.toml).
//
//    [features]
//    # ...
//    lang-foo = "lapce-core/lang-foo"
//

// Use these lists when a language does not have specific settings for "code
// lens".
#[allow(dead_code)]
const DEFAULT_CODE_LENS_LIST: &[&str] = &["source_file"];
#[allow(dead_code)]
const DEFAULT_CODE_LENS_IGNORE_LIST: &[&str] = &["source_file"];

struct Setting {
    /// An extra check to make sure that the array elements are in the correct
    /// order.  If this id does not match the enum value, a panic will happen
    /// with a debug assertion message.
    id: LapceLanguage,
    /// This is the factory function defined in the tree-sitter crate that
    /// creates the language parser.  For most languages, it is
    /// `tree_sitter_$crate::language`.
    language: fn() -> tree_sitter::Language,
    /// For most languages, it is `tree_sitter_$crate::HIGHLIGHT_QUERY`.
    highlight: &'static str,
    /// The comment token.  "#" for python, "//" for rust for example.
    comment: &'static str,
    /// The indent unit.  "\t" for python, "    " for rust, for example.
    indent: &'static str,
    /// If unsure, use `DEFAULT_CODE_LENS_LIST` and
    /// `DEFAULT_CODE_LENS_IGNORE_LIST`.
    code_lens: (&'static [&'static str], &'static [&'static str]),
}

// NOTE: Keep the enum variants "fieldless" so they can cast to usize as array
// indices into the LANGUAGES array.  See `LapceLanguage::find`.
//
// Do not assign values to the variants because the number of variants and
// number of elements in the LANGUAGES array change as different features
// selected by the cargo build command.
#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug)]
pub enum LapceLanguage {
    #[cfg(feature = "lang-rust")]
    Rust,
    #[cfg(feature = "lang-go")]
    Go,
    #[cfg(feature = "lang-javascript")]
    Javascript,
    #[cfg(feature = "lang-javascript")]
    Jsx,
    #[cfg(feature = "lang-typescript")]
    Typescript,
    #[cfg(feature = "lang-typescript")]
    Tsx,
    #[cfg(feature = "lang-python")]
    Python,
    #[cfg(feature = "lang-toml")]
    Toml,
    #[cfg(feature = "lang-php")]
    Php,
    #[cfg(feature = "lang-elixir")]
    Elixir,
    #[cfg(feature = "lang-c")]
    C,
    #[cfg(feature = "lang-cpp")]
    Cpp,
    #[cfg(feature = "lang-json")]
    Json,
    #[cfg(feature = "lang-md")]
    Markdown,
    #[cfg(feature = "lang-ruby")]
    Ruby,
    #[cfg(feature = "lang-html")]
    Html,
    #[cfg(feature = "lang-java")]
    Java,
}

// NOTE: Elements in the array must be in the same order as the enum variants of
// `LapceLanguage` as they will be accessed using the enum variants as indices.
const LANGUAGES: &[Setting] = &[
    #[cfg(feature = "lang-rust")]
    Setting{
        id: LapceLanguage::Rust,
        language: tree_sitter_rust::language,
        highlight: tree_sitter_rust::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (
            &["source_file", "impl_item", "trait_item", "declaration_list"],
            &["source_file", "use_declaration", "line_comment"]
        ),
    },
    #[cfg(feature = "lang-go")]
    Setting{
        id: LapceLanguage::Go,
        language: tree_sitter_go::language,
        highlight: tree_sitter_go::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (
            &[
                "source_file",
                "type_declaration",
                "type_spec",
                "interface_type",
                "method_spec_list",
            ],
            &["source_file", "comment", "line_comment"]
        ),
    },
    #[cfg(feature = "lang-javascript")]
    Setting{
        id: LapceLanguage::Javascript,
        language: tree_sitter_javascript::language,
        highlight: tree_sitter_javascript::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (&["source_file", "program"], &["source_file"]),
    },
    #[cfg(feature = "lang-javascript")]
    Setting{
        id: LapceLanguage::Jsx,
        language: tree_sitter_javascript::language,
        highlight: tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (&["source_file", "program"], &["source_file"]),
    },
    #[cfg(feature = "lang-typescript")]
    Setting{
        id: LapceLanguage::Typescript,
        language: tree_sitter_typescript::language_typescript,
        highlight: tree_sitter_typescript::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (&["source_file", "program"], &["source_file"]),
    },
    #[cfg(feature = "lang-typescript")]
    Setting{
        id: LapceLanguage::Tsx,
        language: tree_sitter_typescript::language_tsx,
        highlight: tree_sitter_typescript::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (&["source_file", "program"], &["source_file"]),
    },
    #[cfg(feature = "lang-python")]
    Setting{
        id: LapceLanguage::Python,
        language: tree_sitter_python::language,
        highlight: tree_sitter_python::HIGHLIGHT_QUERY,
        comment: "#",
        indent: "\t",
        code_lens: (
            &[
                "source_file",
                "module",
                "class_definition",
                "class",
                "identifier",
                "decorated_definition",
                "block",
            ],
            &["source_file", "import_statement", "import_from_statement"]
        ),
    },
    #[cfg(feature = "lang-toml")]
    Setting{
        id: LapceLanguage::Toml,
        language: tree_sitter_toml::language,
        highlight: tree_sitter_toml::HIGHLIGHT_QUERY,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-php")]
    Setting{
        id: LapceLanguage::Php,
        language: tree_sitter_php::language,
        highlight: tree_sitter_php::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-elixir")]
    Setting{
        id: LapceLanguage::Elixir,
        language: tree_sitter_elixir::language,
        highlight: tree_sitter_elixir::HIGHLIGHTS_QUERY,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-c")]
    Setting{
        id: LapceLanguage::C,
        language: tree_sitter_c::language,
        highlight: tree_sitter_c::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-cpp")]
    Setting{
        id: LapceLanguage::Cpp,
        language: tree_sitter_cpp::language,
        highlight: tree_sitter_cpp::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-json")]
    Setting{
        id: LapceLanguage::Json,
        language: tree_sitter_json::language,
        highlight: tree_sitter_json::HIGHLIGHT_QUERY,
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-md")]
    Setting{
        id: LapceLanguage::Markdown,
        language: tree_sitter_md::language,
        highlight: tree_sitter_md::HIGHLIGHTS_QUERY,
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-ruby")]
    Setting{
        id: LapceLanguage::Ruby,
        language: tree_sitter_ruby::language,
        highlight: tree_sitter_ruby::HIGHLIGHT_QUERY,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-html")]
    Setting{
        id: LapceLanguage::Html,
        language: tree_sitter_html::language,
        highlight: tree_sitter_html::HIGHLIGHT_QUERY,
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
    #[cfg(feature = "lang-java")]
    Setting{
        id: LapceLanguage::Java,
        language: tree_sitter_java::language,
        highlight: tree_sitter_java::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
    },
];

impl LapceLanguage {
    pub fn from_path(path: &Path) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?.to_lowercase();
        Some(match extension.as_str() {
            #[cfg(feature = "lang-rust")]
            "rs" => LapceLanguage::Rust,
            #[cfg(feature = "lang-javascript")]
            "js" => LapceLanguage::Javascript,
            #[cfg(feature = "lang-javascript")]
            "jsx" => LapceLanguage::Jsx,
            #[cfg(feature = "lang-typescript")]
            "ts" => LapceLanguage::Typescript,
            #[cfg(feature = "lang-typescript")]
            "tsx" => LapceLanguage::Tsx,
            #[cfg(feature = "lang-go")]
            "go" => LapceLanguage::Go,
            #[cfg(feature = "lang-python")]
            "py" => LapceLanguage::Python,
            #[cfg(feature = "lang-toml")]
            "toml" => LapceLanguage::Toml,
            #[cfg(feature = "lang-php")]
            "php" => LapceLanguage::Php,
            #[cfg(feature = "lang-elixir")]
            "ex" | "exs" => LapceLanguage::Elixir,
            #[cfg(feature = "lang-c")]
            "c" | "h" => LapceLanguage::C,
            #[cfg(feature = "lang-cpp")]
            "cpp" | "cxx" | "cc" | "c++" | "hpp" | "hxx" | "hh" | "h++" => {
                LapceLanguage::Cpp
            }
            #[cfg(feature = "lang-json")]
            "json" => LapceLanguage::Json,
            #[cfg(feature = "lang-md")]
            "md" => LapceLanguage::Markdown,
            #[cfg(feature = "lang-ruby")]
            "rb" => LapceLanguage::Ruby,
            #[cfg(feature = "lang-html")]
            "html" | "htm" => LapceLanguage::Html,
            #[cfg(feature = "lang-java")]
            "java" => LapceLanguage::Java,
            _ => return None,
        })
    }

    fn find(&self) -> &Setting {
        let i = *self as usize;
        let l = &LANGUAGES[i];
        debug_assert!(l.id == *self, "LANGUAGES[{i}]: Setting::id mismatch: {:?} != {:?}", l.id, self);
        l
    }

    pub fn comment_token(&self) -> &str {
        self.find().comment
    }

    pub fn indent_unit(&self) -> &str {
        self.find().indent
    }

    pub(crate) fn new_parser(&self) -> Parser {
        let language = (self.find().language)();
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        parser
    }

    pub(crate) fn new_highlight_config(&self) -> HighlightConfiguration {
        let language = (self.find().language)();
        let query = self.find().highlight;

        HighlightConfiguration::new(language, query, "", "").unwrap()
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        let (list, ignore_list) = self.find().code_lens;
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
