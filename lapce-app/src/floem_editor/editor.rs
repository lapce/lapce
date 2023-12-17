use std::{rc::Rc, sync::Arc};

use floem::{
    cosmic_text::{Attrs, AttrsList, LineHeightValue, TextLayout, Wrap},
    kurbo::Rect,
    reactive::{RwSignal, Scope},
};
use lapce_core::buffer::rope_text::RopeText;
use lapce_xi_rope::Rope;

use crate::editor::{
    view_data::TextLayoutLine,
    visual_line::{Lines, ResolvedWrap, TextLayoutProvider},
};

use super::text::{Document, Styling, WrapMethod};
/// The data for a specific editor view
#[derive(Clone)]
pub struct Editor {
    /// Whether you can edit within this editor.
    read_only: bool,
    doc: RwSignal<Rc<dyn Document>>,
    style: RwSignal<Rc<dyn Styling>>,
    /// Holds the cache of the lines and provides many utility functions for them.
    lines: Rc<Lines>,

    viewport: RwSignal<Rect>,
}
impl Editor {
    pub fn new(cx: Scope) -> Editor {
        let cx = cx.create_child();

        todo!()
    }

    // Get the text layout for a document line, creating it if needed.
    pub(crate) fn text_layout(&self, line: usize) -> Arc<TextLayoutLine> {
        self.text_layout_trigger(line, true)
    }

    fn text_prov(&self) -> EditorTextProv {
        let doc = self.doc.get_untracked();
        EditorTextProv {
            text: doc.text(),
            doc,
            style: self.style.get_untracked(),
            viewport: self.viewport.get_untracked(),
        }
    }

    pub(crate) fn text_layout_trigger(
        &self,
        line: usize,
        trigger: bool,
    ) -> Arc<TextLayoutLine> {
        // TODO: config id
        let config_id = 0;
        let text_prov = self.text_prov();
        self.lines
            .get_init_text_layout(config_id, &text_prov, line, trigger)
    }
}

struct EditorTextProv {
    text: Rope,
    doc: Rc<dyn Document>,
    style: Rc<dyn Styling>,

    viewport: Rect,
}
impl TextLayoutProvider for EditorTextProv {
    // TODO: should this just return a `Rope`, or should `Document::text` return a `&Rope`?
    fn text(&self) -> &Rope {
        &self.text
    }

    fn new_text_layout(
        &self,
        line: usize,
        _font_size: usize,
        _wrap: ResolvedWrap,
    ) -> Arc<TextLayoutLine> {
        // TODO: we could share text layouts between different editor views given some knowledge of
        // their wrapping
        let text = self.rope_text();

        let line_content_original = text.line_content(line);

        let font_size = self.style.font_size(self.style.font_size(line));

        // Get the line content with newline characters replaced with spaces
        // and the content without the newline characters
        // TODO: cache or add some way that text layout is created to auto insert the spaces instead
        // though we immediately combine with phantom text so that's a thing.
        let line_content =
            if let Some(s) = line_content_original.strip_suffix("\r\n") {
                format!("{s}  ")
            } else if let Some(s) = line_content_original.strip_suffix('\n') {
                format!("{s} ",)
            } else {
                line_content_original.to_string()
            };
        // Combine the phantom text with the line content
        let phantom_text = self.doc.phantom_text(line);
        let line_content = phantom_text.combine_with_text(&line_content);

        let family = self.style.font_family(line);
        let attrs = Attrs::new()
            .color(self.style.foreground(line))
            .family(&family)
            .font_size(font_size as f32)
            .line_height(LineHeightValue::Px(self.style.line_height(line)));
        let mut attrs_list = AttrsList::new(attrs);

        self.style.apply_attr_styles(line, attrs, &mut attrs_list);

        let mut text_layout = TextLayout::new();
        // TODO: we could move tab width setting to be done by the document
        text_layout.set_tab_width(self.style.tab_width(line));
        text_layout.set_text(&line_content, attrs_list);

        match self.style.wrap(line) {
            WrapMethod::None => {}
            WrapMethod::EditorWidth => {
                text_layout.set_wrap(Wrap::Word);
                text_layout.set_size(self.viewport.width() as f32, f32::MAX);
            }
            WrapMethod::WrapWidth { width } => {
                text_layout.set_wrap(Wrap::Word);
                text_layout.set_size(width, f32::MAX);
            }
            // TODO:
            WrapMethod::WrapColumn { .. } => {}
        }

        // TODO(floem-editor):
        // let whitespaces = Self::new_whitespace_layout(
        //     line_content_original,
        //     &text_layout,
        //     &phantom_text,
        //     styling.render_whitespace(),
        // );

        // let indent_line = B::indent_line(self, line, line_content_original);

        // let indent = if indent_line != line {
        //     self.get_text_layout(indent_line, font_size).indent + 1.0
        // } else {
        //     let (_, col) = self.buffer.with_untracked(|buffer| {
        //         let offset = buffer.first_non_blank_character_on_line(indent_line);
        //         buffer.offset_to_line_col(offset)
        //     });
        //     text_layout.hit_position(col).point.x
        // };
        let whitespaces = None;
        let indent = 0.0;

        let mut layout_line = TextLayoutLine {
            text: text_layout,
            extra_style: Vec::new(),
            whitespaces,
            indent,
        };
        self.style.apply_layout_styles(line, &mut layout_line);

        Arc::new(layout_line)
    }

    // TODO: doc has these two functions, should we just make it a common subtrait for having
    // phantom text?
    fn before_phantom_col(&self, line: usize, col: usize) -> usize {
        self.doc.before_phantom_col(line, col)
    }

    fn has_multiline_phantom(&self) -> bool {
        self.doc.has_multiline_phantom()
    }
}
