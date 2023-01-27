use std::{collections::HashSet, path::Path, str::FromStr};

use strum_macros::{Display, EnumString};
use tree_sitter::TreeCursor;

use crate::syntax::highlight::{HighlightConfiguration, HighlightIssue};

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
//            injection: Some(tree_sitter_foo::INJECTION_QUERY), // or None if there is no injections
//            comment: "//",
//            indent: "    ",
//            code_lens: (&[/* ... */], &[/* ... */]),
//            extensions: &["foo"],
//        },
//    ];
//
// 5. In `syntax/highlight.rs`, add `Foo: "lang-foo",` to the list in the
//    `declare_language_highlights` macro.
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
#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, Display, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum LapceLanguage {
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
    #[cfg(feature = "lang-hcl")]
    Hcl,
    #[cfg(feature = "lang-html")]
    Html,
    #[cfg(feature = "lang-java")]
    Java,
    #[cfg(feature = "lang-javascript")]
    Javascript,
    #[cfg(feature = "lang-json")]
    Json,
    #[cfg(feature = "lang-javascript")]
    Jsx,
    #[cfg(feature = "lang-julia")]
    Julia,
    #[cfg(feature = "lang-kotlin")]
    Kotlin,
    #[cfg(feature = "lang-latex")]
    Latex,
    #[cfg(feature = "lang-lua")]
    Lua,
    #[cfg(feature = "lang-markdown")]
    Markdown,
    // TODO: Hide this when it is shown to the user!
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
    #[cfg(feature = "lang-scss")]
    Scss,
    #[cfg(feature = "lang-sql")]
    Sql,
    #[cfg(feature = "lang-svelte")]
    Svelte,
    #[cfg(feature = "lang-swift")]
    Swift,
    #[cfg(feature = "lang-toml")]
    Toml,
    #[cfg(feature = "lang-typescript")]
    Tsx,
    #[cfg(feature = "lang-typescript")]
    Typescript,
    #[cfg(feature = "lang-vue")]
    Vue,
    #[cfg(feature = "lang-wgsl")]
    Wgsl,
    #[cfg(feature = "lang-xml")]
    Xml,
    #[cfg(feature = "lang-yaml")]
    Yaml,
    #[cfg(feature = "lang-zig")]
    Zig,
}

