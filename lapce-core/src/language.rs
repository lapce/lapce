use std::{collections::HashSet, path::Path, str::FromStr};

use strum_macros::{Display, EnumMessage, EnumString};
use tree_sitter::TreeCursor;

use crate::{
    directory::Directory, syntax::highlight::HighlightConfiguration,
    syntax::highlight::HighlightIssue,
};

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
    code_lens: (&[], &[]),
    sticky_headers: &[],

    tree_sitter: Some(TreeSitterProperties {
        language: tree_sitter_plaintext::language,
        highlight: Some(tree_sitter_plaintext::HIGHLIGHTS_QUERY),
        injection: Some(tree_sitter_plaintext::INJECTIONS_QUERY),
    }),
};

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
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
    /// "  " for bash, "    " for rust, for example.
    indent: &'static str,
    /// Filenames that belong to this language  
    /// `["Dockerfile"]` for Dockerfile, `[".editorconfig"]` for EditorConfig
    files: &'static [&'static str],
    /// File name extensions to determine the language.  
    /// `["py"]` for python, `["rs"]` for rust, for example.
    extensions: &'static [&'static str],
    /// Tuple of lists to preserve and hide when using Lapce code lens feature
    code_lens: (&'static [&'static str], &'static [&'static str]),
    /// Tree sitter tag names that can be put in sticky headers
    /// Not part of tree sitter config since those are just defaults
    sticky_headers: &'static [&'static str],

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
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
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
#[derive(
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Clone,
    Copy,
    Debug,
    Display,
    EnumString,
    EnumMessage,
    Default,
)]
#[strum(ascii_case_insensitive)]
pub enum LapceLanguage {
    // Do not move
    #[default]
    #[strum(message = "Plain Text")]
    Plaintext,

