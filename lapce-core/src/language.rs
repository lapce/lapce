use std::{collections::HashSet, path::Path, str::FromStr};

use strum_macros::{Display, EnumString};
use tree_sitter::TreeCursor;

use crate::syntax::highlight::{HighlightConfiguration, HighlightIssue};

///
/// To add support for an hypothetical language called Foo, for example, using
/// the crate named as tree-sitter-foo:
///
/// 1. Add an optional dependency on tree-sitter-foo in this crate.
///
///    [dependencies]
///    # ...
///    tree-sitter-foo = { version = "1", optional = true }
///
/// 2. Add a new feature, say "lang-foo", to this crate to use this dependency.
///    Also add "lang-foo" to the "all-languages" feature (see
///    lapce-core/Cargo.toml).
///
///    [features]
///    # ...
///    lang-foo = "dep:tree-sitter-foo"
///
/// 3. Add a new variant to `LapceLanguage`, say Foo, following the existing
///    variants, guard the new variant with the new feature.
///
///    pub enum LapceLanguage {
///         // ...
///         #[cfg(feature = "lang-foo")]
///         Foo,
///    }
///
/// 4. Add a new element in the LANGUAGES array, guard the new element with the
///    new feature.
///
///    const LANGUAGES: &[Settings] = &[
///        // ...
///        #[cfg(feature = "lang-foo")]
///        Setting{
///            id: LapceLanguage::Foo,
///            language: tree_sitter_foo::language,
///            highlight: Some(tree_sitter_foo::HIGHLIGHT_QUERY),
///            injection: Some(tree_sitter_foo::INJECTION_QUERY), // or None if there is no injections
///            comment: "//",
///            indent: "    ",
///            code_lens: (&[/* ... */], &[/* ... */]),
///            extensions: &["foo"],
///        },
///    ];
///
/// 5. In `syntax/highlight.rs`, add `Foo: "lang-foo",` to the list in the
///    `declare_language_highlights` macro.
///
/// 6. Add a new feature, say "lang-foo", to the lapce-ui crate (see
///    lapce-ui/Cargo.toml).
///
///    [features]
///    # ...
///    lang-foo = "lapce-core/lang-foo"
///

/// Use these lists when a language does not have specific settings for "code
/// lens".
#[allow(dead_code)]
const DEFAULT_CODE_LENS_LIST: &[&str] = &["source_file"];
#[allow(dead_code)]
const DEFAULT_CODE_LENS_IGNORE_LIST: &[&str] = &["source_file"];

const EMPTY_LANGUAGE: SyntaxProperties = SyntaxProperties {
    id: LapceLanguage::Plaintext,

    indent: "    ",
    files: &[],
    extensions: &[],

    comment: CommentProperties {
        single_line_start: "",
        single_line_end: "",

        multi_line_start: "",
        multi_line_prefix: "",
        multi_line_end: "",
    },

    tree_sitter: Some(TreeSitterProperties {
        language: tree_sitter_plaintext::language,
        highlight: Some(tree_sitter_plaintext::HIGHLIGHTS_QUERY),
        injection: Some(tree_sitter_plaintext::INJECTIONS_QUERY),
        code_lens: (&[], &[]),
        sticky_headers: &[],
    }),
};

struct SyntaxProperties {
    /// An extra check to make sure that the array elements are in the correct order.  
    /// If this id does not match the enum value, a panic will happen with a debug assertion message.
    id: LapceLanguage,
    // /// Single line comment token used when commenting out one line.
    // /// "#" for python, "//" for rust for example.
    // single_line_comment_token: &'static str,
    // /// Multi line comment token used when commenting a selection of lines.
    // /// "#" for python, "//" for rust for example.
    // multi_line_comment_token: &'static str,
    // /// All tokens that can be used for comments in language
    // comment_tokens: &'static [&'static str],
    comment: CommentProperties,

