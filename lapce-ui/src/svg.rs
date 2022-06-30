use std::{collections::HashMap, ffi::OsStr, path::Path, str::FromStr};

use druid::{piet::Svg, Color};
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use lsp_types::{CompletionItemKind, SymbolKind};

use lapce_data::config::{Config, LOGO};

const ICONS_DIR: Dir = include_dir!("../icons");

lazy_static! {
    static ref SVG_STORE: SvgStore = SvgStore::new();
}

struct SvgStore {
    svgs: HashMap<&'static str, Option<Svg>>,
}

impl SvgStore {
    fn new() -> Self {
        let mut svgs = HashMap::new();
        svgs.insert("lapce_logo", Svg::from_str(LOGO).ok());

        for file in ICONS_DIR.files() {
            if let Some(file_name) = file.path().file_name().and_then(OsStr::to_str)
            {
                let svg =
                    file.contents_utf8().and_then(|str| Svg::from_str(str).ok());

                svgs.insert(file_name, svg);
            }
        }

        Self { svgs }
    }

    fn get_svg(&self, name: &'static str) -> Option<Svg> {
        self.svgs.get(name).and_then(Clone::clone)
    }
}

pub fn logo_svg() -> Svg {
    get_svg("lapce_logo").unwrap()
}

pub fn get_svg(name: &'static str) -> Option<Svg> {
    SVG_STORE.get_svg(name)
}

pub fn file_svg(path: &Path) -> Svg {
    let file_type = if path.file_name().and_then(OsStr::to_str) == Some("LICENSE") {
        "file_type_license.svg"
    } else {
        path.extension()
            .and_then(OsStr::to_str)
            .and_then(|extension| {
                const TYPES: &[(&[&str], &str)] = &[
                    (&["c"], "file_type_c.svg"),
                    (&["cxx", "cc", "c++", "cpp"], "file_type_cpp.svg"),
                    (&["go"], "file_type_go.svg"),
                    (&["json"], "file_type_json.svg"),
                    (&["markdown", "md"], "file_type_markdown.svg"),
                    (&["rs"], "file_type_rust.svg"),
                    (&["toml"], "file_type_toml.svg"),
                    (&["yaml"], "file_type_yaml.svg"),
                    (&["py"], "file_type_python.svg"),
                    (&["lua"], "file_type_lua.svg"),
                    (&["html", "htm"], "file_type_html.svg"),
                    (&["zip"], "file_type_zip.svg"),
                    (&["js"], "file_type_js.svg"),
                    (&["ts"], "file_type_ts.svg"),
                    (&["css"], "file_type_css.svg"),
                ];

                for (exts, file_type) in TYPES {
                    for ext in exts.iter() {
                        if extension.eq_ignore_ascii_case(ext) {
                            return Some(*file_type);
                        }
                    }
                }

                None
            })
            .unwrap_or("default_file.svg")
    };

    get_svg(file_type).unwrap()
}

pub fn symbol_svg(kind: &SymbolKind) -> Option<Svg> {
    let kind_str = match kind {
        SymbolKind::Array => "symbol-array.svg",
        SymbolKind::Boolean => "symbol-boolean.svg",
        SymbolKind::Class => "symbol-class.svg",
        SymbolKind::Constant => "symbol-constant.svg",
        SymbolKind::EnumMember => "symbol-enum-member.svg",
        SymbolKind::Enum => "symbol-enum.svg",
        SymbolKind::Event => "symbol-event.svg",
        SymbolKind::Field => "symbol-field.svg",
        SymbolKind::File => "symbol-file.svg",
        SymbolKind::Interface => "symbol-interface.svg",
        SymbolKind::Key => "symbol-key.svg",
        SymbolKind::Function => "symbol-method.svg",
        SymbolKind::Method => "symbol-method.svg",
        SymbolKind::Object => "symbol-namespace.svg",
        SymbolKind::Namespace => "symbol-namespace.svg",
        SymbolKind::Number => "symbol-numeric.svg",
        SymbolKind::Operator => "symbol-operator.svg",
        SymbolKind::TypeParameter => "symbol-parameter.svg",
        SymbolKind::Property => "symbol-property.svg",
        SymbolKind::String => "symbol-string.svg",
        SymbolKind::Struct => "symbol-structure.svg",
        SymbolKind::Variable => "symbol-variable.svg",
        _ => return None,
    };

    get_svg(kind_str)
}

pub fn completion_svg(
    kind: Option<CompletionItemKind>,
    config: &Config,
) -> Option<(Svg, Option<Color>)> {
    let kind = kind?;
    let kind_str = match kind {
        CompletionItemKind::Method => "symbol-method.svg",
        CompletionItemKind::Function => "symbol-method.svg",
        CompletionItemKind::Enum => "symbol-enum.svg",
        CompletionItemKind::EnumMember => "symbol-enum-member.svg",
        CompletionItemKind::Class => "symbol-class.svg",
        CompletionItemKind::Variable => "symbol-variable.svg",
        CompletionItemKind::Struct => "symbol-structure.svg",
        CompletionItemKind::Keyword => "symbol-keyword.svg",
        CompletionItemKind::Constant => "symbol-constant.svg",
        CompletionItemKind::Property => "symbol-property.svg",
        CompletionItemKind::Field => "symbol-field.svg",
        CompletionItemKind::Interface => "symbol-interface.svg",
        CompletionItemKind::Snippet => "symbol-snippet.svg",
        CompletionItemKind::Module => "symbol-namespace.svg",
        _ => "symbol-string.svg",
    };
    let theme_str = match kind {
        CompletionItemKind::Method => "method",
        CompletionItemKind::Function => "method",
        CompletionItemKind::Enum => "enum",
        CompletionItemKind::EnumMember => "enum-member",
        CompletionItemKind::Class => "class",
        CompletionItemKind::Variable => "field",
        CompletionItemKind::Struct => "structure",
        CompletionItemKind::Keyword => "keyword",
        CompletionItemKind::Constant => "constant",
        CompletionItemKind::Property => "property",
        CompletionItemKind::Field => "field",
        CompletionItemKind::Interface => "interface",
        CompletionItemKind::Snippet => "snippet",
        CompletionItemKind::Module => "builtinType",
        _ => "string",
    };

    Some((
        get_svg(kind_str)?,
        config.get_style_color(theme_str).cloned(),
    ))
}
