use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use druid::{
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    PaintCtx, Point,
};
use lapce_core::{
    buffer::Buffer, command::EditCommand, cursor::Cursor, editor::Editor,
    style::line_styles, syntax::Syntax,
};
use lapce_rpc::style::{LineStyle, LineStyles, Style};
use xi_rope::spans::Spans;

use crate::config::{Config, LapceTheme};

#[derive(Clone)]
pub struct Document {
    buffer: Buffer,
    syntax: Option<Syntax>,
    line_styles: Rc<RefCell<LineStyles>>,
    semantic_styles: Option<Arc<Spans<Style>>>,
    text_layouts: Rc<RefCell<HashMap<usize, Arc<PietTextLayout>>>>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(""),
            syntax: None,
            line_styles: Rc::new(RefCell::new(HashMap::new())),
            text_layouts: Rc::new(RefCell::new(HashMap::new())),
            semantic_styles: None,
        }
    }

    pub fn load_content(&mut self, content: &str) {
        self.buffer.load_content(content);
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    pub fn syntax(&self) -> Option<&Syntax> {
        self.syntax.as_ref()
    }

    pub fn do_edit(&mut self, curosr: &mut Cursor, cmd: &EditCommand) {
        Editor::do_edit(curosr, &mut self.buffer, cmd, self.syntax.as_ref());
    }

    fn line_style(&self, line: usize) -> Arc<Vec<LineStyle>> {
        if self.line_styles.borrow().get(&line).is_none() {
            let styles = self
                .semantic_styles
                .as_ref()
                .or_else(|| self.syntax().and_then(|s| s.styles.as_ref()));

            let line_styles = styles
                .map(|styles| line_styles(self.buffer.text(), line, styles))
                .unwrap_or_default();
            self.line_styles
                .borrow_mut()
                .insert(line, Arc::new(line_styles));
        }
        self.line_styles.borrow().get(&line).cloned().unwrap()
    }

    pub fn point_of_line_col(
        &self,
        text: &mut PietText,
        line: usize,
        col: usize,
        font_size: usize,
        config: &Config,
    ) -> Point {
        let text_layout = self.get_text_layout(text, line, font_size, config);
        text_layout.hit_test_text_position(col).point
    }

    pub fn point_of_offset(
        &self,
        text: &mut PietText,
        offset: usize,
        font_size: usize,
        config: &Config,
    ) -> Point {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let text_layout = self.get_text_layout(text, line, font_size, config);
        text_layout.hit_test_text_position(col).point
    }

    pub fn get_text_layout(
        &self,
        text: &mut PietText,
        line: usize,
        font_size: usize,
        config: &Config,
    ) -> Arc<PietTextLayout> {
        if self.text_layouts.borrow().get(&line).is_none() {
            self.text_layouts.borrow_mut().insert(
                line,
                Arc::new(self.new_text_layout(text, line, font_size, config)),
            );
        }
        self.text_layouts.borrow().get(&line).cloned().unwrap()
    }

    fn new_text_layout(
        &self,
        text: &mut PietText,
        line: usize,
        font_size: usize,
        config: &Config,
    ) -> PietTextLayout {
        let line_content = self.buffer.line_content(line);
        let mut layout_builder = text
            .new_text_layout(line_content.to_string())
            .font(config.editor.font_family(), font_size as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            );

        let styles = self.line_style(line);
        for line_style in styles.iter() {
            if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                if let Some(fg_color) = config.get_style_color(fg_color) {
                    layout_builder = layout_builder.range_attribute(
                        line_style.start..line_style.end,
                        TextAttribute::TextColor(fg_color.clone()),
                    );
                }
            }
        }

        layout_builder.build().unwrap()
    }
}
