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
    
    // catch if path.to_str() is Some without calling it twice
    let path_str = path.to_str();
    if path_str == None {
        panic!("Missing path");
    }
    let file_name = path_str.unwrap(); // unwrap path_str safely now

    let file_type = if file_name == "LICENSE" {
        "file_type_license.svg"
    } else if file_name.to_lowercase().contains("makefile") {
        "file_type_make.svg"
    } else if file_name.to_lowercase().contains("git") {
        "git-icon.svg"
    } else if file_name.to_lowercase().contains("cargo") {
        "file_type_rust.svg"
    } else if file_name.contains("CMakeLists") {
        "file_type_cmake.svg"
    } else {
        path.extension()
            .and_then(OsStr::to_str)
            .and_then(|extension| {
                const TYPES: &[(&[&str], &str)] = &[
                    (&["c"], "file_type_c.svg"),
                    (&["h"], "file_type_h.svg"),
                    (&["cxx", "cc", "c++", "cpp"], "file_type_cpp.svg"),
                    (&["hxx", "hh", "h++", "hpp"], "file_type_hpp.svg)"),
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
                    (&["svg"], "file_type_svg.svg"),
                    (&["cmake"], "file_type_cmake.svg"),
                    (&["make"], "file_type_make.svg"),
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
    
    let svg_file = get_svg(file_type);
    if svg_file.is_none() {
        panic!("Missing svg icon '{}'", file_type);
    }
    
    return svg_file.unwrap();
}

pub fn symbol_svg(kind: &SymbolKind) -> Option<Svg> {
    let kind_str = match *kind {
        SymbolKind::ARRAY => "symbol-array.svg",
        SymbolKind::BOOLEAN => "symbol-boolean.svg",
        SymbolKind::CLASS => "symbol-class.svg",
        SymbolKind::CONSTANT => "symbol-constant.svg",
        SymbolKind::ENUM_MEMBER => "symbol-enum-member.svg",
        SymbolKind::ENUM => "symbol-enum.svg",
        SymbolKind::EVENT => "symbol-event.svg",
        SymbolKind::FIELD => "symbol-field.svg",
        SymbolKind::FILE => "symbol-file.svg",
        SymbolKind::INTERFACE => "symbol-interface.svg",
        SymbolKind::KEY => "symbol-key.svg",
        SymbolKind::FUNCTION => "symbol-method.svg",
        SymbolKind::METHOD => "symbol-method.svg",
        SymbolKind::OBJECT => "symbol-namespace.svg",
        SymbolKind::NAMESPACE => "symbol-namespace.svg",
        SymbolKind::NUMBER => "symbol-numeric.svg",
        SymbolKind::OPERATOR => "symbol-operator.svg",
        SymbolKind::TYPE_PARAMETER => "symbol-parameter.svg",
        SymbolKind::PROPERTY => "symbol-property.svg",
        SymbolKind::STRING => "symbol-string.svg",
        SymbolKind::STRUCT => "symbol-structure.svg",
        SymbolKind::VARIABLE => "symbol-variable.svg",
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
        CompletionItemKind::METHOD => "symbol-method.svg",
        CompletionItemKind::FUNCTION => "symbol-method.svg",
        CompletionItemKind::ENUM => "symbol-enum.svg",
        CompletionItemKind::ENUM_MEMBER => "symbol-enum-member.svg",
        CompletionItemKind::CLASS => "symbol-class.svg",
        CompletionItemKind::VARIABLE => "symbol-variable.svg",
        CompletionItemKind::STRUCT => "symbol-structure.svg",
        CompletionItemKind::KEYWORD => "symbol-keyword.svg",
        CompletionItemKind::CONSTANT => "symbol-constant.svg",
        CompletionItemKind::PROPERTY => "symbol-property.svg",
        CompletionItemKind::FIELD => "symbol-field.svg",
        CompletionItemKind::INTERFACE => "symbol-interface.svg",
        CompletionItemKind::SNIPPET => "symbol-snippet.svg",
        CompletionItemKind::MODULE => "symbol-namespace.svg",
        _ => "symbol-string.svg",
    };
    let theme_str = match kind {
        CompletionItemKind::METHOD => "method",
        CompletionItemKind::FUNCTION => "method",
        CompletionItemKind::ENUM => "enum",
        CompletionItemKind::ENUM_MEMBER => "enum-member",
        CompletionItemKind::CLASS => "class",
        CompletionItemKind::VARIABLE => "field",
        CompletionItemKind::STRUCT => "structure",
        CompletionItemKind::KEYWORD => "keyword",
        CompletionItemKind::CONSTANT => "constant",
        CompletionItemKind::PROPERTY => "property",
        CompletionItemKind::FIELD => "field",
        CompletionItemKind::INTERFACE => "interface",
        CompletionItemKind::SNIPPET => "snippet",
        CompletionItemKind::MODULE => "builtinType",
        _ => "string",
    };

    Some((
        get_svg(kind_str)?,
        config.get_style_color(theme_str).cloned(),
    ))
}
