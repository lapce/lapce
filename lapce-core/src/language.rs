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
//            extensions: &["foo"],
//        },
//    ];
//
// 5. Add a new feature, say "lang-foo", to the lapce-ui crate (see
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

struct SyntaxProperties {
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
    /// TODO: someone more knowledgeable please describe what the two lists are.
    /// Anyway, the second element of the tuple is a "ignore list". See
    /// `walk_tree`. If unsure, use `DEFAULT_CODE_LENS_LIST` and
    /// `DEFAULT_CODE_LENS_IGNORE_LIST`.
    code_lens: (&'static [&'static str], &'static [&'static str]),
    /// File name extensions to determine the language.  `["py"]` for python,
    /// `["rs"]` for rust, for example.
    extensions: &'static [&'static str],
}

// NOTE: Keep the enum variants "fieldless" so they can cast to usize as array
// indices into the LANGUAGES array.  See method `LapceLanguage::properties`.
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
    #[cfg(feature = "lang-elm")]
    Elm,
    #[cfg(feature = "lang-swift")]
    Swift,
    #[cfg(feature = "lang-ql")]
    QL,
    #[cfg(feature = "lang-haskell")]
    Haskell,
    #[cfg(feature = "lang-glimmer")]
    Glimmer,
    #[cfg(feature = "lang-haxe")]
    Haxe,
    #[cfg(feature = "lang-hcl")]
    HCL,
    #[cfg(feature = "lang-ocaml")]
    OCaml,
    #[cfg(feature = "lang-ocaml")]
    OCamlInterface,
    #[cfg(feature = "lang-scss")]
    SCSS,
    #[cfg(feature = "lang-hare")]
    Hare,
}

