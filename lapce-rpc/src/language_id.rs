use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Clone, Debug, Serialize, PartialEq, Eq)]
pub enum LanguageId {
    Cpp,
    ObjectiveC,
    Bat,
    Clojure,
    C,
    CoffeeScript,
    Csharp,
    Css,
    Dlang,
    Diff,
    Dart,
    Dockerfile,
    Elm,
    Elixir,
    Erlang,
    Fsharp,
    Git,
    Go,
    Groovy,
    Handlebars,
    Html,
    Ini,
    Java,
    JavaScript,
    JavaScriptReact,
    Json,
    Julia,
    Kotlin,
    Less,
    Lua,
    Makefile,
    Markdown,
    ObjectiveCpp,
    Perl,
    Perl6,
    Php,
    Proto,
    Powershell,
    Python,
    R,
    Ruby,
    Rust,
    Scss,
    Scala,
    ShellScript,
    Sql,
    Swift,
    Svelte,
    Toml,
    TypeScript,
    TypeScriptReact,
    Tex,
    Vb,
    Xml,
    Xsl,
    Yaml,
    Zig,
    Vue,

    ///
    Unknown,
}

impl LanguageId {
    pub fn from_path(path: &Path) -> LanguageId {
        LanguageId::impl_from_path(path).unwrap_or(LanguageId::Unknown)
    }

    fn impl_from_path(path: &Path) -> Option<LanguageId> {
        // recommended language_id values
        // https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocumentItem
        Some(match path.extension() {
            Some(ext) => {
                match ext.to_str()? {
                    "C" | "H" => LanguageId::Cpp,
                    "M" => LanguageId::ObjectiveC,
                    // stop case-sensitive matching
                    ext => {
                        return Some(LanguageId::from_str(
                            ext.to_lowercase().as_str(),
                        ))
                    }
                }
            }
            // Handle paths without extension
            #[allow(clippy::match_single_binding)]
            None => match path.file_name()?.to_str()? {
                // case-insensitive matching
                filename => match filename.to_lowercase().as_str() {
                    "dockerfile" => LanguageId::Dockerfile,
                    "makefile" | "gnumakefile" => LanguageId::Makefile,
                    _ => return None,
                },
            },
        })
    }

