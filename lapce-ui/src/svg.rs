use std::{collections::HashMap, ffi::OsStr, path::Path, str::FromStr};

use druid::{piet::Svg, Color};
use include_dir::{include_dir, Dir};
use lapce_data::config::{Config, LapceIcons, LOGO};
use lazy_static::lazy_static;
use lsp_types::{CompletionItemKind, SymbolKind};

const UI_ICONS_DIR: Dir = include_dir!("../icons");

lazy_static! {
    static ref SVG_STORE: SvgStore = SvgStore::new();
}

struct SvgStore {
    svgs: HashMap<&'static str, Option<Svg>>,
}

fn insert_svgs(svgs: &'static Dir, map: &mut HashMap<&'static str, Option<Svg>>) {
    for file in svgs.files() {
        let file_path = file.path();
        if file_path.extension().is_some() {
            if let Some(file_name) = file_path.file_stem().and_then(OsStr::to_str) {
                let svg = file
                    .contents_utf8()
                    .and_then(|content| Svg::from_str(content).ok());

                map.insert(file_name, svg);
            }
        }
    }
}

impl SvgStore {
    fn new() -> Self {
        let mut svgs = HashMap::new();
        svgs.insert("lapce_logo", Svg::from_str(LOGO).ok());
        insert_svgs(&UI_ICONS_DIR, &mut svgs);

        Self { svgs }
    }

    fn get_svg(&self, name: &'static str) -> Option<Svg> {
        self.svgs.get(name).and_then(Clone::clone)
    }
}

pub fn logo_svg() -> Svg {
    get_svg(LapceIcons::LAPCE_LOGO).unwrap()
}

pub fn get_svg(name: &'static str) -> Option<Svg> {
    SVG_STORE.get_svg(name)
}

#[allow(unused)]
pub fn file_svg(path: &Path) -> (Svg, Option<&Color>) {
    let icon_name: Option<&str>;
    let icon_color: Option<&Color>;
    (icon_name, icon_color) = match path.extension().and_then(OsStr::to_str) {
        Some(extension) => {
            // Fallback
            const TYPES: &[(&[&str], &str, Option<&Color>)] = &[
                (&["c"], "c", None),
                (&["h"], "c", None),
                (&["cxx", "cc", "c++", "cpp"], "C++", None),
                (&["hxx", "hh", "h++", "hpp"], "C++", None),
                (&["go"], "go", None),
                (&["json"], "json", None),
                (&["markdown", "md"], "markdown", None),
                (&["rs"], "rust", None),
                // (&["toml"], "toml", None),
                (&["yaml"], "yaml", None),
                (&["py"], "python", None),
                (&["lua"], "lua", None),
                (&["html", "htm"], "html", None),
                (&["zip"], "zip", None),
                (&["js"], "js", None),
                (&["ts"], "ts", None),
                (&["css"], "css", None),
                (
                    &[
                        "svg", "png", "jpeg", "webp", "gif", "tiff", "bmp", "jpg",
                        "jfif", "mp4", "avi", "",
                    ],
                    LapceIcons::FILE_TYPE_MEDIA,
                    None,
                ),
            ];

            let (mut icon, mut color) = (None, None);

            for (exts, file_type, col) in TYPES {
                for ext in exts.iter() {
                    if extension.eq_ignore_ascii_case(ext) {
                        (icon, color) = (Some(*file_type), *col)
                    }
                }
            }

            (icon, color)
        }
        None => match path.file_name().and_then(OsStr::to_str) {
            Some(file_name) => {
                const FILES: &[(&[&str], &str, Option<&Color>)] = &[
                    // (&["LICENSE", "LICENCE"], "license", None),
                    // (&["COPYRIGHT"], "license", None),
                    // (&["NOTICE"], "license", None),
                ];

                let (mut icon, mut color) = (None, None);

                for (filenames, file_type, col) in FILES {
                    for filename in filenames.iter() {
                        if file_name.to_lowercase().starts_with(filename) {
                            (icon, color) = (Some(*file_type), *col)
                        }
                    }
                }

                (icon, color)
            }
            None => (None, None),
        },
    };

    match icon_name {
        Some(svg) => (get_svg(svg).unwrap(), Some(&Color::WHITE)),
        None => (get_svg(LapceIcons::FILE).unwrap(), Some(&Color::WHITE)),
    }
}

