use floem::{
    Renderer, View, ViewId,
    peniko::kurbo::{BezPath, Point, Size},
    style::TextColor,
};

pub fn wave_box() -> WaveBox {
    WaveBox { id: ViewId::new() }
}

pub struct WaveBox {
    id: ViewId,
}

impl View for WaveBox {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        if let Some(color) = self.id.get_combined_style().get(TextColor) {
            let layout = self.id.get_layout().unwrap_or_default();
            let size = layout.size;
            let size = Size::new(size.width as f64, size.height as f64);
            let radius = 4.0;

            let origin = Point::new(0.0, size.height);
            let mut path = BezPath::new();
            path.move_to(origin);

            let mut x = 0.0;
            let mut direction = -1.0;
            while x < size.width {
                let point = origin + (x, 0.0);
                let p1 = point + (radius * 1.5, -radius * direction);
                let p2 = point + (radius * 3.0, 0.0);
                path.quad_to(p1, p2);
                x += radius * 3.0;
                direction *= -1.0;
            }
            {
                let origin = Point::new(0.0, 0.0);
                path.line_to(origin + (x, 0.0));
                direction *= -1.0;
                while x >= 0.0 {
                    x -= radius * 3.0;
                    let point = origin + (x, 0.0);
                    let p1 = point + (radius * 1.5, -radius * direction);
                    let p2 = point;
                    path.quad_to(p1, p2);
                    direction *= -1.0;
                }
            }
            path.line_to(origin);

            cx.fill(&path, color, 0.0);
        }
    }
}
