use std::{
    collections::HashMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use druid::{piet::Svg, Color};
use include_dir::{include_dir, Dir};
use lapce_data::config::{LapceConfig, LapceIcons, LapceTheme, LOGO};
use lsp_types::{CompletionItemKind, SymbolKind};
use once_cell::sync::Lazy;

const CODICONS_ICONS_DIR: Dir = include_dir!("../icons/codicons");
const LAPCE_ICONS_DIR: Dir = include_dir!("../icons/lapce");

static SVG_STORE: Lazy<SvgStore> = Lazy::new(SvgStore::new);

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
        insert_svgs(&CODICONS_ICONS_DIR, &mut svgs);
        insert_svgs(&LAPCE_ICONS_DIR, &mut svgs);

        Self { svgs }
    }

    fn get_svg(&self, name: &str) -> Option<Svg> {
        self.svgs.get(name).and_then(Clone::clone)
    }
}

pub fn logo_svg() -> Svg {
    read_svg((Some("lapce_logo".to_string()), None)).unwrap()
}

pub fn read_svg(icon: (Option<String>, Option<PathBuf>)) -> Option<Svg> {
    if let Some(path) = icon.1 {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(svg) = Svg::from_str(&content) {
                return Some(svg);
            }
        }
    }

    if let Some(icon) = icon.0.clone() {
        return SVG_STORE.get_svg(icon.as_str());
    }

    SVG_STORE.get_svg("blank")
}

pub fn get_svg(icon: &str, config: &LapceConfig) -> Option<Svg> {
    if icon.is_empty() {
        None
    } else {
        read_svg(config.resolve_ui_icon(icon))
    }
}

// TODO: colours?
pub fn file_svg<'a>(
    file: &Path,
    config: &'a LapceConfig,
) -> (Svg, Option<&'a Color>) {
    if let Some(path) = config.resolve_file_icon(file) {
        if path.exists() {
            (
                read_svg((None, Some(path))).unwrap(),
                Some(config.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)),
            )
        } else {
            (
                get_svg(LapceIcons::FILE, config).unwrap(),
                Some(config.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)),
            )
        }
    } else {
        (
            get_svg(LapceIcons::FILE, config).unwrap(),
            Some(config.get_color_unchecked(LapceTheme::LAPCE_ICON_ACTIVE)),
        )
    }
}

pub fn symbol_svg(kind: &SymbolKind, config: &LapceConfig) -> Option<Svg> {
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

    get_svg(kind_str, config)
}

pub fn completion_svg(
    kind: Option<CompletionItemKind>,
    config: &LapceConfig,
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
        get_svg(kind_str, config)?,
        config.get_style_color(theme_str).cloned(),
    ))
}
