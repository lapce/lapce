use floem::{
    id::Id,
    peniko::kurbo::{BezPath, Point, Size},
    style::TextColor,
    view::{View, ViewData},
    Renderer,
};

pub fn wave_box() -> WaveBox {
    let id = Id::next();
    WaveBox {
        id,
        data: ViewData::new(id),
    }
}

pub struct WaveBox {
    id: Id,
    data: ViewData,
}

impl View for WaveBox {
    fn id(&self) -> Id {
        self.id
    }

    fn view_data(&self) -> &ViewData {
        &self.data
    }

    fn view_data_mut(&mut self) -> &mut ViewData {
        &mut self.data
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        if let Some(color) = cx.get_computed_style(self.id).get(TextColor) {
            let layout = cx.get_layout(self.id).unwrap();
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