// NOTE: Elements in the array must be in the same order as the enum variants of
// `LapceLanguage` as they will be accessed using the enum variants as indices.
const LANGUAGES: &[SyntaxProperties] = &[
    #[cfg(feature = "lang-rust")]
    SyntaxProperties {
        id: LapceLanguage::Rust,
        language: tree_sitter_rust::language,
        highlight: tree_sitter_rust::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (
            &["source_file", "impl_item", "trait_item", "declaration_list"],
            &["source_file", "use_declaration", "line_comment"],
        ),
        extensions: &["rs"],
    },
    #[cfg(feature = "lang-go")]
    SyntaxProperties {
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
            &["source_file", "comment", "line_comment"],
        ),
        extensions: &["go"],
    },
    #[cfg(feature = "lang-javascript")]
    SyntaxProperties {
        id: LapceLanguage::Javascript,
        language: tree_sitter_javascript::language,
        highlight: tree_sitter_javascript::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        extensions: &["js"],
    },
    #[cfg(feature = "lang-javascript")]
    SyntaxProperties {
        id: LapceLanguage::Jsx,
        language: tree_sitter_javascript::language,
        highlight: tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        extensions: &["jsx"],
    },
    #[cfg(feature = "lang-typescript")]
    SyntaxProperties {
        id: LapceLanguage::Typescript,
        language: tree_sitter_typescript::language_typescript,
        highlight: tree_sitter_typescript::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        extensions: &["ts"],
    },
    #[cfg(feature = "lang-typescript")]
    SyntaxProperties {
        id: LapceLanguage::Tsx,
        language: tree_sitter_typescript::language_tsx,
        highlight: tree_sitter_typescript::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        extensions: &["tsx"],
    },
    #[cfg(feature = "lang-python")]
    SyntaxProperties {
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
            &["source_file", "import_statement", "import_from_statement"],
        ),
        extensions: &["py"],
    },
    #[cfg(feature = "lang-toml")]
    SyntaxProperties {
        id: LapceLanguage::Toml,
        language: tree_sitter_toml::language,
        highlight: tree_sitter_toml::HIGHLIGHT_QUERY,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["toml"],
    },
    #[cfg(feature = "lang-php")]
    SyntaxProperties {
        id: LapceLanguage::Php,
        language: tree_sitter_php::language,
        highlight: tree_sitter_php::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["php"],
    },
    #[cfg(feature = "lang-elixir")]
    SyntaxProperties {
        id: LapceLanguage::Elixir,
        language: tree_sitter_elixir::language,
        highlight: tree_sitter_elixir::HIGHLIGHTS_QUERY,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["ex", "exs", "eex", "heex", "sface"],
    },
    #[cfg(feature = "lang-c")]
    SyntaxProperties {
        id: LapceLanguage::C,
        language: tree_sitter_c::language,
        highlight: tree_sitter_c::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["c"],
    },
    #[cfg(feature = "lang-cpp")]
    SyntaxProperties {
        id: LapceLanguage::Cpp,
        language: tree_sitter_cpp::language,
        highlight: tree_sitter_cpp::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["cpp", "cxx", "cc", "c++", "hpp", "hxx", "hh", "h++"],
    },
    #[cfg(feature = "lang-json")]
    SyntaxProperties {
        id: LapceLanguage::Json,
        language: tree_sitter_json::language,
        highlight: tree_sitter_json::HIGHLIGHT_QUERY,
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["json"],
    },
    #[cfg(feature = "lang-md")]
    SyntaxProperties {
        id: LapceLanguage::Markdown,
        language: tree_sitter_md::language,
        highlight: tree_sitter_md::HIGHLIGHTS_QUERY,
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["md"],
    },
    #[cfg(feature = "lang-ruby")]
    SyntaxProperties {
        id: LapceLanguage::Ruby,
        language: tree_sitter_ruby::language,
        highlight: tree_sitter_ruby::HIGHLIGHT_QUERY,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["rb"],
    },
    #[cfg(feature = "lang-html")]
    SyntaxProperties {
        id: LapceLanguage::Html,
        language: tree_sitter_html::language,
        highlight: tree_sitter_html::HIGHLIGHT_QUERY,
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["html", "htm"],
    },
    #[cfg(feature = "lang-java")]
    SyntaxProperties {
        id: LapceLanguage::Java,
        language: tree_sitter_java::language,
        highlight: tree_sitter_java::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["java"],
    },
    #[cfg(feature = "lang-elm")]
    SyntaxProperties {
        id: LapceLanguage::Elm,
        language: tree_sitter_elm::language,
        highlight: tree_sitter_elm::HIGHLIGHTS_QUERY,
        comment: "#",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["elm"],
    },
    #[cfg(feature = "lang-swift")]
    SyntaxProperties {
        id: LapceLanguage::Swift,
        language: tree_sitter_swift::language,
        highlight: tree_sitter_swift::HIGHLIGHTS_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["swift"],
    },
    #[cfg(feature = "lang-ql")]
    SyntaxProperties {
        id: LapceLanguage::QL,
        language: tree_sitter_ql::language,
        highlight: tree_sitter_ql::HIGHLIGHTS_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["ql"],
    },
    #[cfg(feature = "lang-haskell")]
    SyntaxProperties {
        id: LapceLanguage::Haskell,
        language: tree_sitter_haskell::language,
        highlight: tree_sitter_haskell::HIGHLIGHTS_QUERY,
        comment: "--",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["hs"],
    },
    #[cfg(feature = "lang-glimmer")]
    SyntaxProperties {
        id: LapceLanguage::Glimmer,
        language: tree_sitter_glimmer::language,
        highlight: tree_sitter_glimmer::HIGHLIGHTS_QUERY,
        comment: "{{!",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["hbs"],
    },
    #[cfg(feature = "lang-haxe")]
    SyntaxProperties {
        id: LapceLanguage::Haxe,
        language: tree_sitter_haxe::language,
        highlight: tree_sitter_haxe::HIGHLIGHTS_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["hx"],
    },
    #[cfg(feature = "lang-hcl")]
    SyntaxProperties {
        id: LapceLanguage::HCL,
        language: tree_sitter_hcl::language,
        highlight: tree_sitter_hcl::HIGHLIGHTS_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["hcl"],
    },
    #[cfg(feature = "lang-ocaml")]
    SyntaxProperties {
        id: LapceLanguage::OCaml,
        language: tree_sitter_ocaml::language_ocaml,
        highlight: tree_sitter_ocaml::HIGHLIGHTS_QUERY,
        comment: "(*",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["ml"],
    },
    #[cfg(feature = "lang-ocaml")]
    SyntaxProperties {
        id: LapceLanguage::OCaml,
        language: tree_sitter_ocaml::language_ocaml_interface,
        highlight: tree_sitter_ocaml::HIGHLIGHTS_QUERY,
        comment: "(*",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["mli"],
    },
    #[cfg(feature = "lang-scss")]
    SyntaxProperties {
        id: LapceLanguage::SCSS,
        language: tree_sitter_scss::language,
        highlight: tree_sitter_scss::HIGHLIGHTS_QUERY,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["scss"],
    },
    #[cfg(feature = "lang-hare")]
    SyntaxProperties {
        id: LapceLanguage::Hare,
        language: tree_sitter_hare::language,
        highlight: tree_sitter_hare::HIGHLIGHT_QUERY,
        comment: "//",
        indent: "        ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["ha"],
    },
];

