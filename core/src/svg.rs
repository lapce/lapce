use std::{collections::HashMap, rc::Rc, str::FromStr, sync::Arc};

use druid::{
    kurbo::BezPath,
    piet::{
        self, FixedLinearGradient, GradientStop, LineCap, LineJoin, StrokeStyle,
    },
    Affine, Color, PaintCtx, Point, Rect, RenderContext, Size,
};
use include_dir::{include_dir, Dir};
use lsp_types::SymbolKind;
use usvg;

pub const ICONS_DIR: Dir = include_dir!("../icons");

pub struct Svg {
    tree: Arc<usvg::Tree>,
}

impl Svg {
    pub fn paint(&self, ctx: &mut PaintCtx, rect: Rect, color: Option<&Color>) {
        ctx.draw_svg(&*self.tree, rect, color);
    }

    pub fn to_piet(
        &self,
        offset_matrix: Affine,
        ctx: &mut PaintCtx,
        color: Option<&Color>,
    ) {
        let mut state = SvgRenderer::new(offset_matrix * self.inner_affine());
        // I actually made `SvgRenderer` able to handle a stack of `<defs>`, but I'm gonna see if
        // resvg always puts them at the top.
        let root = self.tree.root();
        for n in root.children() {
            state.render_node(&n, ctx, color);
        }
    }

    fn inner_affine(&self) -> Affine {
        let viewbox = self.viewbox();
        let size = self.size();
        // we want to move the viewbox top left to (0,0) and then scale it from viewbox size to
        // size.
        // TODO respect preserveAspectRatio
        let t = Affine::translate((viewbox.min_x(), viewbox.min_y()));
        let scale = Affine::scale_non_uniform(
            size.width / viewbox.width(),
            size.height / viewbox.height(),
        );
        scale * t
    }

    fn viewbox(&self) -> Rect {
        let root = self.tree.root();
        let rect = match *root.borrow() {
            usvg::NodeKind::Svg(svg) => {
                let r = svg.view_box.rect;
                Rect::new(r.left(), r.top(), r.right(), r.bottom())
            }
            _ => Rect::ZERO,
        };
        rect
    }

    fn size(&self) -> Size {
        let root = self.tree.root();
        let rect = match *root.borrow() {
            usvg::NodeKind::Svg(svg) => {
                let s = svg.size;
                Size::new(s.width(), s.height())
            }
            _ => Size::ZERO,
        };
        rect
    }
}

impl FromStr for Svg {
    type Err = Box<dyn std::error::Error>;

    fn from_str(svg_str: &str) -> Result<Self, Self::Err> {
        let re_opt = usvg::Options {
            keep_named_groups: false,
            ..usvg::Options::default()
        };

        match usvg::Tree::from_str(svg_str, &re_opt) {
            Ok(tree) => Ok(Svg {
                tree: Arc::new(tree),
            }),
            Err(err) => Err(err.into()),
        }
    }
}

struct SvgRenderer {
    offset_matrix: Affine,
    defs: Defs,
}

impl SvgRenderer {
    fn new(offset_matrix: Affine) -> Self {
        Self {
            offset_matrix,
            defs: Defs::new(),
        }
    }

    fn render_node(
        &mut self,
        n: &usvg::Node,
        ctx: &mut PaintCtx,
        color: Option<&Color>,
    ) {
        match *n.borrow() {
            usvg::NodeKind::Path(ref p) => self.render_path(p, ctx, color),
            usvg::NodeKind::Defs => {
                // children are defs
                for def in n.children() {
                    match &*def.borrow() {
                        usvg::NodeKind::LinearGradient(linear_gradient) => {
                            self.linear_gradient_def(linear_gradient, ctx);
                        }
                        other => (),
                    }
                }
            }
            usvg::NodeKind::Group(_) => {
                // TODO I'm not sure if we need to apply the transform, or if usvg has already
                // done it for us? I'm guessing the latter for now, but that could easily be wrong.
                for child in n.children() {
                    self.render_node(&child, ctx, color);
                }
            }
            _ => {
                // TODO: handle more of the SVG spec.
            }
        }
    }

