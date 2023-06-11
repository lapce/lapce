use floem::{
    id::Id,
    peniko::kurbo::{BezPath, Point, Size},
    view::{ChangeFlags, View},
    Renderer, ViewContext,
};

pub fn wave_line() -> WaveLine {
    let cx = ViewContext::get_current();
    let id = cx.new_id();
    WaveLine { id }
}

pub struct WaveLine {
    id: Id,
}

impl View for WaveLine {
    fn id(&self) -> Id {
        self.id
    }

    fn child(&mut self, _id: floem::id::Id) -> Option<&mut dyn View> {
        None
    }

    fn children(&mut self) -> Vec<&mut dyn View> {
        Vec::new()
    }

    fn update(
        &mut self,
        _cx: &mut floem::context::UpdateCx,
        _state: Box<dyn std::any::Any>,
    ) -> ChangeFlags {
        ChangeFlags::empty()
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::Node {
        cx.layout_node(self.id, false, |_cx| Vec::new())
    }

    fn event(
        &mut self,
        _cx: &mut floem::context::EventCx,
        _id_path: Option<&[floem::id::Id]>,
        _event: floem::event::Event,
    ) -> bool {
        false
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        if let Some(color) = cx.get_computed_style(self.id).color {
            let layout = cx.get_layout(self.id).unwrap();
            let size = layout.size;
            let size = Size::new(size.width as f64, size.height as f64);
            let radius = 2.0;

            let origin = Point::new(0.0, size.height + radius);
            let mut path = BezPath::new();
            path.move_to(origin);

            let mut x = 0.0;
            let mut direction = -1.0;
            while x < size.width {
                let point = origin + (x, 0.0);
                let p1 = point + (radius, -radius * direction);
                let p2 = point + (radius * 2.0, 0.0);
                path.quad_to(p1, p2);
                x += radius * 2.0;
                direction *= -1.0;
            }

            cx.stroke(&path, color, 1.0);
        }
    }
}
