use std::{cell::RefCell, collections::HashMap, sync::Arc};

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use floem::{
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout, Wrap},
    reactive::Scope,
};
use lapce_app::{
    doc::phantom_text::PhantomTextLine,
    editor::{
        view_data::TextLayoutLine,
        visual_line::{
            FontSizeCacheId, LineFontSizeProvider, Lines, ResolvedWrap,
            TextLayoutProvider, VLine,
        },
    },
};
use lapce_core::{
    buffer::rope_text::{RopeText, RopeTextRef},
    cursor::CursorAffinity,
};
use lapce_xi_rope::Rope;

const FONT_SIZE: usize = 12;

// TODO: use the editor data view structures!
struct TLProv<'a> {
    text: &'a Rope,
    phantom: HashMap<usize, PhantomTextLine>,
    font_family: Vec<FamilyOwned>,
    has_multiline_phantom: bool,
}
impl<'a> TextLayoutProvider for TLProv<'a> {
    fn text(&self) -> &Rope {
        self.text
    }

    // An implementation relatively close to the actual new text layout impl but simplified.
    // TODO(minor): It would be nice to just use the same impl as view's
    fn new_text_layout(
        &self,
        line: usize,
        font_size: usize,
        wrap: ResolvedWrap,
    ) -> Arc<TextLayoutLine> {
        let rope_text = RopeTextRef::new(self.text);
        let line_content_original = rope_text.line_content(line);

        // Get the line content with newline characters replaced with spaces
        // and the content without the newline characters
        let (line_content, _line_content_original) =
            if let Some(s) = line_content_original.strip_suffix("\r\n") {
                (
                    format!("{s}  "),
                    &line_content_original[..line_content_original.len() - 2],
                )
            } else if let Some(s) = line_content_original.strip_suffix('\n') {
                (
                    format!("{s} ",),
                    &line_content_original[..line_content_original.len() - 1],
                )
            } else {
                (
                    line_content_original.to_string(),
                    &line_content_original[..],
                )
            };

        let phantom_text = self.phantom.get(&line).cloned().unwrap_or_default();
        let line_content = phantom_text.combine_with_text(line_content);

        let attrs = Attrs::new()
            .family(&self.font_family)
            .font_size(font_size as f32);
        let mut attrs_list = AttrsList::new(attrs);

        // We don't do line styles, since they aren't relevant

        // Apply phantom text specific styling
        for (offset, size, col, phantom) in phantom_text.offset_size_iter() {
            let start = col + offset;
            let end = start + size;

            let mut attrs = attrs;
            if let Some(fg) = phantom.fg {
                attrs = attrs.color(fg);
            }
            if let Some(phantom_font_size) = phantom.font_size {
                attrs = attrs.font_size(phantom_font_size.min(font_size) as f32);
            }
            attrs_list.add_span(start..end, attrs);
            // if let Some(font_family) = phantom.font_family.clone() {
            //     layout_builder = layout_builder.range_attribute(
            //         start..end,
            //         TextAttribute::FontFamily(font_family),
            //     );
            // }
        }

        let mut text_layout = TextLayout::new();
        text_layout.set_wrap(Wrap::Word);
        match wrap {
            // We do not have to set the wrap mode if we do not set the width
            ResolvedWrap::None => {}
            ResolvedWrap::Column(_col) => todo!(),
            ResolvedWrap::Width(px) => {
                text_layout.set_size(px, f32::MAX);
            }
        }
        text_layout.set_text(&line_content, attrs_list);

        // skip phantom text background styling because it doesn't shift positions
        // skip severity styling
        // skip diagnostic background styling

        Arc::new(TextLayoutLine {
            extra_style: Vec::new(),
            text: text_layout,
            whitespaces: None,
            indent: 0.0,
        })
    }

    fn before_phantom_col(&self, line: usize, col: usize) -> usize {
        if let Some(phantom) = self.phantom.get(&line) {
            phantom.before_col(col)
        } else {
            col
        }
    }

    fn has_multiline_phantom(&self) -> bool {
        self.has_multiline_phantom
    }
}
struct TestFontSize;
impl LineFontSizeProvider for TestFontSize {
    fn font_size(&self, _line: usize) -> usize {
        FONT_SIZE
    }

    fn cache_id(&self) -> FontSizeCacheId {
        0
    }
}

