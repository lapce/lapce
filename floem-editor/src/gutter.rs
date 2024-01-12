use std::rc::Rc;

use floem::{
    cosmic_text::{Attrs, AttrsList, TextLayout},
    id::Id,
    peniko::kurbo::Point,
    view::{View, ViewData},
    Renderer,
};
use lapce_core::mode::Mode;

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