pub fn symbol_svg(kind: &SymbolKind) -> Option<Svg> {
    let kind_str = match *kind {
        SymbolKind::ARRAY => LapceIcons::SYMBOL_KIND_ARRAY,
        SymbolKind::BOOLEAN => LapceIcons::SYMBOL_KIND_BOOLEAN,
        SymbolKind::CLASS => LapceIcons::SYMBOL_KIND_CLASS,
        SymbolKind::CONSTANT => LapceIcons::SYMBOL_KIND_CONSTANT,
        SymbolKind::ENUM_MEMBER => LapceIcons::SYMBOL_KIND_ENUM_MEMBER,
        SymbolKind::ENUM => LapceIcons::SYMBOL_KIND_ENUM,
        SymbolKind::EVENT => LapceIcons::SYMBOL_KIND_EVENT,
        SymbolKind::FIELD => LapceIcons::SYMBOL_KIND_FIELD,
        SymbolKind::FILE => LapceIcons::SYMBOL_KIND_FILE,
        SymbolKind::INTERFACE => LapceIcons::SYMBOL_KIND_INTERFACE,
        SymbolKind::KEY => LapceIcons::SYMBOL_KIND_KEY,
        SymbolKind::FUNCTION => LapceIcons::SYMBOL_KIND_FUNCTION,
        SymbolKind::METHOD => LapceIcons::SYMBOL_KIND_METHOD,
        SymbolKind::OBJECT => LapceIcons::SYMBOL_KIND_OBJECT,
        SymbolKind::NAMESPACE => LapceIcons::SYMBOL_KIND_NAMESPACE,
        SymbolKind::NUMBER => LapceIcons::SYMBOL_KIND_NUMBER,
        SymbolKind::OPERATOR => LapceIcons::SYMBOL_KIND_OPERATOR,
        SymbolKind::TYPE_PARAMETER => LapceIcons::SYMBOL_KIND_TYPE_PARAMETER,
        SymbolKind::PROPERTY => LapceIcons::SYMBOL_KIND_PROPERTY,
        SymbolKind::STRING => LapceIcons::SYMBOL_KIND_STRING,
        SymbolKind::STRUCT => LapceIcons::SYMBOL_KIND_STRUCT,
        SymbolKind::VARIABLE => LapceIcons::SYMBOL_KIND_VARIABLE,
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
        CompletionItemKind::METHOD => LapceIcons::COMPLETION_ITEM_KIND_METHOD,
        CompletionItemKind::FUNCTION => LapceIcons::COMPLETION_ITEM_KIND_FUNCTION,
        CompletionItemKind::ENUM => LapceIcons::COMPLETION_ITEM_KIND_ENUM,
        CompletionItemKind::ENUM_MEMBER => {
            LapceIcons::COMPLETION_ITEM_KIND_ENUM_MEMBER
        }
        CompletionItemKind::CLASS => LapceIcons::COMPLETION_ITEM_KIND_CLASS,
        CompletionItemKind::VARIABLE => LapceIcons::SYMBOL_KIND_VARIABLE,
        CompletionItemKind::STRUCT => LapceIcons::SYMBOL_KIND_STRUCT,
        CompletionItemKind::KEYWORD => LapceIcons::COMPLETION_ITEM_KIND_KEYWORD,
        CompletionItemKind::CONSTANT => LapceIcons::COMPLETION_ITEM_KIND_CONSTANT,
        CompletionItemKind::PROPERTY => LapceIcons::COMPLETION_ITEM_KIND_PROPERTY,
        CompletionItemKind::FIELD => LapceIcons::COMPLETION_ITEM_KIND_FIELD,
        CompletionItemKind::INTERFACE => LapceIcons::COMPLETION_ITEM_KIND_INTERFACE,
        CompletionItemKind::SNIPPET => LapceIcons::COMPLETION_ITEM_KIND_SNIPPET,
        CompletionItemKind::MODULE => LapceIcons::COMPLETION_ITEM_KIND_MODULE,
        _ => LapceIcons::COMPLETION_ITEM_KIND_STRING,
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
