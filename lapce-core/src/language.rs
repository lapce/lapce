use std::{collections::HashSet, path::Path, str::FromStr};

use anyhow::Result;
use once_cell::sync::Lazy;
use strum_macros::{AsRefStr, AsStaticStr, Display, EnumMessage, EnumString};
use tracing::{debug, error};
use tree_sitter::TreeCursor;

use crate::{
    directory::Directory,
    syntax::highlight::{HighlightConfiguration, HighlightIssue},
};

pub static RUNTIME_LANGUAGES: Lazy<Vec<SyntaxProperties>> = Lazy::new(Vec::new);

#[allow(dead_code)]
const DEFAULT_CODE_LENS_LIST: &[&str] = &["source_file"];
#[allow(dead_code)]
const DEFAULT_CODE_LENS_IGNORE_LIST: &[&str] = &["source_file"];
const DEFAULT_QUERIES: include_dir::Dir = include_dir::include_dir!("queries");

#[macro_export]
macro_rules! comment_properties {
    () => {
        CommentProperties {
            single_line_start: None,
            single_line_end: None,

            multi_line_start: None,
            multi_line_end: None,
            multi_line_prefix: None,
        }
    };
    ($s:expr) => {
        CommentProperties {
            single_line_start: Some($s),
            single_line_end: None,

            multi_line_start: None,
            multi_line_end: None,
            multi_line_prefix: None,
        }
    };
    ($s:expr, $e:expr) => {
        CommentProperties {
            single_line_start: Some($s),
            single_line_end: Some($e),

            multi_line_start: None,
            multi_line_end: None,
            multi_line_prefix: None,
        }
    };
    ($sl_s:expr, $sl_e:expr, $ml_s:expr, $ml_e:expr) => {
        CommentProperties {
            single_line_start: Some($sl_s),
            single_line_end: Some($sl_e),

            multi_line_start: Some($sl_s),
            multi_line_end: None,
            multi_line_prefix: Some($sl_e),
        }
    };
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
pub struct SyntaxProperties {
    /// An extra check to make sure that the array elements are in the correct order.  
    /// If this id does not match the enum value, a panic will happen with a debug assertion message.
    id: LapceLanguage,

    /// All tokens that can be used for comments in language
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
    /// Tree-sitter properties
    tree_sitter: Option<TreeSitterProperties>,
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord)]
struct TreeSitterProperties {
    /// This is the factory function defined in the tree-sitter crate that creates the language parser.  
    /// For most languages, it is `tree_sitter_$crate::language`.
    language: Option<fn() -> tree_sitter::Language>,
    /// the grammar name that's in the grammars folder
    grammar: Option<&'static str>,
    /// the query folder name
    query: Option<&'static str>,
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

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
struct CommentProperties {
    /// Single line comment token used when commenting out one line.
    /// "#" for python, "//" for rust for example.
    single_line_start: Option<&'static str>,
    single_line_end: Option<&'static str>,

    /// Multi line comment token used when commenting a selection of lines.
    /// "#" for python, "//" for rust for example.
    multi_line_start: Option<&'static str>,
    multi_line_end: Option<&'static str>,
    multi_line_prefix: Option<&'static str>,
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
    AsRefStr,
    AsStaticStr,
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