fn make_lines(text: &Rope, wrap: ResolvedWrap, init: bool) -> (TLProv<'_>, Lines) {
    make_lines_ph(text, wrap, init, HashMap::new(), false)
}

fn make_lines_ph(
    text: &Rope,
    wrap: ResolvedWrap,
    init: bool,
    ph: HashMap<usize, PhantomTextLine>,
    has_multiline_phantom: bool,
) -> (TLProv<'_>, Lines) {
    // let wrap = Wrap::Word;
    // let r_wrap = ResolvedWrap::Width(width);
    let font_sizes = TestFontSize;
    let text = TLProv {
        text,
        phantom: ph,
        font_family: Vec::new(),
        has_multiline_phantom,
    };
    let cx = Scope::new();
    let lines = Lines::new(cx, RefCell::new(Arc::new(font_sizes)));
    lines.set_wrap(wrap);

    if init {
        let config_id = 0;
        lines.init_all(config_id, &text, true);
    }

    (text, lines)
}

fn medium_rope() -> Rope {
    let mut text = String::new();

    // TODO: use some actual file's content.
    for i in 0..3000 {
        let content = if i % 2 == 0 {
            "This is a roughly typical line of text\n"
        } else if i % 3 == 0 {
            "\n"
        } else {
            "A short line\n"
        };

        text.push_str(content);
    }

    Rope::from(&text)
}

fn visual_line(c: &mut Criterion) {
    let text = medium_rope();

    // Should be very fast because it is trivially linear and there's no multiline phantom
    c.bench_function("last vline (uninit)", |b| {
        let (text_prov, lines) = make_lines(&text, ResolvedWrap::None, false);
        b.iter(|| {
            lines.clear_last_vline();

            let last_vline = lines.last_vline(&text_prov);
            black_box(last_vline);
        })
    });

    // Unrealistic since the user will very rarely have all of the lines initialized
    // Should still be fast because there's no wrapping or multiline phantom text
    c.bench_function("last vline (all, no wrapping)", |b| {
        let (text_prov, lines) = make_lines(&text, ResolvedWrap::None, true);
        b.iter(|| {
            lines.clear_last_vline();

            let last_vline = lines.last_vline(&text_prov);
            let _val = black_box(last_vline);
        })
    });

    // TODO: we could precompute line count on the text layout?
    // Unrealistic since the user will very rarely have all of the lines initialized
    // Still decently fast, though. <1ms
    c.bench_function("last vline (all, wrapping)", |b| {
        let width = 100.0;
        let (text_prov, lines) = make_lines(&text, ResolvedWrap::Width(width), true);

        b.iter(|| {
            // This should clear any other caching mechanisms that get added
            lines.clear_last_vline();
            let last_vline = lines.last_vline(&text_prov);
            let _val = black_box(last_vline);
        })
    });

    // Q: This seems like 1/5th the cost of last vline despite only being half the lines..
    c.bench_function("vline of offset (all, wrapping)", |b| {
        let width = 100.0;
        let (text_prov, lines) = make_lines(&text, ResolvedWrap::Width(width), true);

        // Not past the middle. If we were past the middle then it'd be just benching last vline
        // calculation, which is admittedly relatively similar to this.
        let offset = 1450;

        b.iter(|| {
            // This should clear any other caching mechanisms that get added
            lines.clear_last_vline();

            let vline =
                lines.vline_of_offset(&text_prov, offset, CursorAffinity::Backward);
            let _val = black_box(vline);
        })
    });

    c.bench_function("offset of vline (all, wrapping)", |b| {
        let width = 100.0;
        let (text_prov, lines) = make_lines(&text, ResolvedWrap::Width(width), true);

        let vline = VLine(3300);

        b.iter(|| {
            // This should clear any other caching mechanisms that get added
            lines.clear_last_vline();

            let offset = lines.offset_of_vline(&text_prov, vline);
            let _val = black_box(offset);
        })
    });

    // TODO: when we have the reverse search of vline for offset, we should have a separate instance where the last vline isn't cached, which would give us a range of 'worst case' (never reused, have to recopmute it everytime) and 'best case' (always reused)

    // TODO: bench common operations, like a single line changing or the (equivalent of)  cache rev
    // updating.
}

criterion_group!(benches, visual_line);
criterion_main!(benches);