    /// The indent unit.  
    /// "\t" for python, "    " for rust, for example.
    indent: &'static str,
    /// Filenames that belong to this language  
    /// `["Dockerfile"]` for Dockerfile, `[".editorconfig"]` for EditorConfig
    files: &'static [&'static str],
    /// File name extensions to determine the language.  
    /// `["py"]` for python, `["rs"]` for rust, for example.
    extensions: &'static [&'static str],
    /// Tree-sitter properties
    tree_sitter: Option<TreeSitterProperties>,
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord)]
struct TreeSitterProperties {
    /// This is the factory function defined in the tree-sitter crate that creates the language parser.  
    /// For most languages, it is `tree_sitter_$crate::language`.
    language: fn() -> tree_sitter::Language,
    /// For most languages, it is `tree_sitter_$crate::HIGHLIGHT_QUERY`.
    highlight: Option<&'static str>,
    /// For most languages, it is `tree_sitter_$crate::INJECTION_QUERY`.  
    /// Though, not all languages have injections.
    injection: Option<&'static str>,
    /// The comment token.  "#" for python, "//" for rust for example.
    comment: &'static str,
    /// The indent unit.  "  " for javascript, "    " for rust, for example.
    indent: &'static str,
        /// Lists of tree-sitter node types that control how code lenses are built.
    /// The first is a list of nodes that should be traversed and included in
    /// the lens, along with thier children. The second is a list of nodes that
    /// should be excluded from the lens, though they will still be traversed.
    /// See `walk_tree` for more details.
    ///
    /// The tree-sitter playground may be useful when creating these lists:
    /// https://tree-sitter.github.io/tree-sitter/playground
    ///
    /// If unsure, use `DEFAULT_CODE_LENS_LIST` and
    /// `DEFAULT_CODE_LENS_IGNORE_LIST`.
    code_lens: (&'static [&'static str], &'static [&'static str]),
    /// the tree sitter tag names that can be put in sticky headers
    sticky_headers: &'static [&'static str],
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord)]
struct CommentProperties {
    single_line_start: &'static str,
    single_line_end: &'static str,

    multi_line_start: &'static str,
    multi_line_end: &'static str,
    multi_line_prefix: &'static str,
}

/// NOTE: Keep the enum variants "fieldless" so they can cast to usize as array
/// indices into the LANGUAGES array.  See method `LapceLanguage::properties`.
///
/// Do not assign values to the variants because the number of variants and
/// number of elements in the LANGUAGES array change as different features
/// selected by the cargo build command.
#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, Display, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum LapceLanguage {
    // Do not move
    Plaintext,

    #[cfg(feature = "lang-bash")]
    #[strum(serialize = "bash", serialize = "sh")]
    Bash,
    #[cfg(feature = "lang-c")]
    C,
    #[cfg(feature = "lang-clojure")]
    Clojure,
    #[cfg(feature = "lang-cmake")]
    Cmake,
    #[cfg(feature = "lang-cpp")]
    Cpp,
    #[cfg(feature = "lang-csharp")]
    Csharp,
    #[cfg(feature = "lang-css")]
    Css,
    #[cfg(feature = "lang-d")]
    D,
    #[cfg(feature = "lang-dart")]
    Dart,
    #[cfg(feature = "lang-dockerfile")]
    Dockerfile,
    #[cfg(feature = "lang-elixir")]
    Elixir,
    #[cfg(feature = "lang-elm")]
    Elm,
    #[cfg(feature = "lang-erlang")]
    Erlang,
    #[cfg(feature = "lang-glimmer")]
    Glimmer,
    #[cfg(feature = "lang-glsl")]
    Glsl,
    #[cfg(feature = "lang-go")]
    Go,
    #[cfg(feature = "lang-hare")]
    Hare,
    #[cfg(feature = "lang-haskell")]
    Haskell,
    #[cfg(feature = "lang-haxe")]
    Haxe,
    #[strum(serialize = "HCL2")]
    #[cfg(feature = "lang-hcl")]
    Hcl,
    #[strum(serialize = "HTML")]
    #[cfg(feature = "lang-html")]
    Html,
    #[cfg(feature = "lang-java")]
    Java,
    #[strum(serialize = "JavaScript")]
    #[cfg(feature = "lang-javascript")]
    Javascript,
    #[strum(serialize = "JSON")]
    #[cfg(feature = "lang-json")]
    Json,
    #[strum(serialize = "JavaScript React")]
    #[cfg(feature = "lang-javascript")]
    Jsx,
    #[cfg(feature = "lang-julia")]
    Julia,
    #[cfg(feature = "lang-kotlin")]
    Kotlin,
    #[strum(serialize = "LaTeX")]
    #[cfg(feature = "lang-latex")]
    Latex,
    #[cfg(feature = "lang-lua")]
    Lua,
    #[cfg(feature = "lang-markdown")]
    Markdown,
    #[cfg(feature = "lang-markdown")]
    #[strum(serialize = "markdown.inline")]
    MarkdownInline,
    #[cfg(feature = "lang-nix")]
    Nix,
    #[cfg(feature = "lang-ocaml")]
    Ocaml,
    #[cfg(feature = "lang-ocaml")]
    OcamlInterface,
    #[cfg(feature = "lang-php")]
    Php,
    #[cfg(feature = "lang-prisma")]
    Prisma,
    #[cfg(feature = "lang-protobuf")]
    ProtoBuf,
    #[cfg(feature = "lang-python")]
    Python,
    #[cfg(feature = "lang-ql")]
    Ql,
    #[cfg(feature = "lang-r")]
    R,
    #[cfg(feature = "lang-ruby")]
    Ruby,
    #[cfg(feature = "lang-rust")]
    Rust,
    #[cfg(feature = "lang-scheme")]
    Scheme,
    Scss,
    #[strum(serialize = "POSIX Shell")]
    #[cfg(feature = "lang-bash")]
    Sh,
    #[strum(serialize = "SQL")]
    #[cfg(feature = "lang-sql")]
    Sql,
    #[cfg(feature = "lang-svelte")]
    Svelte,
    #[cfg(feature = "lang-swift")]
    Swift,
    #[strum(serialize = "TOML")]
    #[cfg(feature = "lang-toml")]
    Toml,
    #[strum(serialize = "TypeScript React")]
    #[cfg(feature = "lang-typescript")]
    Tsx,
    #[cfg(feature = "lang-typescript")]
    Typescript,
    #[cfg(feature = "lang-vue")]
    Vue,
    #[strum(serialize = "WGSL")]
    #[cfg(feature = "lang-wgsl")]
    Wgsl,
    #[strum(serialize = "XML")]
    #[cfg(feature = "lang-xml")]
    Xml,
    #[strum(serialize = "YAML")]
    #[cfg(feature = "lang-yaml")]
    Yaml,
    #[cfg(feature = "lang-zig")]
    Zig,
}