    Bash,
    C,
    #[strum(message = "CMake")]
    Cmake,
    #[strum(message = "C++")]
    Cpp,
    #[strum(message = "C#")]
    Csharp,
    #[strum(message = "CSS")]
    Css,
    D,
    Dart,
    Dockerfile,
    Elixir,
    Elm,
    Erlang,
    Glimmer,
    Glsl,
    Go,
    Hare,
    Haskell,
    Haxe,
    #[strum(message = "HCL")]
    Hcl,
    #[strum(message = "HTML")]
    Html,
    Java,
    #[strum(message = "JavaScript")]
    Javascript,
    #[strum(message = "JSON")]
    Json,
    #[strum(message = "JavaScript React")]
    Jsx,
    Julia,
    Kotlin,
    #[strum(message = "LaTeX")]
    Latex,
    Lua,
    Markdown,
    #[strum(serialize = "markdown.inline")]
    MarkdownInline,
    Nix,
    Ocaml,
    OcamlInterface,
    #[strum(message = "PHP")]
    Php,
    Prisma,
    ProtoBuf,
    Python,
    Ql,
    R,
    Ruby,
    Rust,
    Scheme,
    #[strum(message = "SCSS")]
    Scss,
    #[strum(message = "POSIX Shell")]
    Sh,
    #[strum(message = "SQL")]
    Sql,
    Svelte,
    Swift,
    #[strum(message = "TOML")]
    Toml,
    #[strum(message = "TypeScript React")]
    Tsx,
    #[strum(message = "TypeScript")]
    Typescript,
    Vue,
    Wgsl,
    #[strum(message = "XML")]
    Xml,
    #[strum(message = "YAML")]
    Yaml,
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
        extensions: &["bash"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
            highlight: Some(tree_sitter_bash::HIGHLIGHT_QUERY),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::C,

        indent: "    ",
        files: &[],
        extensions: &["c", "h"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["function_definition", "struct_specifier"],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Cmake,

        indent: "  ",
        files: &[],
        extensions: &["cmake"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["function_definition"],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "",
            multi_line_prefix: "",
            multi_line_end: "",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_cmake::language,
            highlight: Some(include_str!("../queries/cmake/highlights.scm")),
            injection: Some(include_str!("../queries/cmake/injections.scm")),
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Cpp,

        indent: "    ",
        files: &[],
        extensions: &["cpp", "cxx", "cc", "c++", "hpp", "hxx", "hh", "h++"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[
            "function_definition",
            "class_specifier",
            "struct_specifier",
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
            language: tree_sitter_cpp::language,
            highlight: Some(include_str!("../queries/cpp/highlights.scm")),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Csharp,

        indent: "  ",
        files: &[],
        extensions: &["cs", "csx"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Css,

        indent: "  ",
        files: &[],
        extensions: &["css"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::D,

        indent: "    ",
        files: &[],
        extensions: &["d", "di", "dlang"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "/+",
            multi_line_prefix: "",
            multi_line_end: "+/",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_d::language,
            highlight: Some(tree_sitter_d::HIGHLIGHTS_QUERY),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Dart,

        indent: "  ",
        files: &[],
        extensions: &["dart"],
        code_lens: (
            &["program", "class_definition"],
            &[
                "program",
                "import_or_export",
                "comment",
                "documentation_comment",
            ],
        ),
        sticky_headers: &["class_definition"],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "/*",
            multi_line_prefix: "",
            multi_line_end: "*/",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_dart::language,
            highlight: Some(tree_sitter_dart::HIGHLIGHTS_QUERY),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Dockerfile,

        indent: "  ",
        files: &["dockerfile", "containerfile"],
        extensions: &["containerfile", "dockerfile"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Elixir,

        indent: "  ",
        files: &[],
        extensions: &["ex", "exs", "eex", "heex", "sface"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["do_block"],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Elm,

        indent: "    ",
        files: &[],
        extensions: &["elm"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Erlang,

        indent: "    ",
        files: &[],
        extensions: &["erl", "hrl"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Glimmer,

        indent: "  ",
        files: &[],
        extensions: &["hbs"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Go,

        indent: "    ",
        files: &[],
        extensions: &["go"],
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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Hare,

        indent: "        ",
        files: &[],
        extensions: &["ha"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Haskell,

        indent: "  ",
        files: &[],
        extensions: &["hs"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Haxe,

        indent: "  ",
        files: &[],
        extensions: &["hx"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Hcl,

        indent: "  ",
        files: &[],
        extensions: &["hcl", "tf"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Html,

        indent: "    ",
        files: &[],
        extensions: &["html", "htm"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Java,

        indent: "  ",
        files: &[],
        extensions: &["java"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Javascript,

        indent: "  ",
        files: &[],
        extensions: &["js", "cjs", "mjs"],
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Json,

        indent: "    ",
        files: &[],
        extensions: &["json"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Jsx,

        indent: "  ",
        files: &[],
        extensions: &["jsx"],
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Julia,

        indent: "    ",
        files: &[],
        extensions: &["julia", "jl"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "#=",
            multi_line_prefix: "",
            multi_line_end: "=#",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_julia::language,
            highlight: Some(include_str!("../queries/julia/highlights.scm")),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Kotlin,

        indent: "  ",
        files: &[],
        extensions: &["kt", "kts"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

        comment: CommentProperties {
            single_line_start: "//",
            single_line_end: "",

            multi_line_start: "/*",
            multi_line_prefix: "",
            multi_line_end: "*/",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_kotlin::language,
            highlight: Some(include_str!("../queries/kotlin/highlights.scm")),
            injection: Some(include_str!("../queries/kotlin/injections.scm")),
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Latex,

        indent: "  ",
        files: &[],
        extensions: &["tex"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Lua,

        indent: "  ",
        files: &[],
        extensions: &["lua"],
        sticky_headers: &[],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Markdown,

        indent: "    ",
        files: &[],
        extensions: &["md"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Nix,

        indent: "  ",
        files: &[],
        extensions: &["nix"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

        comment: CommentProperties {
            single_line_start: "#",
            single_line_end: "",

            multi_line_start: "/*",
            multi_line_prefix: "",
            multi_line_end: "*/",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_nix::language,
            highlight: Some(tree_sitter_nix::HIGHLIGHTS_QUERY),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ocaml,

        indent: "  ",
        files: &[],
        extensions: &["ml"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

        comment: CommentProperties {
            single_line_start: "(*",
            single_line_end: "*)",

            multi_line_start: "(*",
            multi_line_prefix: "*",
            multi_line_end: "*)",
        },

        #[cfg(feature = "compile-grammars")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ocaml::language_ocaml,
            highlight: Some(tree_sitter_ocaml::HIGHLIGHTS_QUERY),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::OcamlInterface,

        indent: "  ",
        files: &[],
        extensions: &["mli"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Php,

        indent: "  ",
        files: &[],
        extensions: &["php"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Prisma,

        indent: "    ",
        files: &[],
        extensions: &["prisma"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::ProtoBuf,

        indent: "  ",
        files: &[],
        extensions: &["proto"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Python,

        indent: "    ",
        files: &[],
        extensions: &["py", "pyi", "pyc", "pyd", "pyw"],
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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ql,

        indent: "  ",
        files: &[],
        extensions: &["ql"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::R,

        indent: "  ",
        files: &[],
        extensions: &["r"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ruby,

        indent: "  ",
        files: &[],
        extensions: &["rb"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["module", "class", "method", "do_block"],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Rust,

        indent: "    ",
        files: &[],
        extensions: &["rs"],
        code_lens: (
            &["source_file", "impl_item", "trait_item", "declaration_list"],
            &["source_file", "use_declaration", "line_comment"],
        ),
        sticky_headers: &["struct_item", "enum_item", "function_item", "impl_item"],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Scheme,

        indent: "  ",
        files: &[],
        extensions: &["scm", "ss"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Scss,

        indent: "  ",
        files: &[],
        extensions: &["scss"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Sh,

        indent: "  ",
        files: &[],
        extensions: &["sh"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
            highlight: Some(tree_sitter_bash::HIGHLIGHT_QUERY),
            injection: None,
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Sql,

        indent: "  ",
        files: &[],
        extensions: &["sql"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Svelte,

        indent: "  ",
        files: &[],
        extensions: &["svelte"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Swift,

        indent: "  ",
        files: &[],
        extensions: &["swift"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Toml,

        indent: "  ",
        files: &[],
        extensions: &["toml"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Tsx,

        indent: "    ",
        files: &[],
        extensions: &["tsx"],
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Typescript,

        indent: "    ",
        files: &[],
        extensions: &["ts", "cts", "mts"],
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Vue,

        indent: "  ",
        files: &[],
        extensions: &["vue"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Wgsl,

        indent: "    ",
        files: &[],
        extensions: &["wgsl"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Xml,

        indent: "    ",
        files: &[],
        extensions: &["xml"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Yaml,

        indent: "  ",
        files: &[],
        extensions: &["yml", "yaml"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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
        }),
        #[cfg(not(feature = "compile-grammars"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Zig,

        indent: "    ",
        files: &[],
        extensions: &["zig"],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],

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

    // fn tree_sitter(&self) -> TreeSitterProperties {
    //     if let Some(ts) = self.properties().tree_sitter {
    //         ts
    //     } else {
    //         EMPTY_LANGUAGE.tree_sitter.unwrap()
    //     }
    // }

    pub fn sticky_header_tags(&self) -> &[&'static str] {
        self.properties().sticky_headers
    }

    pub fn comment_token(&self) -> &str {
        self.properties().comment.single_line_start
    }

    pub fn indent_unit(&self) -> &str {
        self.properties().indent
    }

    #[allow(dead_code)]
    pub(crate) fn new_highlight_config(
        &self,
    ) -> Result<HighlightConfiguration, HighlightIssue> {
        let props = self.properties();

        let mut language = (EMPTY_LANGUAGE.tree_sitter.unwrap().language)();
        if let Some(grammars_dir) = Directory::grammars_directory() {
            /*
             * This Source Code Form is subject to the terms of the Mozilla Public
             * License, v. 2.0. If a copy of the MPL was not distributed with this
             * file, You can obtain one at https://mozilla.org/MPL/2.0/.
             *
             * Below part is modified form of code from [helix](https://github.com/helix-editor/helix)'s implementation of their tree-sitter loading, which is licenced under MPL.
             */
            let mut library_path =
                grammars_dir.join(props.id.to_string().to_lowercase());

            // TODO: implement custom languages pulled from settings?

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
        } else if let Some(ts) = props.tree_sitter {
            language = (ts.language)();
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

        // HighlightConfiguration::new(language, query, injection, "").unwrap()
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