    fn render_path(
        &self,
        p: &usvg::Path,
        ctx: &mut PaintCtx,
        color: Option<&Color>,
    ) {
        if matches!(
            p.visibility,
            usvg::Visibility::Hidden | usvg::Visibility::Collapse
        ) {
            // skip rendering
            return;
        }

        let mut path = BezPath::new();
        for segment in p.data.iter() {
            match *segment {
                usvg::PathSegment::MoveTo { x, y } => {
                    path.move_to((x, y));
                }
                usvg::PathSegment::LineTo { x, y } => {
                    path.line_to((x, y));
                }
                usvg::PathSegment::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => {
                    path.curve_to((x1, y1), (x2, y2), (x, y));
                }
                usvg::PathSegment::ClosePath => {
                    path.close_path();
                }
            }
        }

        path.apply_affine(self.offset_matrix * transform_to_affine(p.transform));

        match &p.fill {
            Some(fill) => {
                let brush =
                    self.brush_from_usvg(&fill.paint, fill.opacity, ctx, color);
                if let usvg::FillRule::EvenOdd = fill.rule {
                    ctx.fill_even_odd(path.clone(), &*brush);
                } else {
                    ctx.fill(path.clone(), &*brush);
                }
            }
            None => {}
        }

        match &p.stroke {
            Some(stroke) => {
                let brush =
                    self.brush_from_usvg(&stroke.paint, stroke.opacity, ctx, color);
                let mut stroke_style = StrokeStyle::new()
                    .line_join(match stroke.linejoin {
                        usvg::LineJoin::Miter => LineJoin::Miter {
                            limit: stroke.miterlimit.value(),
                        },
                        usvg::LineJoin::Round => LineJoin::Round,
                        usvg::LineJoin::Bevel => LineJoin::Bevel,
                    })
                    .line_cap(match stroke.linecap {
                        usvg::LineCap::Butt => LineCap::Butt,
                        usvg::LineCap::Round => LineCap::Round,
                        usvg::LineCap::Square => LineCap::Square,
                    });
                if let Some(dash_array) = &stroke.dasharray {
                    stroke_style.set_dash_pattern(dash_array.as_slice());
                    stroke_style.set_dash_offset(stroke.dashoffset as f64);
                }
                ctx.stroke_styled(
                    path,
                    &*brush,
                    stroke.width.value(),
                    &stroke_style,
                );
            }
            None => {}
        }
    }

    fn linear_gradient_def(
        &mut self,
        lg: &usvg::LinearGradient,
        ctx: &mut PaintCtx,
    ) {
        // Get start and stop of gradient and transform them to image space (TODO check we need to
        // apply offset matrix)
        let start = self.offset_matrix * Point::new(lg.x1, lg.y1);
        let end = self.offset_matrix * Point::new(lg.x2, lg.y2);
        let stops: Vec<_> = lg
            .base
            .stops
            .iter()
            .map(|stop| GradientStop {
                pos: stop.offset.value() as f32,
                color: color_from_svg(stop.color, stop.opacity),
            })
            .collect();

        // TODO error handling
        let gradient = FixedLinearGradient { start, end, stops };
        let gradient = ctx.gradient(gradient).unwrap();
        self.defs.add_def(lg.id.clone(), gradient);
    }

    fn brush_from_usvg(
        &self,
        paint: &usvg::Paint,
        opacity: usvg::Opacity,
        ctx: &mut PaintCtx,
        color: Option<&Color>,
    ) -> Rc<piet::Brush> {
        if let Some(color) = color {
            return Rc::new(ctx.solid_brush(color.clone()));
        }
        match paint {
            usvg::Paint::Color(c) => {
                // TODO I'm going to assume here that not retaining colors is OK.
                let color = color_from_svg(*c, opacity);
                Rc::new(ctx.solid_brush(color))
            }
            usvg::Paint::Link(id) => self.defs.find(id).unwrap(),
        }
    }
}

type Def = piet::Brush;

/// A map from id to <def>
struct Defs(HashMap<String, Rc<Def>>);

impl Defs {
    fn new() -> Self {
        Defs(HashMap::new())
    }

    /// Add a def.
    fn add_def(&mut self, id: String, def: Def) {
        self.0.insert(id, Rc::new(def));
    }

    /// Look for a def by id.
    fn find(&self, id: &str) -> Option<Rc<Def>> {
        self.0.get(id).cloned()
    }
}

fn transform_to_affine(t: usvg::Transform) -> Affine {
    Affine::new([t.a, t.b, t.c, t.d, t.e, t.f])
}

fn color_from_svg(c: usvg::Color, opacity: usvg::Opacity) -> Color {
    Color::rgb8(c.red, c.green, c.blue).with_alpha(opacity.value())
}

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