// NOTE: Elements in the array must be in the same order as the enum variants of
// `LapceLanguage` as they will be accessed using the enum variants as indices.
const LANGUAGES: &[SyntaxProperties] = &[
    #[cfg(feature = "lang-bash")]
    SyntaxProperties {
        id: LapceLanguage::Bash,
        language: tree_sitter_bash::language,
        highlight: tree_sitter_bash::HIGHLIGHT_QUERY,
        injection: None,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["sh", "bash"],
    },
    #[cfg(feature = "lang-c")]
    SyntaxProperties {
        id: LapceLanguage::C,
        language: tree_sitter_c::language,
        highlight: include_str!("../queries/c/highlights.scm"),
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["function_definition", "struct_specifier"],
        extensions: &["c", "h"],
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
    #[cfg(feature = "lang-cmake")]
    SyntaxProperties {
        id: LapceLanguage::Cmake,
        language: tree_sitter_cmake::language,
        highlight: include_str!("../queries/cmake/highlights.scm"),
        injection: Some(include_str!("../queries/cmake/injections.scm")),
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["function_definition"],
        extensions: &["cmake"],
    },
    #[cfg(feature = "lang-cpp")]
    SyntaxProperties {
        id: LapceLanguage::Cpp,
        language: tree_sitter_cpp::language,
        highlight: include_str!("../queries/cpp/highlights.scm"),
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[
            "function_definition",
            "class_specifier",
            "struct_specifier",
        ],
        extensions: &["cpp", "cxx", "cc", "c++", "hpp", "hxx", "hh", "h++"],
    },
    #[cfg(feature = "lang-csharp")]
    SyntaxProperties {
        id: LapceLanguage::Csharp,
        language: tree_sitter_c_sharp::language,
        highlight: tree_sitter_c_sharp::HIGHLIGHT_QUERY,
        injection: None,
        comment: "//",
        indent: "    ",
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
        extensions: &["cs", "csx"],
    },
    #[cfg(feature = "lang-css")]
    SyntaxProperties {
        id: LapceLanguage::Css,
        language: tree_sitter_css::language,
        highlight: include_str!("../queries/css/highlights.scm"),
        injection: None,
        comment: "/*",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["css"],
    },
    #[cfg(feature = "lang-d")]
    SyntaxProperties {
        id: LapceLanguage::D,
        language: tree_sitter_d::language,
        highlight: tree_sitter_d::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["d", "di", "dlang"],
    },
    #[cfg(feature = "lang-dart")]
    SyntaxProperties {
        id: LapceLanguage::Dart,
        language: tree_sitter_dart::language,
        highlight: tree_sitter_dart::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "  ",
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
        extensions: &["dart"],
    },
    #[cfg(feature = "lang-dockerfile")]
    SyntaxProperties {
        id: LapceLanguage::Dockerfile,
        language: tree_sitter_dockerfile::language,
        highlight: tree_sitter_dockerfile::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["containerfile", "dockerfile"],
    },
    #[cfg(feature = "lang-elixir")]
    SyntaxProperties {
        id: LapceLanguage::Elixir,
        language: tree_sitter_elixir::language,
        highlight: tree_sitter_elixir::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["do_block"],
        extensions: &["ex", "exs", "eex", "heex", "sface"],
    },
    #[cfg(feature = "lang-elm")]
    SyntaxProperties {
        id: LapceLanguage::Elm,
        language: tree_sitter_elm::language,
        highlight: include_str!("../queries/elm/highlights.scm"),
        injection: Some(tree_sitter_elm::INJECTIONS_QUERY),
        comment: "#",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["elm"],
    },
    #[cfg(feature = "lang-erlang")]
    SyntaxProperties {
        id: LapceLanguage::Erlang,
        language: tree_sitter_erlang::language,
        highlight: include_str!("../queries/erlang/highlights.scm"),
        injection: None,
        comment: "%",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["erl", "hrl"],
    },
    #[cfg(feature = "lang-glimmer")]
    SyntaxProperties {
        id: LapceLanguage::Glimmer,
        language: tree_sitter_glimmer::language,
        highlight: tree_sitter_glimmer::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "{{!",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["hbs"],
    },
    #[cfg(feature = "lang-glsl")]
    SyntaxProperties {
        id: LapceLanguage::Glsl,
        language: tree_sitter_glsl::language,
        highlight: tree_sitter_glsl::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &[
            "glsl", "cs", "vs", "gs", "fs", "csh", "vsh", "gsh", "fsh", "cshader",
            "vshader", "gshader", "fshader", "comp", "vert", "geom", "frag", "tesc",
            "tese", "mesh", "task", "rgen", "rint", "rahit", "rchit", "rmiss",
            "rcall",
        ],
    },
    #[cfg(feature = "lang-go")]
    SyntaxProperties {
        id: LapceLanguage::Go,
        language: tree_sitter_go::language,
        highlight: tree_sitter_go::HIGHLIGHT_QUERY,
        injection: None,
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
        sticky_headers: &[],
        extensions: &["go"],
    },
    #[cfg(feature = "lang-hare")]
    SyntaxProperties {
        id: LapceLanguage::Hare,
        language: tree_sitter_hare::language,
        highlight: tree_sitter_hare::HIGHLIGHT_QUERY,
        injection: None,
        comment: "//",
        indent: "        ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["ha"],
    },
    #[cfg(feature = "lang-haskell")]
    SyntaxProperties {
        id: LapceLanguage::Haskell,
        language: tree_sitter_haskell::language,
        highlight: tree_sitter_haskell::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "--",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["hs"],
    },
    #[cfg(feature = "lang-haxe")]
    SyntaxProperties {
        id: LapceLanguage::Haxe,
        language: tree_sitter_haxe::language,
        highlight: tree_sitter_haxe::HIGHLIGHTS_QUERY,
        injection: Some(tree_sitter_haxe::INJECTIONS_QUERY),
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["hx"],
    },
    #[cfg(feature = "lang-hcl")]
    SyntaxProperties {
        id: LapceLanguage::Hcl,
        language: tree_sitter_hcl::language,
        highlight: tree_sitter_hcl::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["hcl", "tf"],
    },
    #[cfg(feature = "lang-html")]
    SyntaxProperties {
        id: LapceLanguage::Html,
        language: tree_sitter_html::language,
        highlight: tree_sitter_html::HIGHLIGHT_QUERY,
        injection: Some(tree_sitter_html::INJECTION_QUERY),
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["html", "htm"],
    },
    #[cfg(feature = "lang-java")]
    SyntaxProperties {
        id: LapceLanguage::Java,
        language: tree_sitter_java::language,
        highlight: tree_sitter_java::HIGHLIGHT_QUERY,
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["java"],
    },
    #[cfg(feature = "lang-javascript")]
    SyntaxProperties {
        id: LapceLanguage::Javascript,
        language: tree_sitter_javascript::language,
        highlight: include_str!("../queries/javascript/highlights.scm"),
        injection: Some(tree_sitter_javascript::INJECTION_QUERY),
        comment: "//",
        indent: "  ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],
        extensions: &["js", "cjs", "mjs"],
    },
    #[cfg(feature = "lang-json")]
    SyntaxProperties {
        id: LapceLanguage::Json,
        language: tree_sitter_json::language,
        highlight: tree_sitter_json::HIGHLIGHT_QUERY,
        injection: None,
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["pair"],
        extensions: &["json"],
    },
    #[cfg(feature = "lang-javascript")]
    SyntaxProperties {
        id: LapceLanguage::Jsx,
        language: tree_sitter_javascript::language,
        highlight: include_str!("../queries/jsx/highlights.scm"),
        // TODO: Does jsx use the javascript injection query too?
        injection: Some(tree_sitter_javascript::INJECTION_QUERY),
        comment: "//",
        indent: "  ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],
        extensions: &["jsx"],
    },
    #[cfg(feature = "lang-julia")]
    SyntaxProperties {
        id: LapceLanguage::Julia,
        language: tree_sitter_julia::language,
        highlight: include_str!("../queries/julia/highlights.scm"),
        injection: Some(include_str!("../queries/julia/injections.scm")),
        comment: "#",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[
            "function_definition",
            "module_definition",
            "macro_definition",
            "struct_definition",
        ],
        extensions: &["julia", "jl"],
    },
    #[cfg(feature = "lang-kotlin")]
    SyntaxProperties {
        id: LapceLanguage::Kotlin,
        language: tree_sitter_kotlin::language,
        highlight: include_str!("../queries/kotlin/highlights.scm"),
        injection: Some(include_str!("../queries/kotlin/injections.scm")),
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["kt", "kts"],
    },
    #[cfg(feature = "lang-latex")]
    SyntaxProperties {
        id: LapceLanguage::Latex,
        language: tree_sitter_latex::language,
        highlight: include_str!("../queries/latex/highlights.scm"),
        injection: Some(include_str!("../queries/latex/injections.scm")),
        comment: "%",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["tex"],
    },
    #[cfg(feature = "lang-lua")]
    SyntaxProperties {
        id: LapceLanguage::Lua,
        language: tree_sitter_lua::language,
        highlight: include_str!("../queries/lua/highlights.scm"),
        injection: None,
        comment: "--",
        indent: "  ",
        sticky_headers: &[],
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        extensions: &["lua"],
    },
    #[cfg(feature = "lang-markdown")]
    SyntaxProperties {
        id: LapceLanguage::Markdown,
        language: tree_sitter_md::language,
        highlight: include_str!("../queries/markdown/highlights.scm"),
        injection: Some(include_str!("../queries/markdown/injections.scm")),
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["md"],
    },
    #[cfg(feature = "lang-markdown")]
    SyntaxProperties {
        id: LapceLanguage::MarkdownInline,
        language: tree_sitter_md::inline_language,
        highlight: include_str!("../queries/markdown.inline/highlights.scm"),
        injection: Some(include_str!("../queries/markdown.inline/injections.scm")),
        comment: "",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        // markdown inline is only used as an injection by the Markdown language
        extensions: &[],
    },
    #[cfg(feature = "lang-nix")]
    SyntaxProperties {
        id: LapceLanguage::Nix,
        language: tree_sitter_nix::language,
        highlight: tree_sitter_nix::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["nix"],
    },
    #[cfg(feature = "lang-ocaml")]
    SyntaxProperties {
        id: LapceLanguage::Ocaml,
        language: tree_sitter_ocaml::language_ocaml,
        highlight: tree_sitter_ocaml::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "(*",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["ml"],
    },
    #[cfg(feature = "lang-ocaml")]
    SyntaxProperties {
        id: LapceLanguage::Ocaml,
        language: tree_sitter_ocaml::language_ocaml_interface,
        highlight: tree_sitter_ocaml::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "(*",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["mli"],
    },
    #[cfg(feature = "lang-php")]
    SyntaxProperties {
        id: LapceLanguage::Php,
        language: tree_sitter_php::language,
        highlight: tree_sitter_php::HIGHLIGHT_QUERY,
        injection: Some(tree_sitter_php::INJECTIONS_QUERY),
        comment: "//",
        indent: "  ",
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
        sticky_headers: &[
            "class_declaration",
            "trait_declaration",
            "interface_declaration",
            "method_declaration",
            "function_declaration",
        ],
        extensions: &["php"],
    },
    #[cfg(feature = "lang-prisma")]
    SyntaxProperties {
        id: LapceLanguage::Prisma,
        language: tree_sitter_prisma_io::language,
        highlight: include_str!("../queries/prisma/highlights.scm"),
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["prisma"],
    },
    #[cfg(feature = "lang-protobuf")]
    SyntaxProperties {
        id: LapceLanguage::ProtoBuf,
        language: tree_sitter_protobuf::language,
        highlight: include_str!("../queries/protobuf/highlights.scm"),
        injection: Some(include_str!("../queries/protobuf/injections.scm")),
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["proto"],
    },
    #[cfg(feature = "lang-python")]
    SyntaxProperties {
        id: LapceLanguage::Python,
        language: tree_sitter_python::language,
        highlight: tree_sitter_python::HIGHLIGHT_QUERY,
        injection: None,
        comment: "#",
        indent: "    ",
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
        extensions: &["py", "pyi", "pyc", "pyd", "pyw"],
    },
    #[cfg(feature = "lang-ql")]
    SyntaxProperties {
        id: LapceLanguage::Ql,
        language: tree_sitter_ql::language,
        highlight: tree_sitter_ql::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["ql"],
    },
    #[cfg(feature = "lang-r")]
    SyntaxProperties {
        id: LapceLanguage::R,
        language: tree_sitter_r::language,
        highlight: include_str!("../queries/r/highlights.scm"),
        injection: None,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["r"],
    },
    #[cfg(feature = "lang-ruby")]
    SyntaxProperties {
        id: LapceLanguage::Ruby,
        language: tree_sitter_ruby::language,
        highlight: tree_sitter_ruby::HIGHLIGHT_QUERY,
        injection: None,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &["module", "class", "method", "do_block"],
        extensions: &["rb"],
    },
    #[cfg(feature = "lang-rust")]
    SyntaxProperties {
        id: LapceLanguage::Rust,
        language: tree_sitter_rust::language,
        highlight: tree_sitter_rust::HIGHLIGHT_QUERY,
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (
            &["source_file", "impl_item", "trait_item", "declaration_list"],
            &["source_file", "use_declaration", "line_comment"],
        ),
        sticky_headers: &["struct_item", "enum_item", "function_item", "impl_item"],
        extensions: &["rs"],
    },
    #[cfg(feature = "lang-scheme")]
    SyntaxProperties {
        id: LapceLanguage::Scheme,
        language: tree_sitter_scheme::language,
        highlight: tree_sitter_scheme::HIGHLIGHTS_QUERY,
        injection: None,
        comment: ";",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["scm", "ss"],
    },
    #[cfg(feature = "lang-scss")]
    SyntaxProperties {
        id: LapceLanguage::Scss,
        language: tree_sitter_scss::language,
        highlight: tree_sitter_scss::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["scss"],
    },
    #[cfg(feature = "lang-sql")]
    SyntaxProperties {
        id: LapceLanguage::Sql,
        language: tree_sitter_sql::language,
        highlight: tree_sitter_sql::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "--",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["sql"],
    },
    #[cfg(feature = "lang-svelte")]
    SyntaxProperties {
        id: LapceLanguage::Svelte,
        language: tree_sitter_svelte::language,
        highlight: include_str!("../queries/svelte/highlights.scm"),
        injection: Some(include_str!("../queries/svelte/injections.scm")),
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["svelte"],
    },
    #[cfg(feature = "lang-swift")]
    SyntaxProperties {
        id: LapceLanguage::Swift,
        language: tree_sitter_swift::language,
        highlight: tree_sitter_swift::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["swift"],
    },
    #[cfg(feature = "lang-toml")]
    SyntaxProperties {
        id: LapceLanguage::Toml,
        language: tree_sitter_toml::language,
        highlight: tree_sitter_toml::HIGHLIGHT_QUERY,
        injection: None,
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["toml"],
    },
    #[cfg(feature = "lang-typescript")]
    SyntaxProperties {
        id: LapceLanguage::Tsx,
        language: tree_sitter_typescript::language_tsx,
        highlight: include_str!("../queries/typescript/highlights.scm"),
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],
        extensions: &["tsx"],
    },
    #[cfg(feature = "lang-typescript")]
    SyntaxProperties {
        id: LapceLanguage::Typescript,
        language: tree_sitter_typescript::language_typescript,
        highlight: include_str!("../queries/typescript/highlights.scm"),
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (&["source_file", "program"], &["source_file"]),
        sticky_headers: &[],
        extensions: &["ts", "cts", "mts"],
    },
    #[cfg(feature = "lang-vue")]
    SyntaxProperties {
        id: LapceLanguage::Vue,
        language: tree_sitter_vue::language,
        highlight: tree_sitter_vue::HIGHLIGHTS_QUERY,
        injection: Some(tree_sitter_vue::INJECTIONS_QUERY),
        comment: "//",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["vue"],
    },
    #[cfg(feature = "lang-wgsl")]
    SyntaxProperties {
        id: LapceLanguage::Wgsl,
        language: tree_sitter_wgsl::language,
        highlight: tree_sitter_wgsl::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["wgsl"],
    },
    #[cfg(feature = "lang-xml")]
    SyntaxProperties {
        id: LapceLanguage::Xml,
        language: tree_sitter_xml::language,
        highlight: tree_sitter_xml::HIGHLIGHTS_QUERY,
        injection: None,
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["xml"],
    },
    #[cfg(feature = "lang-yaml")]
    SyntaxProperties {
        id: LapceLanguage::Yaml,
        language: tree_sitter_yaml::language,
        highlight: tree_sitter_yaml::HIGHLIGHTS_QUERY,
        injection: Some(tree_sitter_yaml::INJECTIONS_QUERY),
        comment: "#",
        indent: "  ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["yml", "yaml"],
    },
    #[cfg(feature = "lang-zig")]
    SyntaxProperties {
        id: LapceLanguage::Zig,
        language: tree_sitter_zig::language,
        highlight: include_str!("../queries/zig/highlights.scm"),
        injection: Some(tree_sitter_zig::INJECTIONS_QUERY),
        comment: "//",
        indent: "    ",
        code_lens: (DEFAULT_CODE_LENS_LIST, DEFAULT_CODE_LENS_IGNORE_LIST),
        sticky_headers: &[],
        extensions: &["zig"],
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

    pub fn sticky_header_tags(&self) -> &[&'static str] {
        self.properties().sticky_headers
    }

    pub fn comment_token(&self) -> &str {
        self.properties().comment
    }

    pub fn indent_unit(&self) -> &str {
        self.properties().indent
    }

    pub(crate) fn new_highlight_config(
        &self,
    ) -> Result<HighlightConfiguration, HighlightIssue> {
        let props = self.properties();
        let language = (props.language)();
        let query = props.highlight;
        let injection = props.injection;
        match HighlightConfiguration::new(
            language,
            query,
            injection.unwrap_or(""),
            "",
        ) {
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
