use floem::{
    cosmic_text::{Attrs, AttrsList, LineHeightValue, TextLayout},
    event::{Event, EventListener},
    peniko::kurbo::Rect,
    reactive::{
        create_effect, create_rw_signal, SignalGet, SignalGetUntracked, SignalSet,
        SignalUpdate, SignalWith, SignalWithUntracked,
    },
    style::Style,
    view::View,
    views::{container, label, rich_text, scroll, stack, Decorators},
    ViewContext,
};
use lapce_core::buffer::rope_text::RopeText;

use crate::{config::color::LapceColor, editor::EditorData};

pub fn text_area(editor: EditorData) -> impl View {
    let cx = ViewContext::get_current();
    let config = editor.common.config;
    let keypress = editor.common.keypress;
    let doc = editor.doc;
    let cursor = editor.cursor;
    let text_area_rect = create_rw_signal(cx.scope, Rect::ZERO);
    let text_layout = create_rw_signal(cx.scope, TextLayout::new());
    let line_height = 1.2;

    create_effect(cx.scope, move |_| {
        let config = config.get();
        let font_size = config.ui.font_size();
        let font_family = config.ui.font_family();
        let color = config.get_color(LapceColor::EDITOR_FOREGROUND);
        let attrs = Attrs::new()
            .color(*color)
            .family(&font_family)
            .font_size(font_size as f32)
            .line_height(LineHeightValue::Normal(line_height));
        let attrs_list = AttrsList::new(attrs);
        let text = doc.with_untracked(|doc| doc.buffer().to_string());
        text_layout.update(|text_layout| {
            text_layout.set_text(&text, attrs_list);
        });
    });

    create_effect(cx.scope, move |last_rev| {
        let rev = doc.with(|doc| doc.rev());
        if last_rev == Some(rev) {
            return rev;
        }

        let config = config.get_untracked();
        let font_size = config.ui.font_size();
        let font_family = config.ui.font_family();
        let color = config.get_color(LapceColor::EDITOR_FOREGROUND);
        let attrs = Attrs::new()
            .color(*color)
            .family(&font_family)
            .font_size(font_size as f32)
            .line_height(LineHeightValue::Normal(1.2));
        let attrs_list = AttrsList::new(attrs);
        let text = doc.with_untracked(|doc| doc.buffer().to_string());
        text_layout.update(|text_layout| {
            text_layout.set_text(&text, attrs_list);
        });

        rev
    });

    create_effect(cx.scope, move |last_width| {
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
        let (line, col) =
            doc.with_untracked(|doc| doc.buffer().offset_to_line_col(offset));
        text_layout.with(|text_layout| {
            let pos = text_layout.line_col_position(line, col);
            pos.point - (0.0, pos.glyph_ascent)
        })
    };

    container(|| {
        scroll(|| {
            stack(|| {
                (
                    rich_text(move || text_layout.get())
                        .on_resize(move |_, rect| {
                            text_area_rect.set(rect);
                        })
                        .style(|| Style::BASE.width_pct(100.0)),
                    label(|| " ".to_string()).style(move || {
                        let cursor_pos = cursor_pos();
                        Style::BASE
                            .absolute()
                            .line_height(line_height)
                            .margin_left_px(cursor_pos.x as f32)
                            .margin_top_px(cursor_pos.y as f32)
                            .border_left(2.0)
                            .border_color(
                                *config.get().get_color(LapceColor::EDITOR_CARET),
                            )
                    }),
                )
            })
            .style(|| Style::BASE.width_pct(100.0).padding_px(6.0))
        })
        .scroll_bar_color(move || {
            *config.get().get_color(LapceColor::LAPCE_SCROLL_BAR)
        })
        .style(|| Style::BASE.absolute().size_pct(100.0, 100.0))
    })
    .keyboard_navigatable()
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            let mut press = keypress.get_untracked();
            let executed = press.key_down(key_event, &editor);
            keypress.set(press);
            executed
        } else {
            false
        }
    })
    .base_style(move || {
        let config = config.get();
        Style::BASE
            .border(1.0)
            .border_radius(6.0)
            .border_color(*config.get_color(LapceColor::LAPCE_BORDER))
            .background(*config.get_color(LapceColor::EDITOR_BACKGROUND))
    })
}
