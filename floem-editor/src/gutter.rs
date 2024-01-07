use std::rc::Rc;

use floem::{
    context::PaintCx,
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    id::Id,
    peniko::kurbo::{Point, Rect, Size},
    view::{View, ViewData},
    Renderer,
};
use lapce_core::{buffer::rope_text::RopeText, mode::Mode};

use crate::{color::EditorColor, editor::Editor};

pub struct EditorGutterView {
    id: Id,
    data: ViewData,
    editor: Rc<Editor>,
    width: f64,
}

pub fn editor_gutter_view(editor: Rc<Editor>) -> EditorGutterView {
    let id = Id::next();

    EditorGutterView {
        id,
        data: ViewData::new(id),
        editor,
        width: 0.0,
    }
}

impl EditorGutterView {
    // fn paint_sticky_headers(
    //     &self,
    //     cx: &mut PaintCx,
    //     is_normal: bool,
    //     config: &LapceConfig,
    // ) {
    //     if !is_normal {
    //         return;
    //     }

    //     if !config.editor.sticky_header {
    //         return;
    //     }
    //     let sticky_header_height = self.editor.sticky_header_height;
    //     let sticky_header_height = sticky_header_height.get_untracked();
    //     if sticky_header_height == 0.0 {
    //         return;
    //     }

    //     let sticky_area_rect =
    //         Size::new(self.width + 25.0 + 30.0, sticky_header_height)
    //             .to_rect()
    //             .with_origin(Point::new(-25.0, 0.0))
    //             .inflate(25.0, 0.0);
    //     cx.fill(
    //         &sticky_area_rect,
    //         config.color(LapceColor::LAPCE_DROPDOWN_SHADOW),
    //         3.0,
    //     );
    //     cx.fill(
    //         &sticky_area_rect,
    //         config.color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
    //         0.0,
    //     );
    // }
}

impl View for EditorGutterView {
    fn id(&self) -> Id {
        self.id
    }

    fn view_data(&self) -> &ViewData {
        &self.data
    }

    fn view_data_mut(&mut self) -> &mut ViewData {
        &mut self.data
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<floem::peniko::kurbo::Rect> {
        if let Some(width) = cx.get_layout(self.id).map(|l| l.size.width as f64) {
            self.width = width;
        }
        None
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        let viewport = self.editor.viewport.get_untracked();
        let cursor = self.editor.cursor;
        let style = self.editor.style.get_untracked();

        let (offset, mode) = cursor.with_untracked(|c| (c.offset(), c.get_mode()));
        let last_line = self.editor.last_line();
        let current_line = self.editor.line_of_offset(offset);

        // TODO: don't assume font family is constant for each line
        let family = style.font_family(0);
        let attrs = Attrs::new()
            .family(&family)
            .color(style.color(EditorColor::Dim))
            .font_size(style.font_size(0) as f32);
        let attrs_list = AttrsList::new(attrs);
        let current_line_attrs_list =
            AttrsList::new(attrs.color(style.color(EditorColor::Foreground)));
        let show_relative = self.editor.modal.get_untracked()
            && self.editor.modal_relative_line_numbers.get_untracked()
            && mode != Mode::Insert;

        self.editor.screen_lines.with_untracked(|screen_lines| {
            for (line, y) in screen_lines.iter_lines_y() {
                // If it ends up outside the bounds of the file, stop trying to display line numbers
                if line > last_line {
                    break;
                }

                let line_height = f64::from(style.line_height(line));

                let text = if show_relative {
                    if line == current_line {
                        line + 1
                    } else {
                        line.abs_diff(current_line)
                    }
                } else {
                    line + 1
                }
                .to_string();

                let mut text_layout = TextLayout::new();
                if line == current_line {
                    text_layout.set_text(&text, current_line_attrs_list.clone());
                } else {
                    text_layout.set_text(&text, attrs_list.clone());
                }
                let size = text_layout.size();
                let height = size.height;

                cx.draw_text(
                    &text_layout,
                    Point::new(
                        (self.width - (size.width)).max(0.0),
                        y + (line_height - height) / 2.0 - viewport.y0,
                    ),
                );
            }
        });
    }
}
