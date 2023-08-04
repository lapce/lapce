use floem::{
    context::PaintCx,
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    id::Id,
    peniko::kurbo::{Point, Rect, Size},
    reactive::RwSignal,
    view::{ChangeFlags, View},
    Renderer, ViewContext,
};
use lapce_core::{buffer::rope_text::RopeText, mode::Mode};

use crate::{
    config::{color::LapceColor, LapceConfig},
    doc::Document,
};

use super::{view::changes_colors, EditorData};

pub struct EditorGutterView {
    id: Id,
    editor: RwSignal<EditorData>,
    width: f64,
}

pub fn editor_gutter_view(editor: RwSignal<EditorData>) -> EditorGutterView {
    let cx = ViewContext::get_current();
    let id = cx.new_id();

    EditorGutterView {
        id,
        editor,
        width: 0.0,
    }
}

impl EditorGutterView {
    fn paint_head_changes(
        &self,
        cx: &mut PaintCx,
        doc: RwSignal<Document>,
        viewport: Rect,
        is_normal: bool,
        config: &LapceConfig,
    ) {
        if !is_normal {
            return;
        }

        let changes = doc.with_untracked(|doc| doc.head_changes);
        let changes = changes.get();
        let line_height = config.editor.line_height() as f64;

        let min_line = (viewport.y0 / line_height).floor() as usize;
        let max_line = (viewport.y1 / line_height).ceil() as usize;

        let changes = changes_colors(changes, min_line, max_line, config);
        for (y, height, removed, color) in changes {
            let height = if removed {
                10.0
            } else {
                height as f64 * line_height
            };
            let mut y = y as f64 * line_height - viewport.y0;
            if removed {
                y -= 5.0;
            }
            cx.fill(
                &Size::new(3.0, height)
                    .to_rect()
                    .with_origin(Point::new(self.width + 7.0, y)),
                color,
            )
        }
    }
}

impl View for EditorGutterView {
    fn id(&self) -> Id {
        self.id
    }

    fn child(&self, _id: Id) -> Option<&dyn View> {
        None
    }

    fn child_mut(&mut self, _id: Id) -> Option<&mut dyn View> {
        None
    }

    fn children(&self) -> Vec<&dyn View> {
        Vec::new()
    }

    fn children_mut(&mut self) -> Vec<&mut dyn View> {
        Vec::new()
    }

    fn update(
        &mut self,
        _cx: &mut floem::context::UpdateCx,
        _state: Box<dyn std::any::Any>,
    ) -> ChangeFlags {
        ChangeFlags::default()
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::Node {
        cx.layout_node(self.id, false, |_| Vec::new())
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> Option<floem::peniko::kurbo::Rect> {
        if let Some(width) = cx.get_layout(self.id).map(|l| l.size.width as f64) {
            self.width = width;
        }
        None
    }

    fn event(
        &mut self,
        _cx: &mut floem::context::EventCx,
        _id_path: Option<&[Id]>,
        _event: floem::event::Event,
    ) -> bool {
        false
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        let (view, cursor, viewport, screen_lines, config) =
            self.editor.with_untracked(|editor| {
                (
                    editor.view.clone(),
                    editor.cursor,
                    editor.viewport,
                    editor.screen_lines(),
                    editor.common.config,
                )
            });
        let viewport = viewport.get_untracked();

        let kind_is_normal = view.kind.with_untracked(|kind| kind.is_normal());
        let (offset, mode) = cursor.with_untracked(|c| (c.offset(), c.get_mode()));
        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let last_line = view.last_line();
        let current_line = view
            .doc
            .with_untracked(|doc| doc.buffer().line_of_offset(offset));

        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.editor.font_family).collect();
        let attrs = Attrs::new()
            .family(&family)
            .color(*config.get_color(LapceColor::EDITOR_DIM))
            .font_size(config.editor.font_size() as f32);
        let attrs_list = AttrsList::new(attrs);
        let current_line_attrs_list = AttrsList::new(
            attrs.color(*config.get_color(LapceColor::EDITOR_FOREGROUND)),
        );
        let show_relative = config.core.modal
            && config.editor.modal_mode_relative_line_numbers
            && mode != Mode::Insert
            && kind_is_normal;

        for line in &screen_lines.lines {
            let line = *line;
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

            let info = screen_lines.info.get(&line).unwrap();
            let mut text_layout = TextLayout::new();
            if line == current_line {
                text_layout.set_text(&text, current_line_attrs_list.clone());
            } else {
                text_layout.set_text(&text, attrs_list.clone());
            }
            let size = text_layout.size();
            let height = size.height;
            let y = info.y;

            cx.draw_text(
                &text_layout,
                Point::new(
                    (self.width - (size.width)).max(0.0),
                    y as f64 + (line_height - height) / 2.0 - viewport.y0,
                ),
            );
        }

        self.paint_head_changes(cx, view.doc, viewport, kind_is_normal, &config);
    }
}
