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

use crate::{
    config::{color::LapceColor, LapceConfig},
    doc::DocumentExt,
};

use super::{view::changes_colors_screen, view_data::EditorViewData, EditorData};

pub struct EditorGutterView {
    id: Id,
    data: ViewData,
    editor: Rc<EditorData>,
    width: f64,
}

pub fn editor_gutter_view(editor: Rc<EditorData>) -> EditorGutterView {
    let id = Id::next();

    EditorGutterView {
        id,
        data: ViewData::new(id),
        editor,
        width: 0.0,
    }
}

impl EditorGutterView {
    fn paint_head_changes(
        &self,
        cx: &mut PaintCx,
        view: &EditorViewData,
        viewport: Rect,
        is_normal: bool,
        config: &LapceConfig,
    ) {
        if !is_normal {
            return;
        }

        let changes = view
            .doc
            .with_untracked(|doc| doc.head_changes().get_untracked());
        let line_height = config.editor.line_height() as f64;

        let changes = changes_colors_screen(view, changes);
        for (y, height, removed, color) in changes {
            let height = if removed {
                10.0
            } else {
                height as f64 * line_height
            };
            let mut y = y - viewport.y0;
            if removed {
                y -= 5.0;
            }
            cx.fill(
                &Size::new(3.0, height)
                    .to_rect()
                    .with_origin(Point::new(self.width + 7.0, y)),
                color,
                0.0,
            )
        }
    }

    fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        is_normal: bool,
        config: &LapceConfig,
    ) {
        if !is_normal {
            return;
        }

        if !config.editor.sticky_header {
            return;
        }
        let sticky_header_height = self.editor.sticky_header_height;
        let sticky_header_height = sticky_header_height.get_untracked();
        if sticky_header_height == 0.0 {
            return;
        }

        let sticky_area_rect =
            Size::new(self.width + 25.0 + 30.0, sticky_header_height)
                .to_rect()
                .with_origin(Point::new(-25.0, 0.0))
                .inflate(25.0, 0.0);
        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::LAPCE_DROPDOWN_SHADOW),
            3.0,
        );
        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
            0.0,
        );
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
        let screen_lines = self.editor.screen_lines();
        let config = self.editor.common.config;

        let kind_is_normal = self
            .editor
            .view
            .kind
            .with_untracked(|kind| kind.is_normal());
        let (offset, mode) = cursor.with_untracked(|c| (c.offset(), c.get_mode()));
        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let last_line = self.editor.view.last_line();
        let current_line = self
            .editor
            .view
            .doc
            .get_untracked()
            .buffer
            .with_untracked(|buffer| buffer.line_of_offset(offset));

        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.editor.font_family).collect();
        let attrs = Attrs::new()
            .family(&family)
            .color(config.color(LapceColor::EDITOR_DIM))
            .font_size(config.editor.font_size() as f32);
        let attrs_list = AttrsList::new(attrs);
        let current_line_attrs_list =
            AttrsList::new(attrs.color(config.color(LapceColor::EDITOR_FOREGROUND)));
        let show_relative = config.core.modal
            && config.editor.modal_mode_relative_line_numbers
            && mode != Mode::Insert
            && kind_is_normal;

        screen_lines.with_untracked(|screen_lines| {
            for (line, y) in screen_lines.iter_lines_y() {
                // If it ends up outside the bounds of the file, stop trying to display line numbers
                if line > last_line {
                    break;
                }

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

        self.paint_head_changes(
            cx,
            &self.editor.view,
            viewport,
            kind_is_normal,
            &config,
        );
        self.paint_sticky_headers(cx, kind_is_normal, &config);
    }
}
