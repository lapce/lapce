use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use druid::{
    piet::{
        PietText, PietTextLayout, Text, TextAttribute, TextLayout, TextLayoutBuilder,
    },
    PaintCtx, Point,
};
use lapce_core::{
    buffer::{Buffer, InvalLines},
    command::EditCommand,
    cursor::{ColPosition, Cursor, CursorMode},
    editor::Editor,
    mode::Mode,
    movement::{LinePosition, Movement},
    register::RegisterData,
    selection::{SelRegion, Selection},
    style::line_styles,
    syntax::Syntax,
    word::WordCursor,
};
use lapce_rpc::style::{LineStyle, LineStyles, Style};
use xi_rope::{spans::Spans, RopeDelta};

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

    fn update_styles(&mut self, delta: &RopeDelta) {
        if let Some(styles) = self.semantic_styles.as_mut() {
            Arc::make_mut(styles).apply_shape(delta);
        } else if let Some(syntax) = self.syntax.as_mut() {
            if let Some(styles) = syntax.styles.as_mut() {
                Arc::make_mut(styles).apply_shape(delta);
            }
        }

        if let Some(syntax) = self.syntax.as_mut() {
            syntax.lens.apply_delta(delta);
        }

        self.line_styles.borrow_mut().clear();
    }

    fn apply_deltas(&mut self, deltas: &[(RopeDelta, InvalLines)]) {
        for (delta, _) in deltas {
            self.update_styles(delta);
        }
        self.text_layouts.borrow_mut().clear();
    }

    pub fn do_edit(&mut self, curosr: &mut Cursor, cmd: &EditCommand, modal: bool) {
        let deltas = Editor::do_edit(
            curosr,
            &mut self.buffer,
            cmd,
            self.syntax.as_ref(),
            modal,
        );
        self.apply_deltas(&deltas)
    }

    pub fn do_paste(&mut self, cursor: &mut Cursor, data: &RegisterData) {
        let deltas = Editor::do_paste(cursor, &mut self.buffer, data);
        self.apply_deltas(&deltas)
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

    pub fn line_horiz_col(
        &self,
        text: &mut PietText,
        line: usize,
        font_size: usize,
        horiz: &ColPosition,
        caret: bool,
        config: &Config,
    ) -> usize {
        match *horiz {
            ColPosition::Col(x) => {
                let text_layout =
                    self.get_text_layout(text, line, font_size, config);
                let n = text_layout.hit_test_point(Point::new(x, 0.0)).idx;
                n.min(self.buffer.line_end_col(line, caret))
            }
            ColPosition::End => self.buffer.line_end_col(line, caret),
            ColPosition::Start => 0,
            ColPosition::FirstNonBlank => {
                self.buffer.first_non_blank_character_on_line(line)
            }
        }
    }

    fn move_region(
        &self,
        text: &mut PietText,
        region: &SelRegion,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
        font_size: usize,
        config: &Config,
    ) -> SelRegion {
        let (end, horiz) = self.move_offset(
            text,
            region.end,
            region.horiz.as_ref(),
            count,
            movement,
            mode,
            font_size,
            config,
        );
        let start = match modify {
            true => region.start(),
            false => end,
        };
        SelRegion::new(start, end, horiz)
    }

    pub fn move_cursor(
        &self,
        text: &mut PietText,
        cursor: &mut Cursor,
        movement: &Movement,
        count: usize,
        modify: bool,
        font_size: usize,
        config: &Config,
    ) {
        match cursor.mode {
            CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.move_offset(
                    text,
                    offset,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                    font_size,
                    config,
                );
                cursor.mode = CursorMode::Normal(new_offset);
                cursor.horiz = horiz;
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = self.move_offset(
                    text,
                    end,
                    cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                    font_size,
                    config,
                );
                cursor.mode = CursorMode::Visual {
                    start,
                    end: new_offset,
                    mode,
                };
                cursor.horiz = horiz;
            }
            CursorMode::Insert(ref selection) => {
                let selection = self.move_selection(
                    text,
                    selection,
                    cursor.horiz.as_ref(),
                    count,
                    modify,
                    movement,
                    Mode::Insert,
                    font_size,
                    config,
                );
                cursor.mode = CursorMode::Insert(selection);
            }
        }
    }

    fn move_selection(
        &self,
        text: &mut PietText,
        selection: &Selection,
        horiz: Option<&ColPosition>,
        count: usize,
        modify: bool,
        movement: &Movement,
        mode: Mode,
        font_size: usize,
        config: &Config,
    ) -> Selection {
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            new_selection.add_region(self.move_region(
                text, region, count, modify, movement, mode, font_size, config,
            ));
        }
        new_selection
    }

    pub fn move_offset(
        &self,
        text: &mut PietText,
        offset: usize,
        horiz: Option<&ColPosition>,
        count: usize,
        movement: &Movement,
        mode: Mode,
        font_size: usize,
        config: &Config,
    ) -> (usize, Option<ColPosition>) {
        match movement {
            Movement::Left => {
                let line = self.buffer.line_of_offset(offset);
                let line_start_offset = self.buffer.offset_of_line(line);

                let min_offset = if mode == Mode::Insert {
                    0
                } else {
                    line_start_offset
                };

                let new_offset =
                    self.buffer.prev_grapheme_offset(offset, count, min_offset);
                let (_, col) = self.buffer.offset_to_line_col(new_offset);
                (new_offset, None)
            }
            Movement::Right => {
                let line_end =
                    self.buffer.offset_line_end(offset, mode != Mode::Normal);

                let max_offset = if mode == Mode::Insert {
                    self.buffer.len()
                } else {
                    line_end
                };

                let new_offset =
                    self.buffer.next_grapheme_offset(offset, count, max_offset);

                let (_, col) = self.buffer.offset_to_line_col(new_offset);
                (new_offset, None)
            }
            Movement::Up => {
                let line = self.buffer.line_of_offset(offset);
                let line = if line == 0 {
                    0
                } else {
                    line.saturating_sub(count)
                };

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.point_of_offset(text, offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    text,
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::Down => {
                let last_line = self.buffer.last_line();
                let line = self.buffer.line_of_offset(offset);

                let line = (line + count).min(last_line);

                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.point_of_offset(text, offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    text,
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::DocumentStart => (0, Some(ColPosition::Start)),
            Movement::DocumentEnd => {
                let last_offset = self
                    .buffer
                    .offset_line_end(self.buffer.len(), mode != Mode::Normal);
                (last_offset, Some(ColPosition::End))
            }
            Movement::FirstNonBlank => {
                let line = self.buffer.line_of_offset(offset);
                let new_offset = self.buffer.first_non_blank_character_on_line(line);
                (new_offset, Some(ColPosition::FirstNonBlank))
            }
            Movement::StartOfLine => {
                let line = self.buffer.line_of_offset(offset);
                let new_offset = self.buffer.offset_of_line(line);
                (new_offset, Some(ColPosition::Start))
            }
            Movement::EndOfLine => {
                let new_offset =
                    self.buffer.offset_line_end(offset, mode != Mode::Normal);
                (new_offset, Some(ColPosition::End))
            }
            Movement::Line(position) => {
                let line = match position {
                    LinePosition::Line(line) => {
                        (line - 1).min(self.buffer.last_line())
                    }
                    LinePosition::First => 0,
                    LinePosition::Last => self.buffer.last_line(),
                };
                let horiz = horiz.cloned().unwrap_or_else(|| {
                    ColPosition::Col(
                        self.point_of_offset(text, offset, font_size, config).x,
                    )
                });
                let col = self.line_horiz_col(
                    text,
                    line,
                    font_size,
                    &horiz,
                    mode != Mode::Normal,
                    config,
                );
                let new_offset = self.buffer.offset_of_line_col(line, col);
                (new_offset, Some(horiz))
            }
            Movement::Offset(offset) => {
                let new_offset = *offset;
                let new_offset = self
                    .buffer
                    .text()
                    .prev_grapheme_offset(new_offset + 1)
                    .unwrap();
                let (_, col) = self.buffer.offset_to_line_col(new_offset);
                (new_offset, None)
            }
            Movement::WordEndForward => {
                let mut new_offset = WordCursor::new(self.buffer.text(), offset)
                    .end_boundary()
                    .unwrap_or(offset);
                if mode != Mode::Insert {
                    new_offset = self.buffer.prev_grapheme_offset(new_offset, 1, 0);
                }
                let (_, col) = self.buffer.offset_to_line_col(new_offset);
                (new_offset, None)
            }
            Movement::WordForward => {
                let new_offset = WordCursor::new(self.buffer.text(), offset)
                    .next_boundary()
                    .unwrap_or(offset);
                let (_, col) = self.buffer.offset_to_line_col(new_offset);
                (new_offset, None)
            }
            Movement::WordBackward => {
                let new_offset = WordCursor::new(self.buffer.text(), offset)
                    .prev_boundary()
                    .unwrap_or(offset);
                let (_, col) = self.buffer.offset_to_line_col(new_offset);
                (new_offset, None)
            }
            Movement::NextUnmatched(c) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, false, &c.to_string())
                        .unwrap_or(offset);
                    let (_, col) = self.buffer.offset_to_line_col(new_offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .next_unmatched(*c)
                        .map_or(offset, |new| new - 1);
                    let (_, col) = self.buffer.offset_to_line_col(new_offset);
                    (new_offset, None)
                }
            }
            Movement::PreviousUnmatched(c) => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset = syntax
                        .find_tag(offset, true, &c.to_string())
                        .unwrap_or(offset);
                    let (_, col) = self.buffer.offset_to_line_col(new_offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .previous_unmatched(*c)
                        .unwrap_or(offset);
                    let (_, col) = self.buffer.offset_to_line_col(new_offset);
                    (new_offset, None)
                }
            }
            Movement::MatchPairs => {
                if let Some(syntax) = self.syntax.as_ref() {
                    let new_offset =
                        syntax.find_matching_pair(offset).unwrap_or(offset);
                    let (_, col) = self.buffer.offset_to_line_col(new_offset);
                    (new_offset, None)
                } else {
                    let new_offset = WordCursor::new(self.buffer.text(), offset)
                        .match_pairs()
                        .unwrap_or(offset);
                    let (_, col) = self.buffer.offset_to_line_col(new_offset);
                    (new_offset, None)
                }
            }
        }
    }
}