/// NOTE: Elements in the array must be in the same order as the enum variants of
/// `LapceLanguage` as they will be accessed using the enum variants as indices.
const LANGUAGES: &[SyntaxProperties] = &[
    // Plaintext language
    EMPTY_LANGUAGE,
    // Languages
    SyntaxProperties {
        id: LapceLanguage::Bash,

        indent: "  ",
        files: &[],
        extensions: &["sh", "bash"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_bash::language,
            highlight: Some(include_str!("../queries/bash/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::C,

        indent: "    ",
        files: &[],
        extensions: &["c", "h"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_c::language,
            highlight: Some(include_str!("../queries/c/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["function_definition", "struct_specifier"],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    #[cfg(feature = "lang-clojure")]
    SyntaxProperties {
        id: LapceLanguage::Clojure,
        language: tree_sitter_clojure::language,
        highlight: include_str!("../queries/clojure/highlights.scm"),
        injection: Some(include_str!("../queries/clojure/injections.scm")),
        comment: ";",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &[
            "clj",
            "edn",
            "cljs",
            "cljc",
            "cljd",
            "edn",
            "bb",
            "clj_kondo",
        ],
    },
    SyntaxProperties {
        id: LapceLanguage::Cmake,

        indent: "  ",
        files: &[],
        extensions: &["cmake"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "lang-cmake")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_cmake::language,
            highlight: include_str!("../queries/cmake/highlights.scm"),
            injection: Some(include_str!("../queries/cmake/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["function_definition"],
        }),
        #[cfg(not(feature = "lang-cmake"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Cpp,

        indent: "    ",
        files: &[],
        extensions: &["cpp", "cxx", "cc", "c++", "hpp", "hxx", "hh", "h++"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_cpp::language,
            highlight: Some(include_str!("../queries/cpp/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[
                "function_definition",
                "class_specifier",
                "struct_specifier",
            ],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Csharp,

        indent: "  ",
        files: &[],
        extensions: &["cs", "csx"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_c_sharp::language,
            highlight: Some(tree_sitter_c_sharp::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[
                "interface_declaration",
                "class_declaration",
                "enum_declaration",
                "struct_declaration",
                "record_declaration",
                "record_struct_declaration",
                "namespace_declaration",
                "constructor_declaration",
                "destructor_declaration",
                "method_declaration",
            ],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Css,

        indent: "  ",
        files: &[],
        extensions: &["css"],

        comment: CommentProperties {
            single_line_start: "/*",
            single_line_end: "*/",

            multi_line_start: "/*",
            multi_line_prefix: "",
            multi_line_end: "*/",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_css::language,
            highlight: Some(include_str!("../queries/css/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::D,

        indent: "    ",
        files: &[],
        extensions: &["d", "di", "dlang"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_d::language,
            highlight: Some(tree_sitter_d::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Dart,

        indent: "  ",
        files: &[],
        extensions: &["dart"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_dart::language,
            highlight: Some(tree_sitter_dart::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (&["program", "class_definition"],
            &[
                "program",
                "import_or_export",
                "comment",
                "documentation_comment",
            ],),
            sticky_headers: &["class_definition"],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Dockerfile,

        indent: "  ",
        files: &["dockerfile", "containerfile"],
        extensions: &["containerfile", "dockerfile"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_dockerfile::language,
            highlight: Some(tree_sitter_dockerfile::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Elixir,

        indent: "  ",
        files: &[],
        extensions: &["ex", "exs", "eex", "heex", "sface"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_elixir::language,
            highlight: Some(tree_sitter_elixir::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["do_block"],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Elm,

        indent: "    ",
        files: &[],
        extensions: &["elm"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_elm::language,
            highlight: Some(include_str!("../queries/elm/highlights.scm")),
            injection: Some(tree_sitter_elm::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Erlang,

        indent: "    ",
        files: &[],
        extensions: &["erl", "hrl"],

        comment: CommentProperties {
            single_line_start: "%",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_erlang::language,
            highlight: Some(include_str!("../queries/erlang/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Glimmer,

        indent: "  ",
        files: &[],
        extensions: &["hbs"],

        comment: CommentProperties {
            single_line_start: "{{!",
            single_line_end: "!}}",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_glimmer::language,
            highlight: Some(tree_sitter_glimmer::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Glsl,

        indent: "  ",
        files: &[],
        extensions: &[
            "glsl", "cs", "vs", "gs", "fs", "csh", "vsh", "gsh", "fsh", "cshader",
            "vshader", "gshader", "fshader", "comp", "vert", "geom", "frag", "tesc",
            "tese", "mesh", "task", "rgen", "rint", "rahit", "rchit", "rmiss",
            "rcall",
        ],
        
        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_glsl::language,
            highlight: Some(tree_sitter_glsl::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Go,

        indent: "    ",
        files: &[],
        extensions: &["go"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_go::language,
            highlight: Some(tree_sitter_go::HIGHLIGHT_QUERY),
            injection: None,
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
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Hare,

        indent: "        ",
        files: &[],
        extensions: &["ha"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_hare::language,
            highlight: Some(tree_sitter_hare::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Haskell,

        indent: "  ",
        files: &[],
        extensions: &["hs"],

        comment: CommentProperties {
            single_line_start: "--",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_haskell::language,
            highlight: Some(tree_sitter_haskell::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Haxe,

        indent: "  ",
        files: &[],
        extensions: &["hx"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_haxe::language,
            highlight: Some(tree_sitter_haxe::HIGHLIGHTS_QUERY),
            injection: Some(tree_sitter_haxe::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Hcl,

        indent: "  ",
        files: &[],
        extensions: &["hcl", "tf"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_hcl::language,
            highlight: Some(tree_sitter_hcl::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Html,

        indent: "    ",
        files: &[],
        extensions: &["html", "htm"],

        comment: CommentProperties {
            single_line_start: "<!--",
            single_line_end: "-->",

            multi_line_start: "<!--",
            multi_line_prefix: "",
            multi_line_end: "-->",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_html::language,
            highlight: Some(tree_sitter_html::HIGHLIGHT_QUERY),
            injection: Some(tree_sitter_html::INJECTION_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Java,

        indent: "    ",
        files: &[],
        extensions: &["java"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_java::language,
            highlight: Some(tree_sitter_java::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Javascript,

        indent: "  ",
        files: &[],
        extensions: &["js", "cjs", "mjs"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_javascript::language,
            highlight: Some(include_str!("../queries/javascript/highlights.scm")),
            injection: Some(tree_sitter_javascript::INJECTION_QUERY),
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Json,

        indent: "    ",
        files: &[],
        extensions: &["json"],

        comment: CommentProperties {
            single_line_start: "",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_dart::language,
            highlight: Some(tree_sitter_dart::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Jsx,

        indent: "  ",
        files: &[],
        extensions: &["jsx"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_javascript::language,
            highlight: Some(include_str!("../queries/jsx/highlights.scm")),
            // TODO: Does jsx use the javascript injection query too?
            injection: Some(tree_sitter_javascript::INJECTION_QUERY),
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Julia,

        indent: "    ",
        files: &[],
        extensions: &["julia", "jl"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_julia::language,
            highlight: Some(include_str!("../queries/julia/highlights.scm")),
            injection: Some(include_str!("../queries/julia/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Kotlin,

        indent: "  ",
        files: &[],
        extensions: &["kt", "kts"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_kotlin::language,
            highlight: Some(include_str!("../queries/kotlin/highlights.scm")),
            injection: Some(include_str!("../queries/kotlin/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Latex,

        indent: "  ",
        files: &[],
        extensions: &["tex"],

        comment: CommentProperties {
            single_line_start: "%",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_latex::language,
            highlight: Some(include_str!("../queries/latex/highlights.scm")),
            injection: Some(include_str!("../queries/latex/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Lua,

        indent: "  ",
        files: &[],
        extensions: &["lua"],

        comment: CommentProperties {
            single_line_start: "--",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_lua::language,
            highlight: Some(include_str!("../queries/lua/highlights.scm")),
            injection: None,
            sticky_headers: &[],
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Markdown,

        indent: "    ",
        files: &[],
        extensions: &["md"],

        comment: CommentProperties {
            single_line_start: "",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_md::language,
            highlight: Some(include_str!("../queries/markdown/highlights.scm")),
            injection: Some(include_str!("../queries/markdown/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::MarkdownInline,

        indent: "    ",
        // markdown inline is only used as an injection by the Markdown language
        files: &[],
        extensions: &[],

        comment: CommentProperties {
            single_line_start: "",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_md::inline_language,
            highlight: Some(include_str!(
                "../queries/markdown.inline/highlights.scm"
            )),
            injection: Some(include_str!(
                "../queries/markdown.inline/injections.scm"
            )),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Nix,

        indent: "  ",
        files: &[],
        extensions: &["nix"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_nix::language,
            highlight: Some(tree_sitter_nix::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ocaml,

        indent: "  ",
        files: &[],
        extensions: &["ml"],

        comment: CommentProperties {
            single_line_start: "(*",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ocaml::language_ocaml,
            highlight: Some(tree_sitter_ocaml::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::OcamlInterface,

        indent: "  ",
        files: &[],
        extensions: &["mli"],

        comment: CommentProperties {
            single_line_start: "(*",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ocaml::language_ocaml_interface,
            highlight: Some(tree_sitter_ocaml::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Php,

        indent: "  ",
        files: &[],
        extensions: &["php"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_php::language,
            highlight: Some(tree_sitter_php::HIGHLIGHT_QUERY),
            injection: Some(tree_sitter_php::INJECTIONS_QUERY),
            code_lens: (            &[
                "program",
                "class_declaration",
                "trait_declaration",
                "interface_declaration",
                "declaration_list",
                "method_declaration",
                "function_declaration",
            ],
            &[
                "program",
                "php_tag",
                "comment",
                "namespace_definition",
                "namespace_use_declaration",
                "use_declaration",
                "const_declaration",
                "property_declaration",
                "expression_statement",
            ],),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    #[cfg(feature = "lang-prisma")]
    SyntaxProperties {
        id: LapceLanguage::Prisma,

        indent: "    ",
        files: &[],
        extensions: &["prisma"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_prisma_io::language,
            highlight: Some(include_str!("../queries/prisma/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    #[cfg(feature = "lang-protobuf")]
    SyntaxProperties {
        id: LapceLanguage::ProtoBuf,

        indent: "  ",
        files: &[],
        extensions: &["proto"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_protobuf::language,
            highlight: Some(include_str!("../queries/protobuf/highlights.scm")),
            injection: Some(include_str!("../queries/protobuf/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Python,

        indent: "    ",
        files: &[],
        extensions: &["py", "pyi", "pyc", "pyd", "pyw"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_python::language,
            highlight: Some(tree_sitter_python::HIGHLIGHT_QUERY),
            injection: None,
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
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ql,

        indent: "  ",
        files: &[],
        extensions: &["ql"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ql::language,
            highlight: Some(tree_sitter_ql::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::R,

        indent: "  ",
        files: &[],
        extensions: &["r"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_r::language,
            highlight: Some(include_str!("../queries/r/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ruby,

        indent: "  ",
        files: &[],
        extensions: &["rb"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ruby::language,
            highlight: Some(tree_sitter_ruby::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["module", "class", "method", "do_block"],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Rust,

        indent: "    ",
        files: &[],
        extensions: &["rs"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_rust::language,
            highlight: Some(tree_sitter_rust::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (
                &["source_file", "impl_item", "trait_item", "declaration_list"],
                &["source_file", "use_declaration", "line_comment"],
            ),
            sticky_headers: &[
                "struct_item",
                "enum_item",
                "function_item",
                "impl_item",
            ],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Scheme,

        indent: "  ",
        files: &[],
        extensions: &["scm", "ss"],

        comment: CommentProperties {
            single_line_start: ";",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_scheme::language,
            highlight: Some(tree_sitter_scheme::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Scss,

        indent: "  ",
        files: &[],
        extensions: &["scss"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_scss::language,
            highlight: Some(tree_sitter_scss::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Sh,

        indent: "  ",
        files: &[],
        extensions: &["sh"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_bash::language,
            highlight: Some(include_str!("../queries/bash/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Sql,

        indent: "  ",
        files: &[],
        extensions: &["sql"],

        comment: CommentProperties {
            single_line_start: "--",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_sql::language,
            highlight: Some(tree_sitter_sql::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Svelte,

        indent: "  ",
        files: &[],
        extensions: &["svelte"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_svelte::language,
            highlight: Some(include_str!("../queries/svelte/highlights.scm")),
            injection: Some(include_str!("../queries/svelte/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Swift,

        indent: "  ",
        files: &[],
        extensions: &["swift"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_swift::language,
            highlight: Some(tree_sitter_swift::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Toml,

        indent: "  ",
        files: &[],
        extensions: &["toml"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_toml::language,
            highlight: Some(tree_sitter_toml::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Tsx,

        indent: "    ",
        files: &[],
        extensions: &["tsx"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_typescript::language_tsx,
            highlight: Some(include_str!("../queries/typescript/highlights.scm")),
            injection: None,
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Typescript,

        indent: "    ",
        files: &[],
        extensions: &["ts", "cts", "mts"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_typescript::language_typescript,
            highlight: Some(include_str!("../queries/typescript/highlights.scm")),
            injection: None,
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Vue,

        indent: "  ",
        files: &[],
        extensions: &["vue"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_vue::language,
            highlight: Some(tree_sitter_vue::HIGHLIGHTS_QUERY),
            injection: Some(tree_sitter_vue::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Wgsl,

        indent: "    ",
        files: &[],
        extensions: &["wgsl"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_wgsl::language,
            highlight: Some(tree_sitter_wgsl::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Xml,

        indent: "    ",
        files: &[],
        extensions: &["xml"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_xml::language,
            highlight: Some(tree_sitter_xml::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Yaml,

        indent: "  ",
        files: &[],
        extensions: &["yml", "yaml"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_yaml::language,
            highlight: Some(tree_sitter_yaml::HIGHLIGHTS_QUERY),
            injection: Some(tree_sitter_yaml::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Zig,

        indent: "    ",
        files: &[],
        extensions: &["zig"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_zig::language,
            highlight: Some(include_str!("../queries/zig/highlights.scm")),
            injection: Some(tree_sitter_zig::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
];

impl LapceLanguage {
    pub fn from_path(path: &Path) -> Option<LapceLanguage> {
        let filename = path.file_stem()?.to_str()?.to_lowercase();
        let extension = path.extension()?.to_str()?.to_lowercase();
        // NOTE: This is a linear search.  It is assumed that this function
        // isn't called in any tight loop.
        for properties in LANGUAGES {
            if properties.files.contains(&filename.as_str())
                || properties.extensions.contains(&extension.as_str())
            {
                return Some(properties.id);
            }
        }
        None
    }

    pub fn from_name(name: &str) -> Option<LapceLanguage> {
        match LapceLanguage::from_str(name.to_lowercase().as_str()) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("failed parsing {name} LapceLanguage: {e}");
                None
            }
        }
    }

    pub fn languages() -> Vec<String> {
        let mut langs = vec![];
        for l in LANGUAGES {
            langs.push(format!("{}", l.id))
        }
        langs
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

    fn tree_sitter(&self) -> TreeSitterProperties {
        if let Some(ts) = self.properties().tree_sitter {
            ts
        } else {
            EMPTY_LANGUAGE.tree_sitter.unwrap()
        }
    }

    pub fn sticky_header_tags(&self) -> &[&'static str] {
        if let Some(ts) = self.properties().tree_sitter {
            ts.sticky_headers
        } else {
            &[]
        }
    }

    pub fn comment_token(&self) -> &str {
        self.properties().comment.single_line_start
    }

    pub fn indent_unit(&self) -> &str {
        self.properties().indent
    }

    pub(crate) fn new_highlight_config(
        &self,
    ) -> Result<HighlightConfiguration, HighlightIssue> {
        let props = self.properties();

        let mut language = match props.tree_sitter {
            Some(v) => (v.language)(),
            None => unimplemented!(),
        };
        if let Some(grammars_dir) = Directory::grammars_directory() {
            /*
             * This Source Code Form is subject to the terms of the Mozilla Public
             * License, v. 2.0. If a copy of the MPL was not distributed with this
             * file, You can obtain one at https://mozilla.org/MPL/2.0/.
             *
             * Below code is modified from [helix](https://github.com/helix-editor/helix)'s implementation of their tree-sitter loading, which is under the MPL.
             */
            let mut library_path =
                grammars_dir.join(props.id.to_string().to_lowercase());
            library_path.set_extension(std::env::consts::DLL_EXTENSION);

            if library_path.exists() {
                let library =
                    unsafe { libloading::Library::new(&library_path) }.unwrap();
                let language_fn_name = format!("tree-sitter-{}", props.id);
                language = unsafe {
                    let language_fn: libloading::Symbol<
                        unsafe extern "C" fn() -> tree_sitter::Language,
                    > = library.get(language_fn_name.as_bytes()).unwrap();
                    language_fn()
                };
                std::mem::forget(library);
            }
        }

        let mut highlight = String::new();
        if let Some(queries_dir) = Directory::queries_directory() {
            let queries_dir = queries_dir.join(props.id.to_string().to_lowercase());
            if queries_dir.exists() {
                let highlights_file = queries_dir.join("highlights.scm");
                if highlights_file.exists() {
                    highlight =
                        std::fs::read_to_string(highlights_file).unwrap_or_default()
                }
            } else {
                _ = std::fs::DirBuilder::new()
                    .recursive(true)
                    .create(queries_dir);
            }
        }
        let query = if !highlight.is_empty() {
            highlight.as_str()
        } else {
            props.tree_sitter.unwrap().highlight.unwrap_or_default()
        };
        let injection = props.tree_sitter.unwrap().injection.unwrap_or_default();

        match HighlightConfiguration::new(language, query, injection, "") {
            Ok(x) => Ok(x),
            Err(x) => {
                let str = format!("Encountered {x:?} while trying to construct HighlightConfiguration for {self}");
                log::error!("{str}");
                Err(HighlightIssue::Error(str))
            }
        }
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        let (list, ignore_list) = self.tree_sitter().code_lens;
        walk_tree(cursor, normal_lines, list, ignore_list);
    }
}

/// Walk an AST and determine which lines to include in the code lens.
///
/// Node types listed in `list` will be walked, along with their children. All
/// nodes encountered will be included, unless they are listed in `ignore_list`.
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
