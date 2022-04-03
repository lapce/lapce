use std::{collections::HashMap, path::Path, str::FromStr, sync::Arc};

use druid::{piet::Svg, Color};
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use lsp_types::{CompletionItemKind, SymbolKind};
use parking_lot::Mutex;

use lapce_data::config::{Config, LOGO};

pub const ICONS_DIR: Dir = include_dir!("../icons");
lazy_static! {
    static ref SVG_STORE: SvgStore = SvgStore::new();
}

struct SvgStore {
    svgs: Arc<Mutex<HashMap<String, Option<Svg>>>>,
}

impl SvgStore {
    fn new() -> Self {
        Self {
            svgs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_svg(&self, name: &str) -> Option<Svg> {
        let mut svgs = self.svgs.lock();
        if !svgs.contains_key(name) {
            let svg = Svg::from_str(ICONS_DIR.get_file(name)?.contents_utf8()?).ok();
            svgs.insert(name.to_string(), svg);
        }
        svgs.get(name).cloned().unwrap()
    }
}

pub fn logo_svg() -> Svg {
    let name = "lapce_logo";
    let mut svgs = SVG_STORE.svgs.lock();
    if !svgs.contains_key(name) {
        let svg = Svg::from_str(LOGO).ok();
        svgs.insert(name.to_string(), svg);
    }
    svgs.get(name).cloned().unwrap().unwrap()
}

pub fn get_svg(name: &str) -> Option<Svg> {
    SVG_STORE.get_svg(name)
}

pub fn file_svg_new(path: &Path) -> Svg {
    let file_type = match path.file_name().and_then(|f| f.to_str()).unwrap_or("") {
        "LICENSE" => "license",
        _ => match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
            "rs" => "rust",
            "md" => "markdown",
            "cc" => "cpp",
            s => s,
        },
    };
    get_svg(&format!("file_type_{}.svg", file_type))
        .unwrap_or_else(|| get_svg("default_file.svg").unwrap())
}

pub fn symbol_svg_new(kind: &SymbolKind) -> Option<Svg> {
    let kind_str = match kind {
        SymbolKind::Array => "array",
        SymbolKind::Boolean => "boolean",
        SymbolKind::Class => "class",
        SymbolKind::Constant => "constant",
        SymbolKind::EnumMember => "enum-member",
        SymbolKind::Enum => "enum",
        SymbolKind::Event => "event",
        SymbolKind::Field => "field",
        SymbolKind::File => "file",
        SymbolKind::Interface => "interface",
        SymbolKind::Key => "key",
        SymbolKind::Function => "method",
        SymbolKind::Method => "method",
        SymbolKind::Object => "namespace",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Number => "numeric",
        SymbolKind::Operator => "operator",
        SymbolKind::TypeParameter => "parameter",
        SymbolKind::Property => "property",
        SymbolKind::String => "string",
        SymbolKind::Struct => "structure",
        SymbolKind::Variable => "variable",
        _ => return None,
    };

    get_svg(&format!("symbol-{}.svg", kind_str))
}

pub fn completion_svg(
    kind: Option<CompletionItemKind>,
    config: &Config,
) -> Option<(Svg, Option<Color>)> {
    let kind = kind?;
    let kind_str = match kind {
        CompletionItemKind::Method => "method",
        CompletionItemKind::Function => "method",
        CompletionItemKind::Enum => "enum",
        CompletionItemKind::EnumMember => "enum-member",
        CompletionItemKind::Class => "class",
        CompletionItemKind::Variable => "variable",
        CompletionItemKind::Struct => "structure",
        CompletionItemKind::Keyword => "keyword",
        CompletionItemKind::Constant => "constant",
        CompletionItemKind::Property => "property",
        CompletionItemKind::Field => "field",
        CompletionItemKind::Interface => "interface",
        CompletionItemKind::Snippet => "snippet",
        CompletionItemKind::Module => "namespace",
        _ => "string",
    };
    let theme_str = match kind_str {
        "namespace" => "builtinType",
        "variable" => "field",
        _ => kind_str,
    };

    Some((
        get_svg(&format!("symbol-{}.svg", kind_str))?,
        config.get_style_color(theme_str).cloned(),
    ))
}