    pub fn from_str(str: &str) -> LanguageId {
        match str {
            "bat" => LanguageId::Bat,
            "clj" | "cljs" | "cljc" | "edn" => LanguageId::Clojure,
            "coffee" => LanguageId::CoffeeScript,
            "c" | "h" => LanguageId::C,
            "cpp" | "hpp" | "cxx" | "hxx" | "c++" | "h++" | "cc" | "hh" => {
                LanguageId::Cpp
            }
            "cs" | "csx" => LanguageId::Csharp,
            "css" => LanguageId::Css,
            "d" | "di" | "dlang" => LanguageId::Dlang,
            "diff" | "patch" => LanguageId::Diff,
            "dart" => LanguageId::Dart,
            "dockerfile" => LanguageId::Dockerfile,
            "elm" => LanguageId::Elm,
            "ex" | "exs" => LanguageId::Elixir,
            "erl" | "hrl" => LanguageId::Elixir,
            "fs" | "fsi" | "fsx" | "fsscript" => LanguageId::Fsharp,
            "git-commit" | "git-rebase" => LanguageId::Git,
            "go" => LanguageId::Go,
            "groovy" | "gvy" | "gy" | "gsh" => LanguageId::Groovy,
            "hbs" => LanguageId::Handlebars,
            "htm" | "html" | "xhtml" => LanguageId::Html,
            "ini" => LanguageId::Ini,
            "java" | "class" => LanguageId::Java,
            "js" => LanguageId::JavaScript,
            "jsx" => LanguageId::JavaScriptReact,
            "json" => LanguageId::Json,
            "jl" => LanguageId::Julia,
            "kt" | "kts" => LanguageId::Kotlin,
            "less" => LanguageId::Less,
            "lua" => LanguageId::Lua,
            "makefile" | "gnumakefile" => LanguageId::Makefile,
            "md" | "markdown" => LanguageId::Markdown,
            "m" => LanguageId::ObjectiveC,
            "mm" => LanguageId::ObjectiveCpp,
            "plx" | "pl" | "pm" | "xs" | "t" | "pod" | "cgi" => LanguageId::Perl,
            "p6" | "pm6" | "pod6" | "t6" | "raku" | "rakumod" | "rakudoc"
            | "rakutest" => LanguageId::Perl6,
            "php" | "phtml" | "pht" | "phps" => LanguageId::Php,
            "proto" => LanguageId::Proto,
            "ps1" | "ps1xml" | "psc1" | "psm1" | "psd1" | "pssc" | "psrc" => {
                LanguageId::Powershell
            }
            "py" | "pyi" | "pyc" | "pyd" | "pyw" => LanguageId::Python,
            "r" => LanguageId::R,
            "rb" => LanguageId::Ruby,
            "rs" => LanguageId::Rust,
            "scss" | "sass" => LanguageId::Scss,
            "sc" | "scala" => LanguageId::Scala,
            "sh" | "bash" | "zsh" => LanguageId::ShellScript,
            "sql" => LanguageId::Sql,
            "swift" => LanguageId::Swift,
            "svelte" => LanguageId::Svelte,
            "toml" => LanguageId::Toml,
            "ts" => LanguageId::TypeScript,
            "tsx" => LanguageId::TypeScriptReact,
            "tex" => LanguageId::Tex,
            "vb" => LanguageId::Vb,
            "xml" => LanguageId::Xml,
            "xsl" => LanguageId::Xsl,
            "yml" | "yaml" => LanguageId::Yaml,
            "zig" => LanguageId::Zig,
            "vue" => LanguageId::Vue,
            _ => LanguageId::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LanguageId::Cpp => todo!(),
            LanguageId::ObjectiveC => todo!(),
            LanguageId::Bat => todo!(),
            LanguageId::Clojure => todo!(),
            LanguageId::C => todo!(),
            LanguageId::CoffeeScript => todo!(),
            LanguageId::Csharp => todo!(),
            LanguageId::Css => todo!(),
            LanguageId::Dlang => todo!(),
            LanguageId::Diff => todo!(),
            LanguageId::Dart => todo!(),
            LanguageId::Dockerfile => todo!(),
            LanguageId::Elm => todo!(),
            LanguageId::Elixir => todo!(),
            LanguageId::Erlang => todo!(),
            LanguageId::Fsharp => todo!(),
            LanguageId::Git => todo!(),
            LanguageId::Go => todo!(),
            LanguageId::Groovy => todo!(),
            LanguageId::Handlebars => todo!(),
            LanguageId::Html => todo!(),
            LanguageId::Ini => todo!(),
            LanguageId::Java => todo!(),
            LanguageId::JavaScript => todo!(),
            LanguageId::JavaScriptReact => todo!(),
            LanguageId::Json => todo!(),
            LanguageId::Julia => todo!(),
            LanguageId::Kotlin => todo!(),
            LanguageId::Less => todo!(),
            LanguageId::Lua => todo!(),
            LanguageId::Makefile => todo!(),
            LanguageId::Markdown => todo!(),
            LanguageId::ObjectiveCpp => todo!(),
            LanguageId::Perl => todo!(),
            LanguageId::Perl6 => todo!(),
            LanguageId::Php => todo!(),
            LanguageId::Proto => todo!(),
            LanguageId::Powershell => todo!(),
            LanguageId::Python => todo!(),
            LanguageId::R => todo!(),
            LanguageId::Ruby => todo!(),
            LanguageId::Rust => todo!(),
            LanguageId::Scss => todo!(),
            LanguageId::Scala => todo!(),
            LanguageId::ShellScript => todo!(),
            LanguageId::Sql => todo!(),
            LanguageId::Swift => todo!(),
            LanguageId::Svelte => todo!(),
            LanguageId::Toml => todo!(),
            LanguageId::TypeScript => todo!(),
            LanguageId::TypeScriptReact => todo!(),
            LanguageId::Tex => todo!(),
            LanguageId::Vb => todo!(),
            LanguageId::Xml => todo!(),
            LanguageId::Xsl => todo!(),
            LanguageId::Yaml => todo!(),
            LanguageId::Zig => todo!(),
            LanguageId::Vue => todo!(),
            LanguageId::Unknown => todo!(),
        }
    }
}

#[test]
fn test_language_id_size() {
    assert_eq!(std::mem::size_of::<LanguageId>(), 1);
    assert_eq!(std::mem::size_of::<String>(), 24);
    assert_eq!(std::mem::size_of::<Option<String>>(), 24);
}
