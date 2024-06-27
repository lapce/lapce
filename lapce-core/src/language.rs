use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::Write,
    path::Path,
    str::FromStr,
};

use lapce_rpc::style::{LineStyle, Style};
use once_cell::sync::Lazy;
use regex::Regex;
use strum_macros::{AsRefStr, Display, EnumMessage, EnumString, IntoStaticStr};
use tracing::{debug, error};
use tree_sitter::{Point, TreeCursor};

use crate::{
    directory::Directory,
    syntax::highlight::{HighlightConfiguration, HighlightIssue},
};

pub static RUNTIME_LANGUAGES: Lazy<Vec<SyntaxProperties>> = Lazy::new(Vec::new);

#[allow(dead_code)]
const DEFAULT_CODE_LENS_LIST: &[&str] = &["source_file"];
#[allow(dead_code)]
const DEFAULT_CODE_LENS_IGNORE_LIST: &[&str] = &["source_file"];

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
    tree_sitter: TreeSitterProperties,
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
struct TreeSitterProperties {
    /// the grammar name that's in the grammars folder
    grammar: Option<&'static str>,
    /// the query folder name
    query: Option<&'static str>,
    /// Lists of tree-sitter node types that control how code lenses are built.
    /// The first is a list of nodes that should be traversed and included in
    /// the lens, along with their children. The second is a list of nodes that
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

impl TreeSitterProperties {
    const DEFAULT: Self = Self {
        grammar: None,
        query: None,
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
    };
}

// impl Default for TreeSitterProperties {
//     fn default() -> Self {
//         Self {
//             grammar: None,
//             query: None,
//             code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
//             sticky_headers: &[],
//         }
//     }
// }

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
    IntoStaticStr,
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
    #[strum(message = "Tsx")]
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
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Bash,
        indent: "  ",
        files: &[],
        extensions: &["bash"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::C,
        indent: "    ",
        files: &[],
        extensions: &["c", "h"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["function_definition", "struct_specifier"],
        },
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
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Cmake,
        indent: "  ",
        files: &["cmakelists"],
        extensions: &["cmake"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["function_definition"],
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Cpp,
        indent: "    ",
        files: &[],
        extensions: &["cpp", "cxx", "cc", "c++", "hpp", "hxx", "hh", "h++"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[
                "function_definition",
                "class_specifier",
                "struct_specifier",
            ],
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Csharp,
        indent: "  ",
        files: &[],
        extensions: &["cs", "csx"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
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
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Css,
        indent: "  ",
        files: &[],
        extensions: &["css"],
        comment: comment_properties!("/*", "*/"),
        tree_sitter: TreeSitterProperties::DEFAULT,
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
        tree_sitter: TreeSitterProperties::DEFAULT,
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
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
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
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Dockerfile,
        indent: "  ",
        files: &["dockerfile", "containerfile"],
        extensions: &["containerfile", "dockerfile"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Elixir,
        indent: "  ",
        files: &[],
        extensions: &["ex", "exs", "eex", "heex", "sface"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["do_block"],
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Elm,
        indent: "    ",
        files: &[],
        extensions: &["elm"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Erlang,
        indent: "    ",
        files: &[],
        extensions: &["erl", "hrl"],
        comment: comment_properties!("%"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Glimmer,
        indent: "  ",
        files: &[],
        extensions: &["hbs"],
        comment: comment_properties!("{{!", "!}}"),
        tree_sitter: TreeSitterProperties::DEFAULT,
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
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Go,
        indent: "    ",
        files: &[],
        extensions: &["go"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
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
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Hare,
        indent: "        ",
        files: &[],
        extensions: &["ha"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Haskell,
        indent: "  ",
        files: &[],
        extensions: &["hs"],
        comment: comment_properties!("--"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Haxe,
        indent: "  ",
        files: &[],
        extensions: &["hx"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Hcl,
        indent: "  ",
        files: &[],
        extensions: &["hcl", "tf"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Html,
        indent: "    ",
        files: &[],
        extensions: &["html", "htm"],
        comment: comment_properties!("<!--", "-->"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Java,
        indent: "    ",
        files: &[],
        extensions: &["java"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Javascript,
        indent: "  ",
        files: &[],
        extensions: &["js", "cjs", "mjs"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Json,
        indent: "    ",
        files: &[],
        extensions: &["json"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Jsx,
        indent: "  ",
        files: &[],
        extensions: &["jsx"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        },
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
        tree_sitter: TreeSitterProperties::DEFAULT,
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
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Latex,
        indent: "  ",
        files: &[],
        extensions: &["tex"],
        comment: comment_properties!("%"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Lua,
        indent: "  ",
        files: &[],
        extensions: &["lua"],
        comment: comment_properties!("--"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Markdown,
        indent: "    ",
        files: &[],
        extensions: &["md"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::MarkdownInline,
        indent: "    ",
        // markdown inline is only used as an injection by the Markdown language
        files: &[],
        extensions: &[],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties {
            grammar: Some("markdown"),
            query: Some("markdown.inline"),
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &[],
        },
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
        tree_sitter: TreeSitterProperties::DEFAULT,
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
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::OcamlInterface,
        indent: "  ",
        files: &[],
        extensions: &["mli"],
        comment: comment_properties!("(*"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Php,
        indent: "  ",
        files: &[],
        extensions: &["php"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
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
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Prisma,
        indent: "    ",
        files: &[],
        extensions: &["prisma"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::ProtoBuf,
        indent: "  ",
        files: &[],
        extensions: &["proto"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Python,
        indent: "    ",
        files: &[],
        extensions: &["py", "pyi", "pyc", "pyd", "pyw"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties {
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
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Ql,
        indent: "  ",
        files: &[],
        extensions: &["ql"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::R,
        indent: "  ",
        files: &[],
        extensions: &["r"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Ruby,
        indent: "  ",
        files: &[],
        extensions: &["rb"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            query: None,
            code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
            sticky_headers: &["module", "class", "method", "do_block"],
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Rust,
        indent: "    ",
        files: &[],
        extensions: &["rs"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
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
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Scheme,
        indent: "  ",
        files: &[],
        extensions: &["scm", "ss"],
        comment: comment_properties!(";"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Scss,
        indent: "  ",
        files: &[],
        extensions: &["scss"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Sh,
        indent: "  ",
        files: &[],
        extensions: &["sh"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Sql,
        indent: "  ",
        files: &[],
        extensions: &["sql"],
        comment: comment_properties!("--"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Svelte,
        indent: "  ",
        files: &[],
        extensions: &["svelte"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Swift,
        indent: "  ",
        files: &[],
        extensions: &["swift"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Toml,
        indent: "  ",
        files: &[],
        extensions: &["toml"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Tsx,
        indent: "    ",
        files: &[],
        extensions: &["tsx"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: Some("tsx"),
            query: Some("tsx"),
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Typescript,
        indent: "    ",
        files: &[],
        extensions: &["ts", "cts", "mts"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: Some("typescript"),
            query: Some("typescript"),
            code_lens: (&["source_file", "program"], &["source_file"]),
            sticky_headers: &[],
        },
    },
    SyntaxProperties {
        id: LapceLanguage::Vue,
        indent: "  ",
        files: &[],
        extensions: &["vue"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Wgsl,
        indent: "    ",
        files: &[],
        extensions: &["wgsl"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Xml,
        indent: "    ",
        files: &[],
        extensions: &["xml", "csproj"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Yaml,
        indent: "  ",
        files: &[],
        extensions: &["yml", "yaml"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: LapceLanguage::Zig,
        indent: "    ",
        files: &[],
        extensions: &["zig"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
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

    pub fn from_path_raw(path: &Path) -> Option<LapceLanguage> {
        let filename = path
            .file_stem()
            .and_then(|s| s.to_str().map(|s| s.to_lowercase()));
        let extension = path
            .extension()
            .and_then(|s| s.to_str().map(|s| s.to_lowercase()));
        // NOTE: This is a linear search.  It is assumed that this function
        // isn't called in any tight loop.
        for properties in LANGUAGES {
            if properties
                .files
                .iter()
                .any(|f| Some(*f) == filename.as_deref())
            {
                return Some(properties.id);
            }
            if properties
                .extensions
                .iter()
                .any(|e| Some(*e) == extension.as_deref())
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
        strum::EnumMessage::get_message(self).unwrap_or(self.into())
    }

    pub fn sticky_header_tags(&self) -> &[&'static str] {
        self.properties().tree_sitter.sticky_headers
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
        let grammar_name = self.grammar_name();

        #[cfg(unix)]
        {
            let props = self.properties();
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
            .query
            .unwrap_or(self.properties().id.as_ref())
            .to_lowercase()
    }

    fn grammar_name(&self) -> String {
        self.properties()
            .tree_sitter
            .grammar
            .unwrap_or(self.properties().id.as_ref())
            .to_lowercase()
    }

    fn get_grammar_query(&self) -> (String, String) {
        let query_name = self.query_name();

        // Try reading highlights from user config dir
        if let Some(queries_dir) = Directory::queries_directory() {
            return (
                read_grammar_query(
                    &queries_dir,
                    &query_name,
                    Self::HIGHLIGHTS_QUERIES_FILE_NAME,
                ),
                read_grammar_query(
                    &queries_dir,
                    &query_name,
                    Self::HIGHLIGHTS_INJECTIONS_FILE_NAME,
                ),
            );
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
        let (list, ignore_list) = self.properties().tree_sitter.code_lens;
        walk_tree(cursor, normal_lines, list, ignore_list);
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
                if let Some(e) = library.close().err() {
                    error!("Failed to drop loaded library: {e}");
                };
                return Err(HighlightIssue::Error(format!(
                    "Failed to load '{language_fn_name}': '{e}'"
                )));
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

fn add_bracket_pos(
    bracket_pos: &mut HashMap<usize, Vec<LineStyle>>,
    start_pos: Point,
    color: String,
) {
    let line_style = LineStyle {
        start: start_pos.column,
        end: start_pos.column + 1,
        style: Style {
            fg_color: Some(color),
        },
    };
    match bracket_pos.entry(start_pos.row) {
        Entry::Vacant(v) => _ = v.insert(vec![line_style]),
        Entry::Occupied(mut o) => o.get_mut().push(line_style),
    }
}

pub(crate) fn walk_tree_bracket_ast(
    cursor: &mut TreeCursor,
    level: &mut usize,
    counter: &mut usize,
    bracket_pos: &mut HashMap<usize, Vec<LineStyle>>,
    palette: &Vec<String>,
) {
    if cursor.node().kind().ends_with('(')
        || cursor.node().kind().ends_with('{')
        || cursor.node().kind().ends_with('[')
    {
        let row = cursor.node().end_position().row;
        let col = cursor.node().end_position().column - 1;
        let start_pos = Point::new(row, col);
        add_bracket_pos(
            bracket_pos,
            start_pos,
            palette.get(*level % palette.len()).unwrap().clone(),
        );
        *level += 1;
    } else if cursor.node().kind().ends_with(')')
        || cursor.node().kind().ends_with('}')
        || cursor.node().kind().ends_with(']')
    {
        let (new_level, overflow) = (*level).overflowing_sub(1);
        let row = cursor.node().end_position().row;
        let col = cursor.node().end_position().column - 1;
        let start_pos = Point::new(row, col);
        if overflow {
            add_bracket_pos(bracket_pos, start_pos, "bracket.unpaired".to_string());
        } else {
            *level = new_level;
            add_bracket_pos(
                bracket_pos,
                start_pos,
                palette.get(*level % palette.len()).unwrap().clone(),
            );
        }
    }
    *counter += 1;
    if cursor.goto_first_child() {
        loop {
            walk_tree_bracket_ast(cursor, level, counter, bracket_pos, palette);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn read_grammar_query(queries_dir: &Path, name: &str, kind: &str) -> String {
    static INHERITS_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r";+\s*inherits\s*:?\s*([a-z_,()-]+)\s*").unwrap());

    let file = queries_dir.join(name).join(kind);
    let query = std::fs::read_to_string(file).unwrap_or_default();

    INHERITS_REGEX
        .replace_all(&query, |captures: &regex::Captures| {
            captures[1]
                .split(',')
                .fold(String::new(), |mut output, name| {
                    write!(
                        output,
                        "\n{}\n",
                        read_grammar_query(queries_dir, name, kind)
                    )
                    .unwrap();
                    output
                })
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::LapceLanguage;

    #[test]
    fn test_lanaguage_from_path() {
        let l = LapceLanguage::from_path(&PathBuf::new().join("test.rs"));
        assert_eq!(l, LapceLanguage::Rust);
    }
}