impl LapceLanguage {
    pub fn from_path(path: &Path) -> Option<LapceLanguage> {
        let extension = path.extension()?.to_str()?.to_lowercase();
        // NOTE: This is a linear search.  It is assumed that this function
        // isn't called in any tight loop.
        for properties in LANGUAGES {
            if properties.extensions.contains(&extension.as_str()) {
                return Some(properties.id);
            }
        }
        None
    }

    // NOTE: Instead of using `&LANGUAGES[*self as usize]` directly, the
    // `debug_assertion` gives better feedback should something has gone wrong
    // badly.
    fn properties(&self) -> &SyntaxProperties {
        let i = *self as usize;
        let l = &LANGUAGES[i];
        debug_assert!(
            l.id == *self,
            "LANGUAGES[{i}]: Setting::id mismatch: {:?} != {:?}",
            l.id,
            self
        );
        l
    }

    pub fn comment_token(&self) -> &str {
        self.properties().comment
    }

    pub fn indent_unit(&self) -> &str {
        self.properties().indent
    }

    pub(crate) fn new_parser(&self) -> Parser {
        let language = (self.properties().language)();
        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        parser
    }

    pub(crate) fn new_highlight_config(&self) -> HighlightConfiguration {
        let language = (self.properties().language)();
        let query = self.properties().highlight;

        HighlightConfiguration::new(language, query, "", "").unwrap()
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        let (list, ignore_list) = self.properties().code_lens;
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

#[cfg(test)]
mod test {
    // If none of the lang features is selected, the imports and the auxiliary
    // function(s) in the module become unused.  Hence turning off the lints.
    #![allow(unused, unreachable_code)]

    use super::LapceLanguage;
    use std::path::PathBuf;

    fn assert_language(expected: LapceLanguage, exts: &[&str]) {
        for ext in exts {
            let path = PathBuf::from(&format!("a.{ext}"));
            let lang = LapceLanguage::from_path(&path).unwrap();

            assert_eq!(lang, expected);
            // In debug build, this assertion will never set off.  It
            // nonetheless exercises the boundary check, and the debug
            // assertion, in the `properties()` function.
            assert_eq!(lang.properties().id, expected);
        }

        // Hopefully there will not be such a file extension to support.
        let path = PathBuf::from("a.___");
        assert!(LapceLanguage::from_path(&path).is_none());
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn test_rust_lang() {
        assert_language(LapceLanguage::Rust, &["rs"]);
    }

    #[test]
    #[cfg(feature = "lang-go")]
    fn test_go_lang() {
        assert_language(LapceLanguage::Go, &["go"]);
    }

    #[test]
    #[cfg(feature = "lang-javascript")]
    fn test_javascript_lang() {
        assert_language(LapceLanguage::Javascript, &["js"]);
    }

    #[test]
    #[cfg(feature = "lang-javascript")]
    fn test_jsx_lang() {
        assert_language(LapceLanguage::Jsx, &["jsx"]);
    }

    #[test]
    #[cfg(feature = "lang-typescript")]
    fn test_typescript_lang() {
        assert_language(LapceLanguage::Typescript, &["ts"]);
    }

    #[test]
    #[cfg(feature = "lang-typescript")]
    fn test_tsx_lang() {
        assert_language(LapceLanguage::Tsx, &["tsx"]);
    }

    #[test]
    #[cfg(feature = "lang-python")]
    fn test_python_lang() {
        assert_language(LapceLanguage::Python, &["py"]);
    }

    #[test]
    #[cfg(feature = "lang-toml")]
    fn test_toml_lang() {
        assert_language(LapceLanguage::Toml, &["toml"]);
    }

    #[test]
    #[cfg(feature = "lang-elixir")]
    fn test_elixir_lang() {
        assert_language(LapceLanguage::Elixir, &["ex"]);
    }

    #[test]
    #[cfg(feature = "lang-php")]
    fn test_php_lang() {
        assert_language(LapceLanguage::Php, &["php"]);
    }

    #[test]
    #[cfg(feature = "lang-ruby")]
    fn test_ruby_lang() {
        assert_language(LapceLanguage::Ruby, &["rb"]);
    }

    #[test]
    #[cfg(feature = "lang-c")]
    fn test_c_lang() {
        assert_language(LapceLanguage::C, &["c"]);
    }

    #[test]
    #[cfg(feature = "lang-cpp")]
    fn test_cpp_lang() {
        assert_language(
            LapceLanguage::Cpp,
            &["cpp", "cxx", "cc", "c++", "hpp", "hxx", "hh", "h++"],
        );
    }

    #[test]
    #[cfg(feature = "lang-json")]
    fn test_json_lang() {
        assert_language(LapceLanguage::Json, &["json"]);
    }

    #[test]
    #[cfg(feature = "lang-md")]
    fn test_md_lang() {
        assert_language(LapceLanguage::Markdown, &["md"]);
    }

    #[test]
    #[cfg(feature = "lang-html")]
    fn test_html_lang() {
        assert_language(LapceLanguage::Html, &["html", "htm"]);
    }

    #[test]
    #[cfg(feature = "lang-java")]
    fn test_java_lang() {
        assert_language(LapceLanguage::Java, &["java"]);
    }
    #[test]
    #[cfg(feature = "lang-elm")]
    fn test_elm_lang() {
        assert_language(LapceLanguage::Elm, &["elm"]);
    }
    #[test]
    #[cfg(feature = "lang-swift")]
    fn test_swift_lang() {
        assert_language(LapceLanguage::Swift, &["swift"]);
    }
    #[test]
    #[cfg(feature = "lang-ql")]
    fn test_ql_lang() {
        assert_language(LapceLanguage::QL, &["ql"]);
    }
    #[test]
    #[cfg(feature = "lang-haskell")]
    fn test_haskell_lang() {
        assert_language(LapceLanguage::Haskell, &["hs"]);
    }
    #[cfg(feature = "lang-glimmer")]
    fn test_glimmer_lang() {
        assert_language(LapceLanguage::Glimmer, &["hbs"]);
    }
    #[cfg(feature = "lang-haxe")]
    fn test_haxe_lang() {
        assert_language(LapceLanguage::Haxe, &["hx"]);
    }
    #[cfg(feature = "lang-hcl")]
    fn test_hcl_lang() {
        assert_language(LapceLanguage::HCL, &["hcl"]);
    }
    #[cfg(feature = "lang-ocaml")]
    fn test_ocaml_lang() {
        assert_language(LapceLanguage::OCaml, &["ml"]);
        assert_language(LapceLanguage::OCamlInterface, &["mli"]);
    }
    #[cfg(feature = "lang-scss")]
    fn test_scss_lang() {
        assert_language(LapceLanguage::SCSS, &["scss"]);
    }
    #[cfg(feature = "lang-hare")]
    fn test_hare_lang() {
        assert_language(LapceLanguage::Hare, &["ha"]);
    }
}
