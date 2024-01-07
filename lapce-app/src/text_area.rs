use floem::{
    cosmic_text::{Attrs, AttrsList, LineHeightValue, TextLayout},
    peniko::kurbo::Rect,
    reactive::{create_effect, create_rw_signal},
    view::View,
    views::{container, label, rich_text, scroll, stack, Decorators},
};
use lapce_core::buffer::rope_text::RopeText;

use crate::{config::color::LapceColor, editor::EditorData};

pub fn text_area(
    editor: EditorData,
    is_active: impl Fn() -> bool + 'static,
) -> impl View {
    let config = editor.common.config;
    let doc = editor.view.doc;
    let cursor = editor.cursor;
    let text_area_rect = create_rw_signal(Rect::ZERO);
    let text_layout = create_rw_signal(TextLayout::new());
    let line_height = 1.2;

    create_effect(move |_| {
        let config = config.get();
        let font_size = config.ui.font_size();
        let font_family = config.ui.font_family();
        let color = config.color(LapceColor::EDITOR_FOREGROUND);
        let attrs = Attrs::new()
            .color(color)
            .family(&font_family)
            .font_size(font_size as f32)
            .line_height(LineHeightValue::Normal(line_height));
        let attrs_list = AttrsList::new(attrs);
        let doc = doc.get();
        let text = doc.buffer.with(|b| b.to_string());
        text_layout.update(|text_layout| {
            text_layout.set_text(&text, attrs_list);
        });
    });

    create_effect(move |last_rev| {
        let rev = doc.with(|doc| doc.rev());
        if last_rev == Some(rev) {
            return rev;
        }

        let config = config.get_untracked();
        let font_size = config.ui.font_size();
        let font_family = config.ui.font_family();
        let color = config.color(LapceColor::EDITOR_FOREGROUND);
        let attrs = Attrs::new()
            .color(color)
            .family(&font_family)
            .font_size(font_size as f32)
            .line_height(LineHeightValue::Normal(1.2));
        let attrs_list = AttrsList::new(attrs);
        let doc = doc.get();
        let text = doc.buffer.with(|b| b.to_string());
        text_layout.update(|text_layout| {
            text_layout.set_text(&text, attrs_list);
        });

        rev
    });

    create_effect(move |last_width| {
        let width = text_area_rect.get().width();
        if last_width == Some(width) {
            return width;
        }

        text_layout.update(|text_layout| {
            text_layout.set_size(width as f32, f32::MAX);
        });

        width
    });

    let cursor_pos = move || {
        let offset = cursor.with(|c| c.offset());
        let (line, col) = doc
            .with_untracked(|doc| doc.buffer.with(|b| b.offset_to_line_col(offset)));
        text_layout.with(|text_layout| {
            let pos = text_layout.line_col_position(line, col);
            pos.point - (0.0, pos.glyph_ascent)
        })
    };

    container(
        scroll(
            stack((
                rich_text(move || text_layout.get())
                    .on_resize(move |rect| {
                        text_area_rect.set(rect);
                    })
                    .style(|s| s.width_pct(100.0)),
                label(|| " ".to_string()).style(move |s| {
                    let cursor_pos = cursor_pos();
                    s.absolute()
                        .line_height(line_height)
                        .margin_left(cursor_pos.x as f32 - 1.0)
                        .margin_top(cursor_pos.y as f32)
                        .border_left(2.0)
                        .border_color(config.get().color(LapceColor::EDITOR_CARET))
                        .apply_if(!is_active(), |s| s.hide())
                }),
            ))
            .style(|s| s.width_pct(100.0).padding(6.0)),
        )
        .style(|s| s.absolute().size_pct(100.0, 100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    })
}
