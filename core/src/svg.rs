use std::{collections::HashMap, rc::Rc, str::FromStr, sync::Arc};

use druid::{
    kurbo::BezPath,
    piet::{
        self, FixedLinearGradient, GradientStop, LineCap, LineJoin, StrokeStyle, Svg,
    },
    Affine, Color, PaintCtx, Point, Rect, RenderContext, Size,
};
use include_dir::{include_dir, Dir};
use lsp_types::SymbolKind;
use usvg;

pub const ICONS_DIR: Dir = include_dir!("../icons");

pub fn get_svg(name: &str) -> Option<Svg> {
    Svg::from_str(ICONS_DIR.get_file(name)?.contents_utf8()?).ok()
}

pub fn file_svg_new(exten: &str) -> Option<Svg> {
    let file_type = match exten {
        "rs" => "rust",
        "md" => "markdown",
        "cc" => "cpp",
        s => s,
    };
    get_svg(&format!("file_type_{}.svg", file_type))
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