    #[strum(message = "Bash")]
    Bash,
    #[strum(message = "C")]
    C,
    #[strum(message = "Clojure")]
    Clojure,
    #[strum(message = "CMake")]
    Cmake,
    #[strum(message = "C++")]
    Cpp,
    #[strum(message = "C#")]
    Csharp,
    #[strum(message = "CSS")]
    Css,
    #[strum(message = "D")]
    D,
    #[strum(message = "Dart")]
    Dart,
    #[strum(message = "Dockerfile")]
    Dockerfile,
    #[strum(message = "Elixir")]
    Elixir,
    #[strum(message = "Elm")]
    Elm,
    #[strum(message = "Erlang")]
    Erlang,
    #[strum(message = "Glimmer")]
    Glimmer,
    #[strum(message = "GLSL")]
    Glsl,
    #[strum(message = "Go")]
    Go,
    #[strum(message = "Hare")]
    Hare,
    #[strum(message = "Haskell")]
    Haskell,
    #[strum(message = "Haxe")]
    Haxe,
    #[strum(message = "HCL")]
    Hcl,
    #[strum(message = "HTML")]
    Html,
    #[strum(message = "Java")]
    Java,
    #[strum(message = "JavaScript")]
    Javascript,
    #[strum(message = "JSON")]
    Json,
    #[strum(message = "JavaScript React")]
    Jsx,
    #[strum(message = "Julia")]
    Julia,
    #[strum(message = "Kotlin")]
    Kotlin,
    #[strum(message = "LaTeX")]
    Latex,
    #[strum(message = "Lua")]
    Lua,
    #[strum(message = "Markdown")]
    Markdown,
    #[strum(serialize = "markdown.inline")]
    MarkdownInline,
    #[strum(message = "Nix")]
    Nix,
    #[strum(message = "Ocaml")]
    Ocaml,
    #[strum(serialize = "ocaml.interface")]
    OcamlInterface,
    #[strum(message = "PHP")]
    Php,
    #[strum(message = "Prisma")]
    Prisma,
    #[strum(message = "Proto")]
    ProtoBuf,
    #[strum(message = "Python")]
    Python,
    #[strum(message = "QL")]
    Ql,
    #[strum(message = "R")]
    R,
    #[strum(message = "Ruby")]
    Ruby,
    #[strum(message = "Rust")]
    Rust,
    #[strum(message = "Scheme")]
    Scheme,
    #[strum(message = "SCSS")]
    Scss,
    #[strum(message = "Shell (POSIX)")]
    Sh,
    #[strum(message = "SQL")]
    Sql,
    #[strum(message = "Svelte")]
    Svelte,
    #[strum(message = "Swift")]
    Swift,
    #[strum(message = "TOML")]
    Toml,
    #[strum(message = "TypeScript React")]
    Tsx,
    #[strum(message = "TypeScript")]
    Typescript,
    #[strum(message = "Vue")]
    Vue,
    #[strum(message = "WGSL")]
    Wgsl,
    #[strum(message = "XML")]
    Xml,
    #[strum(message = "YAML")]
    Yaml,
    #[strum(message = "Zig")]
    Zig,
}

/// NOTE: Elements in the array must be in the same order as the enum variants of
/// `LapceLanguage` as they will be accessed using the enum variants as indices.
const LANGUAGES: &[SyntaxProperties] = &[
    // Languages
    SyntaxProperties {
        id: LapceLanguage::Plaintext,

        indent: "    ",
        files: &[],
        extensions: &[],

        comment: comment_properties!(),

        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Bash,

        indent: "  ",
        files: &[],
        extensions: &["bash"],

        comment: comment_properties!("#"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-bash")]
            language: Some(tree_sitter_bash::language),
            #[cfg(not(feature = "lang-bash"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::C,

        indent: "    ",
        files: &[],
        extensions: &["c", "h"],

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-c")]
            language: Some(tree_sitter_c::language),
            #[cfg(not(feature = "lang-c"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["function_definition", "struct_specifier"],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Clojure,

        indent: "  ",
        files: &[],
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

        comment: comment_properties!(";"),

        #[cfg(feature = "lang-clojure")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_clojure::language,
            highlight: Some(include_str!("../queries/clojure/highlights.scm")),
            injection: Some(include_str!("../queries/clojure/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-clojure"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Cmake,

        indent: "  ",
        files: &["cmakelists"],
        extensions: &["cmake"],

        comment: comment_properties!("#"),

        #[cfg(feature = "lang-cmake")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_cmake::language,
            highlight: Some(include_str!("../queries/cmake/highlights.scm")),
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

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-cpp")]
            language: Some(tree_sitter_cpp::language),
            #[cfg(not(feature = "lang-cpp"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[
                "function_definition",
                "class_specifier",
                "struct_specifier",
            ],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Csharp,

        indent: "  ",
        files: &[],
        extensions: &["cs", "csx"],

        comment: comment_properties!("#"),

        #[cfg(feature = "lang-csharp")]
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
        #[cfg(not(feature = "lang-csharp"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Css,

        indent: "  ",
        files: &[],
        extensions: &["css"],

        comment: comment_properties!("/*", "*/"),

        #[cfg(feature = "lang-css")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_css::language,
            highlight: Some(include_str!("../queries/css/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-css"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::D,

        indent: "    ",
        files: &[],
        extensions: &["d", "di", "dlang"],

        comment: CommentProperties {
            single_line_start: Some("//"),
            single_line_end: None,

            multi_line_start: Some("/+"),
            multi_line_prefix: None,
            multi_line_end: Some("+/"),
        },

        #[cfg(feature = "lang-d")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_d::language,
            highlight: Some(tree_sitter_d::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-d"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Dart,

        indent: "  ",
        files: &[],
        extensions: &["dart"],

        comment: CommentProperties {
            single_line_start: Some("//"),
            single_line_end: None,

            multi_line_start: Some("/*"),
            multi_line_prefix: None,
            multi_line_end: Some("*/"),
        },

        #[cfg(feature = "lang-dart")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_dart::language,
            highlight: Some(tree_sitter_dart::HIGHLIGHTS_QUERY),
            injection: None,
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
        }),
        #[cfg(not(feature = "lang-dart"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Dockerfile,

        indent: "  ",
        files: &["dockerfile", "containerfile"],
        extensions: &["containerfile", "dockerfile"],

        comment: comment_properties!("#"),

        #[cfg(feature = "lang-dockerfile")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_dockerfile::language,
            highlight: Some(tree_sitter_dockerfile::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-dockerfile"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Elixir,

        indent: "  ",
        files: &[],
        extensions: &["ex", "exs", "eex", "heex", "sface"],

        comment: comment_properties!("#"),

        #[cfg(feature = "lang-elixir")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_elixir::language,
            highlight: Some(tree_sitter_elixir::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["do_block"],
        }),
        #[cfg(not(feature = "lang-elixir"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Elm,

        indent: "    ",
        files: &[],
        extensions: &["elm"],

        comment: comment_properties!("#"),

        #[cfg(feature = "lang-elm")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_elm::language,
            highlight: Some(include_str!("../queries/elm/highlights.scm")),
            injection: Some(tree_sitter_elm::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-elm"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Erlang,

        indent: "    ",
        files: &[],
        extensions: &["erl", "hrl"],

        comment: comment_properties!("%"),

        #[cfg(feature = "lang-erlang")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_erlang::language,
            highlight: Some(include_str!("../queries/erlang/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-erlang"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Glimmer,

        indent: "  ",
        files: &[],
        extensions: &["hbs"],

        comment: comment_properties!("{{!", "!}}"),

        #[cfg(feature = "lang-glimmer")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_glimmer::language,
            highlight: Some(tree_sitter_glimmer::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-glimmer"))]
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

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-glsl")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_glsl::language,
            highlight: Some(tree_sitter_glsl::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-glsl"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Go,

        indent: "    ",
        files: &[],
        extensions: &["go"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-go")]
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
        #[cfg(not(feature = "lang-go"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Hare,

        indent: "        ",
        files: &[],
        extensions: &["ha"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-hare")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_hare::language,
            highlight: Some(tree_sitter_hare::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-hare"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Haskell,

        indent: "  ",
        files: &[],
        extensions: &["hs"],

        comment: comment_properties!("--"),

        #[cfg(feature = "lang-haskell")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_haskell::language,
            highlight: Some(tree_sitter_haskell::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-haskell"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Haxe,

        indent: "  ",
        files: &[],
        extensions: &["hx"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-haxe")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_haxe::language,
            highlight: Some(tree_sitter_haxe::HIGHLIGHTS_QUERY),
            injection: Some(tree_sitter_haxe::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-haxe"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Hcl,

        indent: "  ",
        files: &[],
        extensions: &["hcl", "tf"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-hcl")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_hcl::language,
            highlight: Some(tree_sitter_hcl::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-hcl"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Html,

        indent: "    ",
        files: &[],
        extensions: &["html", "htm"],

        comment: comment_properties!("<!--", "-->"),

        tree_sitter: Some(TreeSitterProperties {
            language: None,
            grammar: Some("html"),
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Java,

        indent: "    ",
        files: &[],
        extensions: &["java"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-java")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_java::language,
            highlight: Some(tree_sitter_java::HIGHLIGHT_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-java"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Javascript,

        indent: "  ",
        files: &[],
        extensions: &["js", "cjs", "mjs"],

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-javascript")]
            language: Some(tree_sitter_javascript::language),
            #[cfg(not(feature = "lang-javascript"))]
            language: None,
            grammar: Some("javascript"),
            query: Some("javascript"),
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Json,

        indent: "    ",
        files: &[],
        extensions: &["json"],

        comment: comment_properties!(),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-json")]
            language: Some(tree_sitter_json::language),
            #[cfg(not(feature = "lang-json"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Jsx,

        indent: "  ",
        files: &[],
        extensions: &["jsx"],

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-javascript")]
            language: Some(tree_sitter_javascript::language),
            #[cfg(not(feature = "lang-javascript"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Julia,

        indent: "    ",
        files: &[],
        extensions: &["julia", "jl"],

        comment: CommentProperties {
            single_line_start: Some("#"),
            single_line_end: None,

            multi_line_start: Some("#="),
            multi_line_prefix: None,
            multi_line_end: Some("=#"),
        },

        tree_sitter: Some(TreeSitterProperties {
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Kotlin,

        indent: "  ",
        files: &[],
        extensions: &["kt", "kts"],

        comment: CommentProperties {
            single_line_start: Some("//"),
            single_line_end: None,

            multi_line_start: Some("/*"),
            multi_line_prefix: None,
            multi_line_end: Some("*/"),
        },

        #[cfg(feature = "lang-kotlin")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_kotlin::language,
            highlight: Some(include_str!("../queries/kotlin/highlights.scm")),
            injection: Some(include_str!("../queries/kotlin/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-kotlin"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Latex,

        indent: "  ",
        files: &[],
        extensions: &["tex"],

        comment: comment_properties!("%"),

        #[cfg(feature = "lang-latex")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_latex::language,
            highlight: Some(include_str!("../queries/latex/highlights.scm")),
            injection: Some(include_str!("../queries/latex/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-latex"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Lua,

        indent: "  ",
        files: &[],
        extensions: &["lua"],

        comment: comment_properties!("--"),

        #[cfg(feature = "lang-lua")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_lua::language,
            highlight: Some(include_str!("../queries/lua/highlights.scm")),
            injection: None,
            sticky_headers: &[],
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        }),
        #[cfg(not(feature = "lang-lua"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Markdown,

        indent: "    ",
        files: &[],
        extensions: &["md"],

        comment: comment_properties!(),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-markdown")]
            language: Some(tree_sitter_md::language),
            #[cfg(not(feature = "lang-markdown"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::MarkdownInline,

        indent: "    ",
        // markdown inline is only used as an injection by the Markdown language
        files: &[],
        extensions: &[],

        comment: comment_properties!(),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-markdown")]
            language: Some(tree_sitter_md::inline_language),
            #[cfg(not(feature = "lang-markdown"))]
            language: None,
            grammar: Some("markdown"),
            query: Some("markdown.inline"),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Nix,

        indent: "  ",
        files: &[],
        extensions: &["nix"],

        comment: CommentProperties {
            single_line_start: Some("#"),
            single_line_end: None,

            multi_line_start: Some("/*"),
            multi_line_prefix: None,
            multi_line_end: Some("*/"),
        },

        #[cfg(feature = "lang-nix")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_nix::language,
            highlight: Some(tree_sitter_nix::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-nix"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ocaml,

        indent: "  ",
        files: &[],
        extensions: &["ml"],

        comment: CommentProperties {
            single_line_start: Some("(*"),
            single_line_end: Some("*)"),

            multi_line_start: Some("(*"),
            multi_line_prefix: Some("*"),
            multi_line_end: Some("*)"),
        },

        #[cfg(feature = "lang-ocaml")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ocaml::language_ocaml,
            highlight: Some(tree_sitter_ocaml::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-ocaml"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::OcamlInterface,

        indent: "  ",
        files: &[],
        extensions: &["mli"],

        comment: comment_properties!("(*"),

        #[cfg(feature = "lang-ocaml")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ocaml::language_ocaml_interface,
            highlight: Some(tree_sitter_ocaml::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-ocaml"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Php,

        indent: "  ",
        files: &[],
        extensions: &["php"],

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            language: None,
            grammar: None,
            query: None,
            code_lens: (
                &[
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
                ],
            ),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Prisma,

        indent: "    ",
        files: &[],
        extensions: &["prisma"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-prisma")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_prisma_io::language,
            highlight: Some(include_str!("../queries/prisma/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-prisma"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::ProtoBuf,

        indent: "  ",
        files: &[],
        extensions: &["proto"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-protobuf")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_protobuf::language,
            highlight: Some(include_str!("../queries/protobuf/highlights.scm")),
            injection: Some(include_str!("../queries/protobuf/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-protobuf"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Python,

        indent: "    ",
        files: &[],
        extensions: &["py", "pyi", "pyc", "pyd", "pyw"],

        comment: comment_properties!("#"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-python")]
            language: Some(tree_sitter_python::language),
            #[cfg(not(feature = "lang-python"))]
            language: None,
            grammar: None,
            query: None,
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
    },
    SyntaxProperties {
        id: LapceLanguage::Ql,

        indent: "  ",
        files: &[],
        extensions: &["ql"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-ql")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_ql::language,
            highlight: Some(tree_sitter_ql::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-ql"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::R,

        indent: "  ",
        files: &[],
        extensions: &["r"],

        comment: comment_properties!("#"),

        #[cfg(feature = "lang-r")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_r::language,
            highlight: Some(include_str!("../queries/r/highlights.scm")),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-r"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Ruby,

        indent: "  ",
        files: &[],
        extensions: &["rb"],

        comment: comment_properties!("#"),

        tree_sitter: Some(TreeSitterProperties {
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["module", "class", "method", "do_block"],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Rust,

        indent: "    ",
        files: &[],
        extensions: &["rs"],

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-rust")]
            language: Some(tree_sitter_rust::language),
            #[cfg(not(feature = "lang-rust"))]
            language: None,
            grammar: None,
            query: None,
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
    },
    SyntaxProperties {
        id: LapceLanguage::Scheme,

        indent: "  ",
        files: &[],
        extensions: &["scm", "ss"],

        comment: comment_properties!(";"),

        #[cfg(feature = "lang-scheme")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_scheme::language,
            highlight: Some(tree_sitter_scheme::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-scheme"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Scss,

        indent: "  ",
        files: &[],
        extensions: &["scss"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-scss")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_scss::language,
            highlight: Some(tree_sitter_scss::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-scss"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Sh,

        indent: "  ",
        files: &[],
        extensions: &["sh"],

        comment: comment_properties!("#"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-bash")]
            language: Some(tree_sitter_bash::language),
            #[cfg(not(feature = "lang-bash"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Sql,

        indent: "  ",
        files: &[],
        extensions: &["sql"],

        comment: comment_properties!("--"),

        #[cfg(feature = "lang-sql")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_sql::language,
            highlight: Some(tree_sitter_sql::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-sql"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Svelte,

        indent: "  ",
        files: &[],
        extensions: &["svelte"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-svelte")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_svelte::language,
            highlight: Some(include_str!("../queries/svelte/highlights.scm")),
            injection: Some(include_str!("../queries/svelte/injections.scm")),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-svelte"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Swift,

        indent: "  ",
        files: &[],
        extensions: &["swift"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-swift")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_swift::language,
            highlight: Some(tree_sitter_swift::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-swift"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Toml,

        indent: "  ",
        files: &[],
        extensions: &["toml"],

        comment: comment_properties!("#"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-toml")]
            language: Some(tree_sitter_toml::language),
            #[cfg(not(feature = "lang-toml"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Tsx,

        indent: "    ",
        files: &[],
        extensions: &["tsx"],

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            language: None,
            grammar: Some("typescript"),
            query: Some("typescript"),
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Typescript,

        indent: "    ",
        files: &[],
        extensions: &["ts", "cts", "mts"],

        comment: comment_properties!("//"),

        tree_sitter: Some(TreeSitterProperties {
            language: None,
            grammar: Some("typescript"),
            query: Some("typescript"),
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Vue,

        indent: "  ",
        files: &[],
        extensions: &["vue"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-vue")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_vue::language,
            highlight: Some(tree_sitter_vue::HIGHLIGHTS_QUERY),
            injection: Some(tree_sitter_vue::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-vue"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Wgsl,

        indent: "    ",
        files: &[],
        extensions: &["wgsl"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-wgsl")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_wgsl::language,
            highlight: Some(tree_sitter_wgsl::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-wgsl"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Xml,

        indent: "    ",
        files: &[],
        extensions: &["xml", "csproj"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-xml")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_xml::language,
            highlight: Some(tree_sitter_xml::HIGHLIGHTS_QUERY),
            injection: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-xml"))]
        tree_sitter: None,
    },
    SyntaxProperties {
        id: LapceLanguage::Yaml,

        indent: "  ",
        files: &[],
        extensions: &["yml", "yaml"],

        comment: comment_properties!("#"),

        tree_sitter: Some(TreeSitterProperties {
            #[cfg(feature = "lang-yaml")]
            language: Some(tree_sitter_yaml::language),
            #[cfg(not(feature = "lang-yaml"))]
            language: None,
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
    },
    SyntaxProperties {
        id: LapceLanguage::Zig,

        indent: "    ",
        files: &[],
        extensions: &["zig"],

        comment: comment_properties!("//"),

        #[cfg(feature = "lang-zig")]
        tree_sitter: Some(TreeSitterProperties {
            language: tree_sitter_zig::language,
            highlight: Some(include_str!("../queries/zig/highlights.scm")),
            injection: Some(tree_sitter_zig::INJECTIONS_QUERY),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        }),
        #[cfg(not(feature = "lang-zig"))]
        tree_sitter: None,
    },
];

impl LapceLanguage {
    const HIGHLIGHTS_QUERIES_FILE_NAME: &'static str = "highlights.scm";
    const HIGHLIGHTS_INJECTIONS_FILE_NAME: &'static str = "injections.scm";
    #[cfg(unix)]
    const SYSTEM_GRAMMARS_DIRECTORY: &'static str = "/usr/lib";
    #[cfg(unix)]
    const SYSTEM_QUERIES_DIRECTORY: &'static str = "/usr/share/tree-sitter/grammars";

    pub fn from_path(path: &Path) -> LapceLanguage {
        Self::from_path_raw(path).unwrap_or(LapceLanguage::Plaintext)
    }

    fn from_path_raw(path: &Path) -> Option<LapceLanguage> {
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
                debug!("failed parsing {name} LapceLanguage: {e}");
                None
            }
        }
    }

    pub fn languages() -> Vec<&'static str> {
        let mut langs = vec![];
        for l in LANGUAGES {
            // Get only languages with display name to hide inline grammars
            if let Some(lang) = strum::EnumMessage::get_message(&l.id) {
                langs.push(lang)
            }
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

    pub fn name(&self) -> &'static str {
        strum::EnumMessage::get_message(self)
            .unwrap_or(strum::AsStaticRef::as_static(self))
    }

    fn tree_sitter(&self) -> Option<TreeSitterProperties> {
        self.properties().tree_sitter
    }

    pub fn sticky_header_tags(&self) -> &[&'static str] {
        if let Some(ts) = self.properties().tree_sitter {
            ts.sticky_headers
        } else {
            &[]
        }
    }

    pub fn comment_token(&self) -> &'static str {
        self.properties()
            .comment
            .single_line_start
            .unwrap_or_default()
    }

    pub fn indent_unit(&self) -> &str {
        self.properties().indent
    }

    fn get_grammar(&self) -> Option<tree_sitter::Language> {
        let props = self.properties();
        let grammar_name = self.grammar_name();

        if let Some(f) = props.tree_sitter.as_ref().and_then(|p| p.language.as_ref())
        {
            return Some(f());
        }

        #[cfg(unix)]
        {
            let grammars_dir = Path::new(Self::SYSTEM_GRAMMARS_DIRECTORY);
            if grammars_dir.exists() {
                let grammars_dir = grammars_dir.join(props.id.as_ref());
                if grammars_dir.exists() {
                    if let Ok(grammar) =
                        self::load_grammar(&grammar_name, &grammars_dir)
                    {
                        return Some(grammar);
                    }
                }
            }
        };

        if let Some(grammars_dir) = Directory::grammars_directory() {
            if let Ok(grammar) = self::load_grammar(&grammar_name, &grammars_dir) {
                return Some(grammar);
            }
        };

        None
    }

    fn query_name(&self) -> String {
        self.properties()
            .tree_sitter
            .as_ref()
            .and_then(|p| p.query.or(p.grammar))
            .unwrap_or(self.properties().id.as_ref())
            .to_lowercase()
    }

    fn grammar_name(&self) -> String {
        self.properties()
            .tree_sitter
            .as_ref()
            .and_then(|p| p.grammar)
            .unwrap_or(self.properties().id.as_ref())
            .to_lowercase()
    }

    fn get_grammar_query(&self) -> (String, String) {
        let query_name = self.query_name();

        // Try reading highlights from user config dir
        if let Some(queries_dir) = Directory::queries_directory() {
            let queries_dir = queries_dir.join(&query_name);
            if queries_dir.exists() {
                let highlights_file =
                    queries_dir.join(Self::HIGHLIGHTS_QUERIES_FILE_NAME);
                if highlights_file.exists() {
                    if let Ok(s) = std::fs::read_to_string(highlights_file) {
                        return (
                            s,
                            std::fs::read_to_string(
                                queries_dir
                                    .join(Self::HIGHLIGHTS_INJECTIONS_FILE_NAME),
                            )
                            .unwrap_or_else(|_| "".to_string()),
                        );
                    }
                }
            }
        }

        #[cfg(unix)]
        {
            let queries_dir = Path::new(Self::SYSTEM_QUERIES_DIRECTORY);
            if queries_dir.join(&query_name).exists() {
                let highlights_file =
                    queries_dir.join(Self::HIGHLIGHTS_QUERIES_FILE_NAME);
                if highlights_file.exists() {
                    if let Ok(s) = std::fs::read_to_string(highlights_file) {
                        return (
                            s,
                            std::fs::read_to_string(
                                queries_dir
                                    .join(Self::HIGHLIGHTS_INJECTIONS_FILE_NAME),
                            )
                            .unwrap_or_else(|_| "".to_string()),
                        );
                    }
                }
            }
        }

        if let Some(file) =
            DEFAULT_QUERIES.get_file(format!("{query_name}/highlights.scm"))
        {
            if let Some(contents) = file.contents_utf8() {
                return (
                    contents.to_string(),
                    DEFAULT_QUERIES
                        .get_file(format!("{query_name}/injections.scm"))
                        .and_then(|f| f.contents_utf8())
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }

        ("".to_string(), "".to_string())
    }

    pub(crate) fn new_highlight_config(
        &self,
    ) -> Result<HighlightConfiguration, HighlightIssue> {
        let grammar = self.get_grammar().ok_or(HighlightIssue::NotAvailable)?;
        let (query, injection) = self.get_grammar_query();

        match HighlightConfiguration::new(grammar, &query, &injection, "") {
            Ok(x) => Ok(x),
            Err(x) => {
                let str = format!("Encountered {x:?} while trying to construct HighlightConfiguration for {}", strum::EnumMessage::get_message(self).unwrap_or(self.as_ref()));
                error!("{str}");
                Err(HighlightIssue::Error(str))
            }
        }
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        if let Some((list, ignore_list)) =
            self.tree_sitter().as_ref().map(|p| p.code_lens)
        {
            walk_tree(cursor, normal_lines, list, ignore_list);
        }
    }
}

fn load_grammar(
    grammar_name: &str,
    path: &Path,
) -> Result<tree_sitter::Language, HighlightIssue> {
    let mut library_path = path.join(format!("tree-sitter-{grammar_name}"));
    library_path.set_extension(std::env::consts::DLL_EXTENSION);

    debug!("Grammars dir: {library_path:?}");
    if !library_path.exists() {
        return Err(HighlightIssue::Error(String::from(
            "Couldn't find any grammar",
        )));
    }

    debug!("Loading grammar from user grammar dir");
    let library = match unsafe { libloading::Library::new(&library_path) } {
        Ok(v) => v,
        Err(e) => {
            return Err(HighlightIssue::Error(format!(
                "Failed to load '{}': '{e}'",
                library_path.display()
            )));
        }
    };
    let language_fn_name = format!("tree_sitter_{}", grammar_name.replace('-', "_"));
    debug!("Loading grammar with address: '{language_fn_name}'");
    let language = unsafe {
        let language_fn: libloading::Symbol<
            unsafe extern "C" fn() -> tree_sitter::Language,
        > = match library.get(language_fn_name.as_bytes()) {
            Ok(v) => v,
            Err(e) => {
                return Err(HighlightIssue::Error(format!(
                    "Failed to load '{language_fn_name}': '{e}'"
                )))
            }
        };
        language_fn()
    };
    std::mem::forget(library);

    Ok(language)
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
